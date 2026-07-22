use crate::common::{calculate_cost, non_empty_string, provider_event_error, read_json_http_error};
use crate::{
    ProviderAdapterConfig, ProviderClock, ProviderHttpClient, ProviderHttpResponse, SseDecoder,
    SseEvent, SystemProviderClock,
};
use futures_util::stream;
use pi_model::{
    CacheRetention, Cancellation, ModelEventStream, ModelFuture, ModelRequest, ModelService,
    ModelServiceError, ModelServiceErrorCategory, ThinkingLevel, ToolChoice,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, CompletionReason, ContentBlock, Extensions, Message,
    MessageContent, StopReason, TextBlock, ThinkingBlock, ToolCallBlock, Usage,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

const MAX_SSE_EVENT_BYTES: usize = 1_048_576;

/// Direct OpenAI Chat Completions protocol adapter.
#[derive(Clone)]
pub struct OpenAiChatAdapter {
    http: ProviderHttpClient,
    clock: Arc<dyn ProviderClock>,
}

impl OpenAiChatAdapter {
    pub fn new(config: &ProviderAdapterConfig) -> Result<Self, ModelServiceError> {
        Ok(Self {
            http: ProviderHttpClient::new(config)?,
            clock: Arc::new(SystemProviderClock),
        })
    }

    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn ProviderClock>) -> Self {
        self.clock = clock;
        self
    }
}

impl ModelService for OpenAiChatAdapter {
    fn stream<'a>(
        &'a self,
        request: ModelRequest,
        cancellation: &'a dyn Cancellation,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            request.options.validate()?;
            let payload = build_request(&request)?;
            let response = self
                .http
                .post_json("chat/completions", &payload, cancellation)
                .await?;
            if !(200..300).contains(&response.status()) {
                return Err(read_json_http_error(&self.http, response, cancellation).await);
            }

            let output = AssistantMessage::new(
                Vec::new(),
                request.model.api.clone(),
                request.model.provider.clone(),
                request.model.id.clone(),
                Usage::default(),
                StopReason::Stop,
                self.clock.now_ms(),
            );
            let state = OpenAiChatStreamState::new(
                response,
                cancellation,
                self.http.clone(),
                request.model,
                output,
            );
            let event_stream = stream::unfold(state, |mut state| async move {
                let item = state.next_item().await?;
                Some((item, state))
            });
            Ok(ModelEventStream::new(event_stream))
        })
    }
}

struct StreamingToolCall {
    content_index: usize,
    id: String,
    name: String,
    partial_arguments: String,
}

struct OpenAiChatStreamState<'a> {
    response: ProviderHttpResponse,
    cancellation: &'a dyn Cancellation,
    http: ProviderHttpClient,
    model: pi_protocol::Model,
    decoder: SseDecoder,
    output: AssistantMessage,
    pending: VecDeque<Result<AssistantMessageEvent, ModelServiceError>>,
    text_index: Option<usize>,
    thinking_index: Option<usize>,
    tool_calls: BTreeMap<usize, StreamingToolCall>,
    tool_keys_by_id: BTreeMap<String, usize>,
    pending_reasoning_details: BTreeMap<String, String>,
    next_tool_key: usize,
    finish_reason: Option<CompletionReason>,
    terminated: bool,
}

impl<'a> OpenAiChatStreamState<'a> {
    fn new(
        response: ProviderHttpResponse,
        cancellation: &'a dyn Cancellation,
        http: ProviderHttpClient,
        model: pi_protocol::Model,
        output: AssistantMessage,
    ) -> Self {
        let mut pending = VecDeque::new();
        pending.push_back(Ok(AssistantMessageEvent::Start {
            partial: output.clone(),
            extensions: Extensions::new(),
        }));
        Self {
            response,
            cancellation,
            http,
            model,
            decoder: SseDecoder::new(MAX_SSE_EVENT_BYTES),
            output,
            pending,
            text_index: None,
            thinking_index: None,
            tool_calls: BTreeMap::new(),
            tool_keys_by_id: BTreeMap::new(),
            pending_reasoning_details: BTreeMap::new(),
            next_tool_key: 0,
            finish_reason: None,
            terminated: false,
        }
    }

    async fn next_item(&mut self) -> Option<Result<AssistantMessageEvent, ModelServiceError>> {
        if let Some(item) = self.pending.pop_front() {
            return Some(item);
        }
        if self.terminated {
            return None;
        }

        loop {
            match self.response.next_chunk(self.cancellation).await {
                Ok(Some(chunk)) => match self.decoder.push(&chunk) {
                    Ok(events) => self.process_sse_events(events),
                    Err(error) => self.fail(error),
                },
                Ok(None) => {
                    match self.decoder.finish() {
                        Ok(events) => self.process_sse_events(events),
                        Err(error) => self.fail(error),
                    }
                    if !self.terminated {
                        if self.finish_reason.is_some() {
                            self.finalize();
                        } else {
                            self.fail(ModelServiceError::protocol(
                                "OpenAI Chat stream ended without finish_reason",
                                true,
                            ));
                        }
                    }
                }
                Err(error) => self.fail(error),
            }
            if let Some(item) = self.pending.pop_front() {
                return Some(item);
            }
            if self.terminated {
                return None;
            }
        }
    }

    fn process_sse_events(&mut self, events: Vec<SseEvent>) {
        for event in events {
            if self.terminated {
                break;
            }
            let data = event.data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                if self.finish_reason.is_some() {
                    self.finalize();
                } else {
                    self.fail(ModelServiceError::protocol(
                        "OpenAI Chat stream ended without finish_reason",
                        true,
                    ));
                }
                continue;
            }
            let chunk: Value = match serde_json::from_str(data) {
                Ok(chunk) => chunk,
                Err(_) => {
                    self.fail(ModelServiceError::protocol(
                        "OpenAI Chat stream contained malformed JSON",
                        false,
                    ));
                    continue;
                }
            };
            if let Some(error) = chunk.get("error") {
                self.fail(provider_event_error(
                    &self.http,
                    error,
                    "provider stream error",
                ));
                continue;
            }
            if let Err(error) = self.process_chunk(&chunk) {
                self.fail(error);
            }
        }
    }

    fn process_chunk(&mut self, chunk: &Value) -> Result<(), ModelServiceError> {
        if self.output.response_id.is_none() {
            self.output.response_id = non_empty_string(chunk.get("id"));
        }
        if self.output.response_model.is_none() {
            let response_model = non_empty_string(chunk.get("model"));
            if response_model.as_deref() != Some(self.model.id.as_str()) {
                self.output.response_model = response_model;
            }
        }
        if let Some(usage) = chunk.get("usage") {
            self.output.usage = parse_usage(usage, &self.model);
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return Ok(());
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            let reason = map_finish_reason(reason)?;
            self.finish_reason = Some(reason);
            self.output.stop_reason = completion_stop_reason(reason);
        }
        let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
            return Ok(());
        };
        if let Some(content) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            self.push_text(content);
        }
        for field in ["reasoning_content", "reasoning", "reasoning_text"] {
            if let Some(reasoning) = delta
                .get(field)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                self.push_thinking(reasoning, field);
                break;
            }
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, tool_call) in tool_calls.iter().enumerate() {
                self.push_tool_call(tool_call, position)?;
            }
        }
        if let Some(details) = delta.get("reasoning_details").and_then(Value::as_array) {
            for detail in details {
                if let (Some(id), Some("reasoning.encrypted"), Some(data)) = (
                    detail.get("id").and_then(Value::as_str),
                    detail.get("type").and_then(Value::as_str),
                    detail.get("data").and_then(Value::as_str),
                ) && !id.is_empty()
                    && !data.is_empty()
                {
                    let serialized = serde_json::to_string(detail).map_err(|_| {
                        ModelServiceError::protocol(
                            "reasoning detail could not be serialized",
                            false,
                        )
                    })?;
                    if let Some(key) = self.tool_keys_by_id.get(id).copied() {
                        self.set_tool_signature(key, serialized);
                    } else {
                        self.pending_reasoning_details
                            .insert(id.to_owned(), serialized);
                    }
                }
            }
        }
        Ok(())
    }

    fn push_text(&mut self, delta: &str) {
        let content_index = match self.text_index {
            Some(index) => index,
            None => {
                let index = self.output.content.len();
                self.output
                    .content
                    .push(ContentBlock::Text(TextBlock::new("")));
                self.text_index = Some(index);
                self.pending.push_back(Ok(AssistantMessageEvent::TextStart {
                    content_index: index,
                    partial: self.output.clone(),
                }));
                index
            }
        };
        if let Some(ContentBlock::Text(block)) = self.output.content.get_mut(content_index) {
            block.text.push_str(delta);
        }
        self.pending.push_back(Ok(AssistantMessageEvent::TextDelta {
            content_index,
            delta: delta.to_owned(),
            partial: self.output.clone(),
        }));
    }

    fn push_thinking(&mut self, delta: &str, signature: &str) {
        let content_index = match self.thinking_index {
            Some(index) => index,
            None => {
                let index = self.output.content.len();
                let mut block = ThinkingBlock::new("");
                block.thinking_signature = Some(signature.to_owned());
                self.output.content.push(ContentBlock::Thinking(block));
                self.thinking_index = Some(index);
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ThinkingStart {
                        content_index: index,
                        partial: self.output.clone(),
                    }));
                index
            }
        };
        if let Some(ContentBlock::Thinking(block)) = self.output.content.get_mut(content_index) {
            block.thinking.push_str(delta);
        }
        self.pending
            .push_back(Ok(AssistantMessageEvent::ThinkingDelta {
                content_index,
                delta: delta.to_owned(),
                partial: self.output.clone(),
            }));
    }

    fn push_tool_call(
        &mut self,
        tool_call: &Value,
        position: usize,
    ) -> Result<(), ModelServiceError> {
        let id = tool_call
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let wire_index = tool_call
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let key = wire_index
            .or_else(|| self.tool_keys_by_id.get(id).copied())
            .unwrap_or_else(|| {
                let key = self.next_tool_key.max(position);
                self.next_tool_key = key.saturating_add(1);
                key
            });
        let function = tool_call.get("function").and_then(Value::as_object);
        let name = function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments_delta = function
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        if !self.tool_calls.contains_key(&key) {
            let content_index = self.output.content.len();
            self.output
                .content
                .push(ContentBlock::ToolCall(ToolCallBlock::new(
                    id,
                    name,
                    json!({}),
                )));
            self.tool_calls.insert(
                key,
                StreamingToolCall {
                    content_index,
                    id: id.to_owned(),
                    name: name.to_owned(),
                    partial_arguments: String::new(),
                },
            );
            self.pending
                .push_back(Ok(AssistantMessageEvent::ToolCallStart {
                    content_index,
                    partial: self.output.clone(),
                }));
        }

        let state = self.tool_calls.get_mut(&key).expect("inserted above");
        if state.id.is_empty() && !id.is_empty() {
            state.id = id.to_owned();
        }
        if state.name.is_empty() && !name.is_empty() {
            state.name = name.to_owned();
        }
        state.partial_arguments.push_str(arguments_delta);
        let content_index = state.content_index;
        let state_id = state.id.clone();
        let state_name = state.name.clone();
        let partial_arguments = state.partial_arguments.clone();
        if !state_id.is_empty() {
            self.tool_keys_by_id.insert(state_id.clone(), key);
        }
        if let Some(ContentBlock::ToolCall(block)) = self.output.content.get_mut(content_index) {
            block.id = state_id.clone();
            block.name = state_name;
            if let Ok(arguments) = serde_json::from_str::<Value>(&partial_arguments) {
                block.arguments = arguments;
            }
        }
        if let Some(signature) = self.pending_reasoning_details.remove(&state_id) {
            self.set_tool_signature(key, signature);
        }
        self.pending
            .push_back(Ok(AssistantMessageEvent::ToolCallDelta {
                content_index,
                delta: arguments_delta.to_owned(),
                partial: self.output.clone(),
            }));
        Ok(())
    }

    fn set_tool_signature(&mut self, key: usize, signature: String) {
        let Some(state) = self.tool_calls.get(&key) else {
            return;
        };
        if let Some(ContentBlock::ToolCall(block)) =
            self.output.content.get_mut(state.content_index)
        {
            block.thought_signature = Some(signature);
        }
    }

    fn finalize(&mut self) {
        let Some(reason) = self.finish_reason else {
            self.fail(ModelServiceError::protocol(
                "OpenAI Chat stream ended without finish_reason",
                true,
            ));
            return;
        };
        for state in self.tool_calls.values() {
            let arguments = match serde_json::from_str::<Value>(&state.partial_arguments) {
                Ok(Value::Object(arguments)) => Value::Object(arguments),
                _ => {
                    self.fail(ModelServiceError::protocol(
                        "OpenAI Chat tool-call arguments were malformed or truncated",
                        false,
                    ));
                    return;
                }
            };
            if let Some(ContentBlock::ToolCall(block)) =
                self.output.content.get_mut(state.content_index)
            {
                block.arguments = arguments;
            }
        }

        for (content_index, block) in self.output.content.clone().into_iter().enumerate() {
            let event = match block {
                ContentBlock::Text(block) => Some(AssistantMessageEvent::TextEnd {
                    content_index,
                    content: block.text,
                    partial: self.output.clone(),
                }),
                ContentBlock::Thinking(block) => Some(AssistantMessageEvent::ThinkingEnd {
                    content_index,
                    content: block.thinking,
                    partial: self.output.clone(),
                }),
                ContentBlock::ToolCall(block) => Some(AssistantMessageEvent::ToolCallEnd {
                    content_index,
                    tool_call: block,
                    partial: self.output.clone(),
                }),
                ContentBlock::Image(_) => None,
            };
            if let Some(event) = event {
                self.pending.push_back(Ok(event));
            }
        }
        self.output.stop_reason = completion_stop_reason(reason);
        self.pending.push_back(Ok(AssistantMessageEvent::Done {
            reason,
            message: self.output.clone(),
        }));
        self.terminated = true;
    }

    fn fail(&mut self, error: ModelServiceError) {
        if !self.terminated {
            self.pending.push_back(Err(error));
            self.terminated = true;
        }
    }
}

fn build_request(request: &ModelRequest) -> Result<Value, ModelServiceError> {
    let mut payload = Map::new();
    payload.insert("model".to_owned(), Value::String(request.model.id.clone()));
    payload.insert(
        "messages".to_owned(),
        Value::Array(convert_messages(request)?),
    );
    payload.insert("stream".to_owned(), Value::Bool(true));
    if compat_bool(&request.model, "supportsUsageInStreaming", true) {
        payload.insert(
            "stream_options".to_owned(),
            json!({ "include_usage": true }),
        );
    }
    if compat_bool(
        &request.model,
        "supportsStore",
        request.model.provider == "openai",
    ) {
        payload.insert("store".to_owned(), Value::Bool(false));
    }

    let max_tokens = request
        .options
        .max_tokens
        .unwrap_or(request.model.max_tokens);
    let max_tokens_field = compat_string(&request.model, "maxTokensField")
        .unwrap_or_else(|| "max_completion_tokens".to_owned());
    payload.insert(max_tokens_field, Value::from(max_tokens));
    if let Some(temperature) = request.options.temperature {
        payload.insert("temperature".to_owned(), Value::from(temperature));
    }
    if !request.tools.is_empty() {
        payload.insert(
            "tools".to_owned(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters,
                                "strict": false
                            }
                        })
                    })
                    .collect(),
            ),
        );
    } else if has_tool_history(&request.messages) {
        payload.insert("tools".to_owned(), Value::Array(Vec::new()));
    }
    if let Some(tool_choice) = &request.options.tool_choice {
        payload.insert("tool_choice".to_owned(), map_tool_choice(tool_choice));
    }
    if request.model.reasoning {
        if let Some(reasoning) = request.options.reasoning {
            payload.insert(
                "reasoning_effort".to_owned(),
                Value::String(map_thinking_level(&request.model, reasoning)),
            );
        } else if let Some(off) = request
            .model
            .thinking_level_map
            .as_ref()
            .and_then(|mapping| mapping.get("off"))
            .and_then(Clone::clone)
        {
            payload.insert("reasoning_effort".to_owned(), Value::String(off));
        }
    }

    if request.options.cache_retention != CacheRetention::None {
        if request.model.base_url.contains("api.openai.com")
            && let Some(session_id) = &request.options.session_id
        {
            payload.insert(
                "prompt_cache_key".to_owned(),
                Value::String(session_id.chars().take(64).collect()),
            );
        }
        if request.options.cache_retention == CacheRetention::Long
            && compat_bool(&request.model, "supportsLongCacheRetention", true)
        {
            payload.insert(
                "prompt_cache_retention".to_owned(),
                Value::String("24h".to_owned()),
            );
        }
    }
    Ok(Value::Object(payload))
}

fn convert_messages(request: &ModelRequest) -> Result<Vec<Value>, ModelServiceError> {
    let mut messages = Vec::new();
    if !request.system_prompt.is_empty() {
        let developer = request.model.reasoning
            && compat_bool(
                &request.model,
                "supportsDeveloperRole",
                request.model.provider == "openai",
            );
        messages.push(json!({
            "role": if developer { "developer" } else { "system" },
            "content": request.system_prompt
        }));
    }

    let mut index = 0;
    while index < request.messages.len() {
        match &request.messages[index] {
            Message::User(message) => {
                messages.push(json!({
                    "role": "user",
                    "content": convert_user_content(&message.content)?
                }));
            }
            Message::Assistant(message) => messages.push(convert_assistant_message(message)),
            Message::ToolResult(_) => {
                let mut image_parts = Vec::new();
                while index < request.messages.len() {
                    let Message::ToolResult(result) = &request.messages[index] else {
                        break;
                    };
                    let text = result
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text(block) => Some(block.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let has_images = result
                        .content
                        .iter()
                        .any(|block| matches!(block, ContentBlock::Image(_)));
                    let output = if !text.is_empty() {
                        text
                    } else if has_images {
                        "(see attached image)".to_owned()
                    } else {
                        "(no tool output)".to_owned()
                    };
                    messages.push(json!({
                        "role": "tool",
                        "content": output,
                        "tool_call_id": result.tool_call_id
                    }));
                    if request
                        .model
                        .input
                        .contains(&pi_protocol::ModelInput::Image)
                    {
                        for block in &result.content {
                            if let ContentBlock::Image(image) = block {
                                image_parts.push(json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": format!("data:{};base64,{}", image.mime_type, image.data)
                                    }
                                }));
                            }
                        }
                    }
                    index += 1;
                }
                if !image_parts.is_empty() {
                    let mut content = vec![json!({
                        "type": "text",
                        "text": "Attached image(s) from tool result:"
                    })];
                    content.extend(image_parts);
                    messages.push(json!({ "role": "user", "content": content }));
                }
                continue;
            }
        }
        index += 1;
    }
    Ok(messages)
}

fn convert_user_content(content: &MessageContent) -> Result<Value, ModelServiceError> {
    match content {
        MessageContent::Text(text) => Ok(Value::String(text.clone())),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text(block) => Ok(json!({ "type": "text", "text": block.text })),
                ContentBlock::Image(block) => Ok(json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{};base64,{}", block.mime_type, block.data) }
                })),
                ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) => Err(ModelServiceError::new(
                    ModelServiceErrorCategory::InvalidRequest,
                    "user messages can contain only text and image blocks",
                    false,
                )),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
    }
}

fn convert_assistant_message(message: &AssistantMessage) -> Value {
    let text = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(block) if !block.text.is_empty() => Some(block.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    let mut converted = Map::new();
    converted.insert("role".to_owned(), Value::String("assistant".to_owned()));
    converted.insert(
        "content".to_owned(),
        if text.is_empty() {
            Value::Null
        } else {
            Value::String(text)
        },
    );
    let tool_calls: Vec<_> = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolCall(block) => Some(json!({
                "id": block.id,
                "type": "function",
                "function": { "name": block.name, "arguments": block.arguments.to_string() }
            })),
            _ => None,
        })
        .collect();
    if !tool_calls.is_empty() {
        converted.insert("tool_calls".to_owned(), Value::Array(tool_calls));
    }
    for block in &message.content {
        if let ContentBlock::Thinking(block) = block
            && let Some(signature) = &block.thinking_signature
        {
            converted.insert(signature.clone(), Value::String(block.thinking.clone()));
            break;
        }
    }
    let reasoning_details: Vec<_> = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolCall(block) => block
                .thought_signature
                .as_ref()
                .and_then(|signature| serde_json::from_str::<Value>(signature).ok()),
            _ => None,
        })
        .collect();
    if !reasoning_details.is_empty() {
        converted.insert(
            "reasoning_details".to_owned(),
            Value::Array(reasoning_details),
        );
    }
    Value::Object(converted)
}

fn has_tool_history(messages: &[Message]) -> bool {
    messages.iter().any(|message| match message {
        Message::ToolResult(_) => true,
        Message::Assistant(message) => message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolCall(_))),
        Message::User(_) => false,
    })
}

fn map_tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => Value::String("auto".to_owned()),
        ToolChoice::None => Value::String("none".to_owned()),
        ToolChoice::Required => Value::String("required".to_owned()),
        ToolChoice::Named(name) => json!({ "type": "function", "function": { "name": name } }),
    }
}

fn map_thinking_level(model: &pi_protocol::Model, level: ThinkingLevel) -> String {
    let name = match level {
        ThinkingLevel::Minimal => "minimal",
        ThinkingLevel::Low => "low",
        ThinkingLevel::Medium => "medium",
        ThinkingLevel::High => "high",
        ThinkingLevel::XHigh => "xhigh",
        ThinkingLevel::Max => "max",
    };
    model
        .thinking_level_map
        .as_ref()
        .and_then(|mapping| mapping.get(name))
        .and_then(Clone::clone)
        .unwrap_or_else(|| name.to_owned())
}

fn compat_bool(model: &pi_protocol::Model, field: &str, default: bool) -> bool {
    model
        .compat
        .as_ref()
        .and_then(|compat| compat.get(field))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn compat_string(model: &pi_protocol::Model, field: &str) -> Option<String> {
    model
        .compat
        .as_ref()
        .and_then(|compat| compat.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn map_finish_reason(reason: &str) -> Result<CompletionReason, ModelServiceError> {
    match reason {
        "stop" | "end" => Ok(CompletionReason::Stop),
        "length" => Ok(CompletionReason::Length),
        "function_call" | "tool_calls" => Ok(CompletionReason::ToolUse),
        "network_error" => Err(ModelServiceError::new(
            ModelServiceErrorCategory::Network,
            "provider finish reason: network_error",
            true,
        )),
        "content_filter" => Err(ModelServiceError::new(
            ModelServiceErrorCategory::InvalidRequest,
            "provider finish reason: content_filter",
            false,
        )),
        _ => Err(ModelServiceError::protocol(
            format!("unrecognized OpenAI Chat finish reason: {reason}"),
            false,
        )),
    }
}

const fn completion_stop_reason(reason: CompletionReason) -> StopReason {
    match reason {
        CompletionReason::Stop => StopReason::Stop,
        CompletionReason::Length => StopReason::Length,
        CompletionReason::ToolUse => StopReason::ToolUse,
    }
}

fn parse_usage(raw: &Value, model: &pi_protocol::Model) -> Usage {
    let prompt = raw
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = raw
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let details = raw.get("prompt_tokens_details");
    let cache_read = details
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .or_else(|| raw.get("prompt_cache_hit_tokens").and_then(Value::as_u64))
        .unwrap_or(0);
    let cache_write = details
        .and_then(|details| details.get("cache_write_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let input = prompt
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    let reasoning = raw
        .get("completion_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64);
    let mut usage = Usage {
        input,
        output,
        cache_read,
        cache_write,
        cache_write_1h: None,
        reasoning,
        total_tokens: input + output + cache_read + cache_write,
        cost: pi_protocol::UsageCost::default(),
    };
    calculate_cost(model, &mut usage);
    usage
}

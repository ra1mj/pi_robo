use crate::common::{calculate_cost, non_empty_string, provider_event_error, read_json_http_error};
use crate::{
    ProviderAdapterConfig, ProviderClock, ProviderHttpClient, ProviderHttpResponse, SseDecoder,
    SseEvent, SystemProviderClock,
};
use futures_util::stream;
use pi_model::{
    CacheRetention, Cancellation, ModelEventStream, ModelFuture, ModelRequest, ModelService,
    ModelServiceError, ModelServiceErrorCategory, ThinkingBudgets, ThinkingLevel, ToolChoice,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, CompletionReason, ContentBlock, Extensions, Message,
    MessageContent, StopReason, TextBlock, ThinkingBlock, ToolCallBlock, Usage,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

const MAX_SSE_EVENT_BYTES: usize = 1_048_576;
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Direct Anthropic Messages protocol adapter.
#[derive(Clone)]
pub struct AnthropicMessagesAdapter {
    http: ProviderHttpClient,
    clock: Arc<dyn ProviderClock>,
}

impl AnthropicMessagesAdapter {
    pub fn new(config: &ProviderAdapterConfig) -> Result<Self, ModelServiceError> {
        let config = config
            .clone()
            .with_default_header("anthropic-version", ANTHROPIC_VERSION);
        Ok(Self {
            http: ProviderHttpClient::new(&config)?,
            clock: Arc::new(SystemProviderClock),
        })
    }

    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn ProviderClock>) -> Self {
        self.clock = clock;
        self
    }
}

impl ModelService for AnthropicMessagesAdapter {
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
                .post_json("messages", &payload, cancellation)
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
            let state = AnthropicStreamState::new(
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

enum AnthropicSlot {
    Text {
        content_index: usize,
    },
    Thinking {
        content_index: usize,
    },
    ToolCall {
        content_index: usize,
        partial_arguments: String,
        initial_arguments: Value,
    },
}

struct AnthropicStreamState<'a> {
    response: ProviderHttpResponse,
    cancellation: &'a dyn Cancellation,
    http: ProviderHttpClient,
    model: pi_protocol::Model,
    decoder: SseDecoder,
    output: AssistantMessage,
    pending: VecDeque<Result<AssistantMessageEvent, ModelServiceError>>,
    slots: BTreeMap<usize, AnthropicSlot>,
    completion_reason: Option<CompletionReason>,
    terminated: bool,
}

impl<'a> AnthropicStreamState<'a> {
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
            slots: BTreeMap::new(),
            completion_reason: None,
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
                        self.fail(ModelServiceError::protocol(
                            "Anthropic stream ended before message_stop",
                            true,
                        ));
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
            if event.data.trim().is_empty() {
                continue;
            }
            let payload: Value = match serde_json::from_str(event.data.trim()) {
                Ok(payload) => payload,
                Err(_) => {
                    self.fail(ModelServiceError::protocol(
                        "Anthropic stream contained malformed JSON",
                        false,
                    ));
                    continue;
                }
            };
            if let Some(sse_type) = event.event.as_deref()
                && payload.get("type").and_then(Value::as_str) != Some(sse_type)
            {
                self.fail(ModelServiceError::protocol(
                    "Anthropic SSE event name did not match its payload",
                    false,
                ));
                continue;
            }
            if let Err(error) = self.process_event(&payload) {
                self.fail(error);
            }
        }
    }

    fn process_event(&mut self, event: &Value) -> Result<(), ModelServiceError> {
        let event_type = event.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelServiceError::protocol("Anthropic event omitted its type", false)
        })?;
        match event_type {
            "message_start" => {
                let message = event.get("message").ok_or_else(|| {
                    ModelServiceError::protocol("message_start omitted its message", false)
                })?;
                self.output.response_id = non_empty_string(message.get("id"));
                if let Some(response_model) = non_empty_string(message.get("model"))
                    && response_model != self.model.id
                {
                    self.output.response_model = Some(response_model);
                }
                if let Some(usage) = message.get("usage") {
                    update_usage(&mut self.output.usage, usage);
                    calculate_cost(&self.model, &mut self.output.usage);
                }
            }
            "content_block_start" => self.start_block(event)?,
            "content_block_delta" => self.apply_delta(event)?,
            "content_block_stop" => {
                let index = event_index(event)?;
                self.finish_block(index)?;
            }
            "message_delta" => {
                if let Some(reason) = event
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    let reason = map_stop_reason(reason)?;
                    self.output.stop_reason = completion_stop_reason(reason);
                    self.completion_reason = Some(reason);
                }
                if let Some(usage) = event.get("usage") {
                    update_usage(&mut self.output.usage, usage);
                    calculate_cost(&self.model, &mut self.output.usage);
                }
            }
            "message_stop" => self.finish_message()?,
            "error" => {
                return Err(provider_event_error(
                    &self.http,
                    event.get("error").unwrap_or(event),
                    "Anthropic stream error",
                ));
            }
            "ping" => {}
            _ => {
                return Err(ModelServiceError::protocol(
                    format!("unsupported Anthropic event type: {event_type}"),
                    false,
                ));
            }
        }
        Ok(())
    }

    fn start_block(&mut self, event: &Value) -> Result<(), ModelServiceError> {
        let index = event_index(event)?;
        if self.slots.contains_key(&index) {
            return Err(ModelServiceError::protocol(
                "Anthropic reused a content block index",
                false,
            ));
        }
        let block = event.get("content_block").ok_or_else(|| {
            ModelServiceError::protocol("content_block_start omitted content_block", false)
        })?;
        let block_type = block.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelServiceError::protocol("Anthropic content block omitted its type", false)
        })?;
        let content_index = self.output.content.len();
        let slot = match block_type {
            "text" => {
                let text = block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.output
                    .content
                    .push(ContentBlock::Text(TextBlock::new(text)));
                self.pending.push_back(Ok(AssistantMessageEvent::TextStart {
                    content_index,
                    partial: self.output.clone(),
                }));
                AnthropicSlot::Text { content_index }
            }
            "thinking" => {
                let mut thinking = ThinkingBlock::new(
                    block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                );
                thinking.thinking_signature = non_empty_string(block.get("signature"));
                self.output.content.push(ContentBlock::Thinking(thinking));
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ThinkingStart {
                        content_index,
                        partial: self.output.clone(),
                    }));
                AnthropicSlot::Thinking { content_index }
            }
            "redacted_thinking" => {
                let mut thinking = ThinkingBlock::new("[Reasoning redacted]");
                thinking.thinking_signature = non_empty_string(block.get("data"));
                thinking.redacted = Some(true);
                self.output.content.push(ContentBlock::Thinking(thinking));
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ThinkingStart {
                        content_index,
                        partial: self.output.clone(),
                    }));
                AnthropicSlot::Thinking { content_index }
            }
            "tool_use" => {
                let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let arguments = block.get("input").cloned().unwrap_or_else(|| json!({}));
                self.output
                    .content
                    .push(ContentBlock::ToolCall(ToolCallBlock::new(
                        id,
                        name,
                        arguments.clone(),
                    )));
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ToolCallStart {
                        content_index,
                        partial: self.output.clone(),
                    }));
                AnthropicSlot::ToolCall {
                    content_index,
                    partial_arguments: String::new(),
                    initial_arguments: arguments,
                }
            }
            _ => {
                return Err(ModelServiceError::protocol(
                    format!("unsupported Anthropic content block: {block_type}"),
                    false,
                ));
            }
        };
        self.slots.insert(index, slot);
        Ok(())
    }

    fn apply_delta(&mut self, event: &Value) -> Result<(), ModelServiceError> {
        let index = event_index(event)?;
        let delta = event.get("delta").ok_or_else(|| {
            ModelServiceError::protocol("content_block_delta omitted delta", false)
        })?;
        let delta_type = delta.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelServiceError::protocol("Anthropic content delta omitted its type", false)
        })?;
        match delta_type {
            "text_delta" => {
                let text = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ModelServiceError::protocol("text_delta omitted text", false))?;
                let Some(AnthropicSlot::Text { content_index }) = self.slots.get(&index) else {
                    return Err(block_mismatch("text", index));
                };
                let content_index = *content_index;
                if let Some(ContentBlock::Text(block)) = self.output.content.get_mut(content_index)
                {
                    block.text.push_str(text);
                }
                self.pending.push_back(Ok(AssistantMessageEvent::TextDelta {
                    content_index,
                    delta: text.to_owned(),
                    partial: self.output.clone(),
                }));
            }
            "thinking_delta" => {
                let thinking = delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ModelServiceError::protocol("thinking_delta omitted thinking", false)
                    })?;
                let Some(AnthropicSlot::Thinking { content_index }) = self.slots.get(&index) else {
                    return Err(block_mismatch("thinking", index));
                };
                let content_index = *content_index;
                if let Some(ContentBlock::Thinking(block)) =
                    self.output.content.get_mut(content_index)
                {
                    block.thinking.push_str(thinking);
                }
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ThinkingDelta {
                        content_index,
                        delta: thinking.to_owned(),
                        partial: self.output.clone(),
                    }));
            }
            "signature_delta" => {
                let signature =
                    delta
                        .get("signature")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ModelServiceError::protocol("signature_delta omitted signature", false)
                        })?;
                let Some(AnthropicSlot::Thinking { content_index }) = self.slots.get(&index) else {
                    return Err(block_mismatch("thinking", index));
                };
                if let Some(ContentBlock::Thinking(block)) =
                    self.output.content.get_mut(*content_index)
                {
                    block
                        .thinking_signature
                        .get_or_insert_with(String::new)
                        .push_str(signature);
                }
            }
            "input_json_delta" => {
                let partial = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ModelServiceError::protocol("input_json_delta omitted partial_json", false)
                    })?;
                let Some(AnthropicSlot::ToolCall {
                    content_index,
                    partial_arguments,
                    ..
                }) = self.slots.get_mut(&index)
                else {
                    return Err(block_mismatch("tool", index));
                };
                partial_arguments.push_str(partial);
                let content_index = *content_index;
                if let Ok(arguments) = serde_json::from_str::<Value>(partial_arguments)
                    && let Some(ContentBlock::ToolCall(block)) =
                        self.output.content.get_mut(content_index)
                {
                    block.arguments = arguments;
                }
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ToolCallDelta {
                        content_index,
                        delta: partial.to_owned(),
                        partial: self.output.clone(),
                    }));
            }
            "citations_delta" => {}
            _ => {
                return Err(ModelServiceError::protocol(
                    format!("unsupported Anthropic content delta: {delta_type}"),
                    false,
                ));
            }
        }
        Ok(())
    }

    fn finish_block(&mut self, index: usize) -> Result<(), ModelServiceError> {
        let slot = self
            .slots
            .remove(&index)
            .ok_or_else(|| block_mismatch("content", index))?;
        match slot {
            AnthropicSlot::Text { content_index } => {
                let Some(ContentBlock::Text(block)) = self.output.content.get(content_index) else {
                    return Err(block_mismatch("text", index));
                };
                self.pending.push_back(Ok(AssistantMessageEvent::TextEnd {
                    content_index,
                    content: block.text.clone(),
                    partial: self.output.clone(),
                }));
            }
            AnthropicSlot::Thinking { content_index } => {
                let Some(ContentBlock::Thinking(block)) = self.output.content.get(content_index)
                else {
                    return Err(block_mismatch("thinking", index));
                };
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ThinkingEnd {
                        content_index,
                        content: block.thinking.clone(),
                        partial: self.output.clone(),
                    }));
            }
            AnthropicSlot::ToolCall {
                content_index,
                partial_arguments,
                initial_arguments,
            } => {
                let arguments = if partial_arguments.is_empty() {
                    initial_arguments
                } else {
                    match serde_json::from_str::<Value>(&partial_arguments) {
                        Ok(Value::Object(arguments)) => Value::Object(arguments),
                        _ => {
                            return Err(ModelServiceError::protocol(
                                "Anthropic tool input was malformed or truncated",
                                false,
                            ));
                        }
                    }
                };
                let tool_call = if let Some(ContentBlock::ToolCall(block)) =
                    self.output.content.get_mut(content_index)
                {
                    if block.id.is_empty() || block.name.is_empty() {
                        return Err(ModelServiceError::protocol(
                            "Anthropic tool use omitted its identifier or name",
                            false,
                        ));
                    }
                    block.arguments = arguments;
                    block.clone()
                } else {
                    return Err(block_mismatch("tool", index));
                };
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ToolCallEnd {
                        content_index,
                        tool_call,
                        partial: self.output.clone(),
                    }));
            }
        }
        Ok(())
    }

    fn finish_message(&mut self) -> Result<(), ModelServiceError> {
        if !self.slots.is_empty() {
            return Err(ModelServiceError::protocol(
                "Anthropic message_stop left content blocks unfinished",
                false,
            ));
        }
        let reason = self.completion_reason.ok_or_else(|| {
            ModelServiceError::protocol("Anthropic message_stop omitted a stop reason", false)
        })?;
        self.pending.push_back(Ok(AssistantMessageEvent::Done {
            reason,
            message: self.output.clone(),
        }));
        self.terminated = true;
        Ok(())
    }

    fn fail(&mut self, error: ModelServiceError) {
        if !self.terminated {
            self.pending.push_back(Err(error));
            self.terminated = true;
        }
    }
}

fn build_request(request: &ModelRequest) -> Result<Value, ModelServiceError> {
    let cache_control = cache_control(request.options.cache_retention);
    let mut payload = Map::from_iter([
        ("model".to_owned(), Value::String(request.model.id.clone())),
        (
            "messages".to_owned(),
            Value::Array(convert_messages(request, cache_control.as_ref())?),
        ),
        (
            "max_tokens".to_owned(),
            Value::from(
                request
                    .options
                    .max_tokens
                    .unwrap_or(request.model.max_tokens),
            ),
        ),
        ("stream".to_owned(), Value::Bool(true)),
    ]);
    if !request.system_prompt.is_empty() {
        let mut system = json!({ "type": "text", "text": request.system_prompt });
        if let Some(cache_control) = &cache_control
            && let Some(system) = system.as_object_mut()
        {
            system.insert("cache_control".to_owned(), cache_control.clone());
        }
        payload.insert("system".to_owned(), Value::Array(vec![system]));
    }
    if request.options.reasoning.is_none()
        && let Some(temperature) = request.options.temperature
        && compat_bool(&request.model, "supportsTemperature", true)
    {
        payload.insert("temperature".to_owned(), Value::from(temperature));
    }
    if !request.tools.is_empty() {
        let supports_eager = compat_bool(&request.model, "supportsEagerToolInputStreaming", false);
        let supports_cache = compat_bool(&request.model, "supportsCacheControlOnTools", true);
        let last_index = request.tools.len().saturating_sub(1);
        let tools = request
            .tools
            .iter()
            .enumerate()
            .map(|(index, tool)| {
                let schema = tool.parameters.as_object();
                let mut converted = Map::from_iter([
                    ("name".to_owned(), Value::String(tool.name.clone())),
                    (
                        "description".to_owned(),
                        Value::String(tool.description.clone()),
                    ),
                    (
                        "input_schema".to_owned(),
                        json!({
                            "type": "object",
                            "properties": schema
                                .and_then(|schema| schema.get("properties"))
                                .cloned()
                                .unwrap_or_else(|| json!({})),
                            "required": schema
                                .and_then(|schema| schema.get("required"))
                                .cloned()
                                .unwrap_or_else(|| json!([]))
                        }),
                    ),
                ]);
                if supports_eager {
                    converted.insert("eager_input_streaming".to_owned(), Value::Bool(true));
                }
                if supports_cache
                    && index == last_index
                    && let Some(cache_control) = &cache_control
                {
                    converted.insert("cache_control".to_owned(), cache_control.clone());
                }
                Value::Object(converted)
            })
            .collect();
        payload.insert("tools".to_owned(), Value::Array(tools));
    }
    if let Some(choice) = &request.options.tool_choice {
        payload.insert("tool_choice".to_owned(), map_tool_choice(choice));
    }
    if request.model.reasoning {
        if let Some(level) = request.options.reasoning {
            if compat_bool(&request.model, "forceAdaptiveThinking", false) {
                payload.insert(
                    "thinking".to_owned(),
                    json!({ "type": "adaptive", "display": "summarized" }),
                );
                payload.insert(
                    "output_config".to_owned(),
                    json!({ "effort": map_thinking_level(&request.model, level) }),
                );
            } else {
                payload.insert(
                    "thinking".to_owned(),
                    json!({
                        "type": "enabled",
                        "budget_tokens": thinking_budget(level, request.options.thinking_budgets),
                        "display": "summarized"
                    }),
                );
            }
        } else if request
            .model
            .thinking_level_map
            .as_ref()
            .is_none_or(|mapping| mapping.get("off") != Some(&None))
        {
            payload.insert("thinking".to_owned(), json!({ "type": "disabled" }));
        }
    }
    Ok(Value::Object(payload))
}

fn convert_messages(
    request: &ModelRequest,
    cache_control: Option<&Value>,
) -> Result<Vec<Value>, ModelServiceError> {
    let mut messages = Vec::new();
    let allow_empty_signature = compat_bool(&request.model, "allowEmptySignature", false);
    let mut index = 0;
    while index < request.messages.len() {
        match &request.messages[index] {
            Message::User(message) => match &message.content {
                MessageContent::Text(text) if !text.trim().is_empty() => {
                    messages.push(json!({ "role": "user", "content": text }));
                }
                MessageContent::Text(_) => {}
                MessageContent::Blocks(blocks) => {
                    let content = convert_content_blocks(blocks)?;
                    if !content.is_empty() {
                        messages.push(json!({ "role": "user", "content": content }));
                    }
                }
            },
            Message::Assistant(message) => {
                let mut content = Vec::new();
                for block in &message.content {
                    match block {
                        ContentBlock::Text(block) if !block.text.trim().is_empty() => {
                            content.push(json!({ "type": "text", "text": block.text }));
                        }
                        ContentBlock::Text(_) => {}
                        ContentBlock::Thinking(block) if block.redacted == Some(true) => {
                            let signature =
                                block.thinking_signature.as_deref().ok_or_else(|| {
                                    ModelServiceError::new(
                                        ModelServiceErrorCategory::InvalidRequest,
                                        "redacted thinking block omitted its signature",
                                        false,
                                    )
                                })?;
                            content.push(json!({
                                "type": "redacted_thinking",
                                "data": signature
                            }));
                        }
                        ContentBlock::Thinking(block) => {
                            let signature = block
                                .thinking_signature
                                .as_deref()
                                .filter(|signature| !signature.trim().is_empty());
                            if block.thinking.trim().is_empty() && signature.is_none() {
                                continue;
                            }
                            if let Some(signature) = signature {
                                content.push(json!({
                                    "type": "thinking",
                                    "thinking": block.thinking,
                                    "signature": signature
                                }));
                            } else if allow_empty_signature {
                                content.push(json!({
                                    "type": "thinking",
                                    "thinking": block.thinking,
                                    "signature": ""
                                }));
                            } else {
                                content.push(json!({ "type": "text", "text": block.thinking }));
                            }
                        }
                        ContentBlock::ToolCall(block) => content.push(json!({
                            "type": "tool_use",
                            "id": normalize_tool_call_id(&block.id),
                            "name": block.name,
                            "input": block.arguments
                        })),
                        ContentBlock::Image(_) => {
                            return Err(ModelServiceError::new(
                                ModelServiceErrorCategory::InvalidRequest,
                                "assistant messages cannot contain image blocks",
                                false,
                            ));
                        }
                    }
                }
                if !content.is_empty() {
                    messages.push(json!({ "role": "assistant", "content": content }));
                }
            }
            Message::ToolResult(_) => {
                let mut content = Vec::new();
                while index < request.messages.len() {
                    let Message::ToolResult(result) = &request.messages[index] else {
                        break;
                    };
                    let converted = convert_content_blocks(&result.content)?;
                    content.push(json!({
                        "type": "tool_result",
                        "tool_use_id": normalize_tool_call_id(&result.tool_call_id),
                        "content": converted,
                        "is_error": result.is_error
                    }));
                    index += 1;
                }
                if !content.is_empty() {
                    messages.push(json!({ "role": "user", "content": content }));
                }
                continue;
            }
        }
        index += 1;
    }
    apply_message_cache_control(&mut messages, cache_control);
    Ok(messages)
}

fn convert_content_blocks(blocks: &[ContentBlock]) -> Result<Vec<Value>, ModelServiceError> {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(block) if !block.text.trim().is_empty() => {
                Some(Ok(json!({ "type": "text", "text": block.text })))
            }
            ContentBlock::Text(_) => None,
            ContentBlock::Image(block) => Some(Ok(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": block.mime_type,
                    "data": block.data
                }
            }))),
            ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) => {
                Some(Err(ModelServiceError::new(
                    ModelServiceErrorCategory::InvalidRequest,
                    "user and tool-result content can contain only text and images",
                    false,
                )))
            }
        })
        .collect()
}

fn apply_message_cache_control(messages: &mut [Value], cache_control: Option<&Value>) {
    let Some(cache_control) = cache_control else {
        return;
    };
    let Some(last_user) = messages
        .iter_mut()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    else {
        return;
    };
    if let Some(text) = last_user
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
    {
        last_user["content"] = json!([{
            "type": "text",
            "text": text,
            "cache_control": cache_control
        }]);
        return;
    }
    if let Some(parts) = last_user.get_mut("content").and_then(Value::as_array_mut)
        && let Some(last) = parts.last_mut().and_then(Value::as_object_mut)
    {
        last.insert("cache_control".to_owned(), cache_control.clone());
    }
}

fn update_usage(usage: &mut Usage, raw: &Value) {
    if let Some(input) = raw.get("input_tokens").and_then(Value::as_u64) {
        usage.input = input;
    }
    if let Some(output) = raw.get("output_tokens").and_then(Value::as_u64) {
        usage.output = output;
    }
    if let Some(cache_read) = raw.get("cache_read_input_tokens").and_then(Value::as_u64) {
        usage.cache_read = cache_read;
    }
    if let Some(cache_write) = raw
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
    {
        usage.cache_write = cache_write;
    }
    if let Some(cache_write_1h) = raw
        .get("cache_creation")
        .and_then(|value| value.get("ephemeral_1h_input_tokens"))
        .and_then(Value::as_u64)
    {
        usage.cache_write_1h = Some(cache_write_1h);
    }
    if let Some(reasoning) = raw
        .get("output_tokens_details")
        .and_then(|value| value.get("thinking_tokens"))
        .and_then(Value::as_u64)
    {
        usage.reasoning = Some(reasoning);
    }
    usage.total_tokens = usage.input + usage.output + usage.cache_read + usage.cache_write;
}

fn cache_control(retention: CacheRetention) -> Option<Value> {
    match retention {
        CacheRetention::None => None,
        CacheRetention::Short => Some(json!({ "type": "ephemeral" })),
        CacheRetention::Long => Some(json!({ "type": "ephemeral", "ttl": "1h" })),
    }
}

fn map_tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!({ "type": "auto" }),
        ToolChoice::None => json!({ "type": "none" }),
        ToolChoice::Required => json!({ "type": "any" }),
        ToolChoice::Named(name) => json!({ "type": "tool", "name": name }),
    }
}

fn thinking_budget(level: ThinkingLevel, budgets: Option<ThinkingBudgets>) -> u64 {
    let configured = budgets.and_then(|budgets| match level {
        ThinkingLevel::Minimal => budgets.minimal,
        ThinkingLevel::Low => budgets.low,
        ThinkingLevel::Medium => budgets.medium,
        ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => budgets.high,
    });
    configured.unwrap_or(1_024)
}

fn map_thinking_level(model: &pi_protocol::Model, level: ThinkingLevel) -> String {
    let name = match level {
        ThinkingLevel::Minimal => "low",
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

fn map_stop_reason(reason: &str) -> Result<CompletionReason, ModelServiceError> {
    match reason {
        "end_turn" | "stop_sequence" | "pause_turn" => Ok(CompletionReason::Stop),
        "max_tokens" | "model_context_window_exceeded" => Ok(CompletionReason::Length),
        "tool_use" => Ok(CompletionReason::ToolUse),
        "refusal" => Err(ModelServiceError::new(
            ModelServiceErrorCategory::InvalidRequest,
            "Anthropic response was refused",
            false,
        )),
        _ => Err(ModelServiceError::protocol(
            format!("unsupported Anthropic stop reason: {reason}"),
            false,
        )),
    }
}

fn normalize_tool_call_id(id: &str) -> String {
    id.chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(64)
        .collect()
}

fn compat_bool(model: &pi_protocol::Model, field: &str, default: bool) -> bool {
    model
        .compat
        .as_ref()
        .and_then(|compat| compat.get(field))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn event_index(event: &Value) -> Result<usize, ModelServiceError> {
    event
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            ModelServiceError::protocol("Anthropic content event omitted its index", false)
        })
}

fn block_mismatch(expected: &str, index: usize) -> ModelServiceError {
    ModelServiceError::protocol(
        format!("Anthropic {expected} event has no block at index {index}"),
        false,
    )
}

const fn completion_stop_reason(reason: CompletionReason) -> StopReason {
    match reason {
        CompletionReason::Stop => StopReason::Stop,
        CompletionReason::Length => StopReason::Length,
        CompletionReason::ToolUse => StopReason::ToolUse,
    }
}

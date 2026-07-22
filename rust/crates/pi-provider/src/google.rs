use crate::common::{calculate_cost, non_empty_string, provider_event_error, read_json_http_error};
use crate::{
    ProviderAdapterConfig, ProviderClock, ProviderHttpClient, ProviderHttpResponse, SseDecoder,
    SseEvent, SystemProviderClock,
};
use base64::Engine;
use futures_util::stream;
use pi_model::{
    Cancellation, ModelEventStream, ModelFuture, ModelRequest, ModelService, ModelServiceError,
    ModelServiceErrorCategory, ThinkingBudgets, ThinkingLevel, ToolChoice,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, CompletionReason, ContentBlock, Extensions, Message,
    MessageContent, StopReason, TextBlock, ThinkingBlock, ToolCallBlock, Usage,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

const MAX_SSE_EVENT_BYTES: usize = 1_048_576;

/// Direct Google Generative Language protocol adapter.
#[derive(Clone)]
pub struct GoogleGenerativeLanguageAdapter {
    http: ProviderHttpClient,
    clock: Arc<dyn ProviderClock>,
}

impl GoogleGenerativeLanguageAdapter {
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

impl ModelService for GoogleGenerativeLanguageAdapter {
    fn stream<'a>(
        &'a self,
        request: ModelRequest,
        cancellation: &'a dyn Cancellation,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            request.options.validate()?;
            let payload = build_request(&request)?;
            let path = format!("models/{}:streamGenerateContent?alt=sse", request.model.id);
            let response = self.http.post_json(&path, &payload, cancellation).await?;
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
            let state = GoogleStreamState::new(
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum OpenTextKind {
    Text,
    Thinking,
}

struct GoogleStreamState<'a> {
    response: ProviderHttpResponse,
    cancellation: &'a dyn Cancellation,
    http: ProviderHttpClient,
    model: pi_protocol::Model,
    decoder: SseDecoder,
    output: AssistantMessage,
    pending: VecDeque<Result<AssistantMessageEvent, ModelServiceError>>,
    open_text: Option<(OpenTextKind, usize)>,
    completion_reason: Option<CompletionReason>,
    tool_call_ids: BTreeSet<String>,
    generated_tool_calls: u64,
    terminated: bool,
}

impl<'a> GoogleStreamState<'a> {
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
            open_text: None,
            completion_reason: None,
            tool_call_ids: BTreeSet::new(),
            generated_tool_calls: 0,
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
                        self.finalize_or_fail();
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
                self.finalize_or_fail();
                continue;
            }
            let chunk: Value = match serde_json::from_str(data) {
                Ok(chunk) => chunk,
                Err(_) => {
                    self.fail(ModelServiceError::protocol(
                        "Google stream contained malformed JSON",
                        false,
                    ));
                    continue;
                }
            };
            if let Some(error) = chunk.get("error") {
                self.fail(provider_event_error(
                    &self.http,
                    error,
                    "Google stream error",
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
            self.output.response_id = non_empty_string(chunk.get("responseId"));
        }
        if let Some(response_model) = non_empty_string(chunk.get("modelVersion"))
            && response_model != self.model.id
        {
            self.output.response_model = Some(response_model);
        }
        if let Some(usage) = chunk.get("usageMetadata") {
            self.output.usage = parse_usage(usage, &self.model);
        }
        if chunk.get("promptFeedback").is_some_and(is_blocked_prompt) {
            return Err(ModelServiceError::new(
                ModelServiceErrorCategory::InvalidRequest,
                "Google blocked the prompt for safety",
                false,
            ));
        }
        let Some(candidate) = chunk
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
        else {
            return Ok(());
        };
        if let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                self.process_part(part)?;
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            self.completion_reason = Some(map_finish_reason(reason)?);
        }
        Ok(())
    }

    fn process_part(&mut self, part: &Value) -> Result<(), ModelServiceError> {
        let has_text = part.get("text").is_some();
        let has_call = part.get("functionCall").is_some();
        if has_text && has_call {
            return Err(ModelServiceError::protocol(
                "Google response part mixed text and a function call",
                false,
            ));
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            let kind = if part.get("thought").and_then(Value::as_bool) == Some(true) {
                OpenTextKind::Thinking
            } else {
                OpenTextKind::Text
            };
            self.push_text(kind, text, part.get("thoughtSignature"))?;
            return Ok(());
        }
        if let Some(call) = part.get("functionCall") {
            self.finish_open_text();
            self.push_tool_call(call, part.get("thoughtSignature"))?;
            return Ok(());
        }
        if part.get("inlineData").is_some()
            || part.get("fileData").is_some()
            || part.get("functionResponse").is_some()
        {
            return Err(ModelServiceError::protocol(
                "unsupported Google response content part",
                false,
            ));
        }
        Ok(())
    }

    fn push_text(
        &mut self,
        kind: OpenTextKind,
        delta: &str,
        signature: Option<&Value>,
    ) -> Result<(), ModelServiceError> {
        if self.open_text.is_some_and(|(open, _)| open != kind) {
            self.finish_open_text();
        }
        let content_index = match self.open_text {
            Some((_, content_index)) => content_index,
            None => {
                let content_index = self.output.content.len();
                match kind {
                    OpenTextKind::Text => {
                        self.output
                            .content
                            .push(ContentBlock::Text(TextBlock::new("")));
                        self.pending.push_back(Ok(AssistantMessageEvent::TextStart {
                            content_index,
                            partial: self.output.clone(),
                        }));
                    }
                    OpenTextKind::Thinking => {
                        self.output
                            .content
                            .push(ContentBlock::Thinking(ThinkingBlock::new("")));
                        self.pending
                            .push_back(Ok(AssistantMessageEvent::ThinkingStart {
                                content_index,
                                partial: self.output.clone(),
                            }));
                    }
                }
                self.open_text = Some((kind, content_index));
                content_index
            }
        };
        let signature = valid_signature(signature)?;
        match self.output.content.get_mut(content_index) {
            Some(ContentBlock::Text(block)) if kind == OpenTextKind::Text => {
                block.text.push_str(delta);
                if signature.is_some() {
                    block.text_signature = signature;
                }
                self.pending.push_back(Ok(AssistantMessageEvent::TextDelta {
                    content_index,
                    delta: delta.to_owned(),
                    partial: self.output.clone(),
                }));
            }
            Some(ContentBlock::Thinking(block)) if kind == OpenTextKind::Thinking => {
                block.thinking.push_str(delta);
                if signature.is_some() {
                    block.thinking_signature = signature;
                }
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ThinkingDelta {
                        content_index,
                        delta: delta.to_owned(),
                        partial: self.output.clone(),
                    }));
            }
            _ => {
                return Err(ModelServiceError::protocol(
                    "Google text block state diverged",
                    false,
                ));
            }
        }
        Ok(())
    }

    fn push_tool_call(
        &mut self,
        call: &Value,
        signature: Option<&Value>,
    ) -> Result<(), ModelServiceError> {
        let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
        if name.is_empty() {
            return Err(ModelServiceError::protocol(
                "Google function call omitted its name",
                false,
            ));
        }
        let signature = valid_signature(signature)?;
        if compat_bool(
            &self.model,
            "requiresThoughtSignature",
            self.model.reasoning,
        ) && signature.is_none()
        {
            return Err(ModelServiceError::protocol(
                "Google reasoning tool call omitted a valid thought signature",
                false,
            ));
        }
        let provided_id = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let id = if let Some(id) = provided_id.filter(|id| !self.tool_call_ids.contains(*id)) {
            id.to_owned()
        } else {
            self.generated_tool_calls = self.generated_tool_calls.saturating_add(1);
            format!(
                "{}_{}_{}",
                name, self.output.timestamp, self.generated_tool_calls
            )
        };
        self.tool_call_ids.insert(id.clone());
        let arguments = call.get("args").cloned().unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return Err(ModelServiceError::protocol(
                "Google function-call arguments were not an object",
                false,
            ));
        }
        let mut tool_call = ToolCallBlock::new(id, name, arguments);
        tool_call.thought_signature = signature;
        let content_index = self.output.content.len();
        self.output
            .content
            .push(ContentBlock::ToolCall(tool_call.clone()));
        self.pending
            .push_back(Ok(AssistantMessageEvent::ToolCallStart {
                content_index,
                partial: self.output.clone(),
            }));
        self.pending
            .push_back(Ok(AssistantMessageEvent::ToolCallDelta {
                content_index,
                delta: tool_call.arguments.to_string(),
                partial: self.output.clone(),
            }));
        self.pending
            .push_back(Ok(AssistantMessageEvent::ToolCallEnd {
                content_index,
                tool_call,
                partial: self.output.clone(),
            }));
        Ok(())
    }

    fn finish_open_text(&mut self) {
        let Some((kind, content_index)) = self.open_text.take() else {
            return;
        };
        let event = match (kind, self.output.content.get(content_index)) {
            (OpenTextKind::Text, Some(ContentBlock::Text(block))) => {
                Some(AssistantMessageEvent::TextEnd {
                    content_index,
                    content: block.text.clone(),
                    partial: self.output.clone(),
                })
            }
            (OpenTextKind::Thinking, Some(ContentBlock::Thinking(block))) => {
                Some(AssistantMessageEvent::ThinkingEnd {
                    content_index,
                    content: block.thinking.clone(),
                    partial: self.output.clone(),
                })
            }
            _ => None,
        };
        if let Some(event) = event {
            self.pending.push_back(Ok(event));
        }
    }

    fn finalize_or_fail(&mut self) {
        let Some(mut reason) = self.completion_reason else {
            self.fail(ModelServiceError::protocol(
                "Google stream ended without a finish reason",
                true,
            ));
            return;
        };
        self.finish_open_text();
        if self
            .output
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolCall(_)))
        {
            reason = CompletionReason::ToolUse;
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
    let mut config = Map::new();
    if !request.system_prompt.is_empty() {
        config.insert(
            "systemInstruction".to_owned(),
            Value::String(request.system_prompt.clone()),
        );
    }
    if let Some(temperature) = request.options.temperature {
        config.insert("temperature".to_owned(), Value::from(temperature));
    }
    if let Some(max_tokens) = request.options.max_tokens {
        config.insert("maxOutputTokens".to_owned(), Value::from(max_tokens));
    }
    if !request.tools.is_empty() {
        config.insert(
            "tools".to_owned(),
            json!([{
                "functionDeclarations": request.tools.iter().map(|tool| json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parametersJsonSchema": tool.parameters
                })).collect::<Vec<_>>()
            }]),
        );
        if let Some(tool_choice) = &request.options.tool_choice {
            config.insert(
                "toolConfig".to_owned(),
                json!({
                    "functionCallingConfig": map_tool_choice(tool_choice)
                }),
            );
        }
    }
    if request.model.reasoning {
        if let Some(level) = request.options.reasoning {
            let mut thinking = Map::from_iter([("includeThoughts".to_owned(), Value::Bool(true))]);
            if uses_thinking_level(&request.model) {
                thinking.insert(
                    "thinkingLevel".to_owned(),
                    Value::String(map_thinking_level(&request.model, level).to_owned()),
                );
            } else {
                thinking.insert(
                    "thinkingBudget".to_owned(),
                    Value::from(thinking_budget(
                        &request.model,
                        level,
                        request.options.thinking_budgets,
                    )),
                );
            }
            config.insert("thinkingConfig".to_owned(), Value::Object(thinking));
        } else {
            config.insert(
                "thinkingConfig".to_owned(),
                disabled_thinking(&request.model),
            );
        }
    }
    Ok(json!({
        "model": request.model.id,
        "contents": convert_messages(request)?,
        "config": config
    }))
}

fn convert_messages(request: &ModelRequest) -> Result<Vec<Value>, ModelServiceError> {
    let mut contents = Vec::new();
    for message in &request.messages {
        match message {
            Message::User(message) => {
                let parts = match &message.content {
                    MessageContent::Text(text) => vec![json!({ "text": text })],
                    MessageContent::Blocks(blocks) => convert_user_parts(blocks)?,
                };
                if !parts.is_empty() {
                    contents.push(json!({ "role": "user", "parts": parts }));
                }
            }
            Message::Assistant(message) => {
                let same_model =
                    message.provider == request.model.provider && message.model == request.model.id;
                let mut parts = Vec::new();
                for block in &message.content {
                    match block {
                        ContentBlock::Text(block) if !block.text.trim().is_empty() => {
                            let mut part = json!({ "text": block.text });
                            retain_replay_signature(
                                &mut part,
                                same_model,
                                block.text_signature.as_deref(),
                            )?;
                            parts.push(part);
                        }
                        ContentBlock::Text(_) => {}
                        ContentBlock::Thinking(block) if !block.thinking.trim().is_empty() => {
                            let mut part = if same_model {
                                json!({ "thought": true, "text": block.thinking })
                            } else {
                                json!({ "text": block.thinking })
                            };
                            retain_replay_signature(
                                &mut part,
                                same_model,
                                block.thinking_signature.as_deref(),
                            )?;
                            parts.push(part);
                        }
                        ContentBlock::Thinking(_) => {}
                        ContentBlock::ToolCall(block) => {
                            let mut call = json!({
                                "name": block.name,
                                "args": block.arguments
                            });
                            if requires_tool_call_id(&request.model.id)
                                && let Some(call) = call.as_object_mut()
                            {
                                call.insert(
                                    "id".to_owned(),
                                    Value::String(normalize_id(&block.id)),
                                );
                            }
                            let mut part = json!({ "functionCall": call });
                            retain_replay_signature(
                                &mut part,
                                same_model,
                                block.thought_signature.as_deref(),
                            )?;
                            if same_model
                                && compat_bool(
                                    &request.model,
                                    "requiresThoughtSignature",
                                    request.model.reasoning,
                                )
                                && part.get("thoughtSignature").is_none()
                            {
                                return Err(ModelServiceError::new(
                                    ModelServiceErrorCategory::InvalidRequest,
                                    "Google reasoning tool call omitted a valid thought signature",
                                    false,
                                ));
                            }
                            parts.push(part);
                        }
                        ContentBlock::Image(_) => {
                            return Err(ModelServiceError::new(
                                ModelServiceErrorCategory::InvalidRequest,
                                "assistant messages cannot contain image blocks",
                                false,
                            ));
                        }
                    }
                }
                if !parts.is_empty() {
                    contents.push(json!({ "role": "model", "parts": parts }));
                }
            }
            Message::ToolResult(result) => {
                let text = result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text(block) => Some(block.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let images = result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Image(block)
                            if request
                                .model
                                .input
                                .contains(&pi_protocol::ModelInput::Image) =>
                        {
                            Some(block)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let response_text = if !text.is_empty() {
                    text
                } else if !images.is_empty() {
                    "(see attached image)".to_owned()
                } else {
                    String::new()
                };
                let mut response = if result.is_error {
                    json!({ "error": response_text })
                } else {
                    json!({ "output": response_text })
                };
                let multimodal = supports_multimodal_function_response(&request.model.id);
                let image_parts = images
                    .iter()
                    .map(|image| {
                        json!({
                            "inlineData": {
                                "mimeType": image.mime_type,
                                "data": image.data
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                let mut function_response = Map::from_iter([
                    ("name".to_owned(), Value::String(result.tool_name.clone())),
                    ("response".to_owned(), response.take()),
                ]);
                if !image_parts.is_empty() && multimodal {
                    function_response.insert("parts".to_owned(), Value::Array(image_parts.clone()));
                }
                if requires_tool_call_id(&request.model.id) {
                    function_response.insert(
                        "id".to_owned(),
                        Value::String(normalize_id(&result.tool_call_id)),
                    );
                }
                let part = json!({ "functionResponse": function_response });
                if let Some(last) = contents.last_mut()
                    && last.get("role").and_then(Value::as_str) == Some("user")
                    && last
                        .get("parts")
                        .and_then(Value::as_array)
                        .is_some_and(|parts| {
                            parts
                                .iter()
                                .any(|part| part.get("functionResponse").is_some())
                        })
                    && let Some(parts) = last.get_mut("parts").and_then(Value::as_array_mut)
                {
                    parts.push(part);
                } else {
                    contents.push(json!({ "role": "user", "parts": [part] }));
                }
                if !image_parts.is_empty() && !multimodal {
                    let mut parts = vec![json!({ "text": "Tool result image:" })];
                    parts.extend(image_parts);
                    contents.push(json!({ "role": "user", "parts": parts }));
                }
            }
        }
    }
    Ok(contents)
}

fn convert_user_parts(blocks: &[ContentBlock]) -> Result<Vec<Value>, ModelServiceError> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text(block) => Ok(json!({ "text": block.text })),
            ContentBlock::Image(block) => Ok(json!({
                "inlineData": { "mimeType": block.mime_type, "data": block.data }
            })),
            ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) => Err(ModelServiceError::new(
                ModelServiceErrorCategory::InvalidRequest,
                "user messages can contain only text and image blocks",
                false,
            )),
        })
        .collect()
}

fn retain_replay_signature(
    part: &mut Value,
    same_model: bool,
    signature: Option<&str>,
) -> Result<(), ModelServiceError> {
    if !same_model || signature.is_none() {
        return Ok(());
    }
    let signature = signature.unwrap_or_default();
    if !is_valid_signature(signature) {
        return Ok(());
    }
    let object = part.as_object_mut().ok_or_else(|| {
        ModelServiceError::new(
            ModelServiceErrorCategory::InvalidRequest,
            "Google replay part was not an object",
            false,
        )
    })?;
    object.insert(
        "thoughtSignature".to_owned(),
        Value::String(signature.to_owned()),
    );
    Ok(())
}

fn valid_signature(value: Option<&Value>) -> Result<Option<String>, ModelServiceError> {
    let Some(signature) = value.and_then(Value::as_str) else {
        return Ok(None);
    };
    if !is_valid_signature(signature) {
        return Err(ModelServiceError::protocol(
            "Google response contained an invalid thought signature",
            false,
        ));
    }
    Ok(Some(signature.to_owned()))
}

fn is_valid_signature(signature: &str) -> bool {
    !signature.is_empty()
        && signature.len().is_multiple_of(4)
        && base64::engine::general_purpose::STANDARD
            .decode(signature)
            .is_ok()
}

fn parse_usage(raw: &Value, model: &pi_protocol::Model) -> Usage {
    let prompt = raw
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let candidates = raw
        .get("candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let reasoning = raw.get("thoughtsTokenCount").and_then(Value::as_u64);
    let cache_read = raw
        .get("cachedContentTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let input = prompt.saturating_sub(cache_read);
    let output = candidates + reasoning.unwrap_or(0);
    let mut usage = Usage {
        input,
        output,
        cache_read,
        cache_write: 0,
        cache_write_1h: None,
        reasoning,
        total_tokens: raw
            .get("totalTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(input + output + cache_read),
        cost: pi_protocol::UsageCost::default(),
    };
    calculate_cost(model, &mut usage);
    usage
}

fn map_finish_reason(reason: &str) -> Result<CompletionReason, ModelServiceError> {
    match reason {
        "STOP" => Ok(CompletionReason::Stop),
        "MAX_TOKENS" => Ok(CompletionReason::Length),
        "SAFETY"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "RECITATION"
        | "LANGUAGE"
        | "MALFORMED_FUNCTION_CALL"
        | "UNEXPECTED_TOOL_CALL"
        | "OTHER"
        | "FINISH_REASON_UNSPECIFIED" => Err(ModelServiceError::new(
            ModelServiceErrorCategory::InvalidRequest,
            format!("Google response blocked with finish reason {reason}"),
            false,
        )),
        _ => Err(ModelServiceError::protocol(
            format!("unsupported Google finish reason: {reason}"),
            false,
        )),
    }
}

fn is_blocked_prompt(feedback: &Value) -> bool {
    feedback
        .get("blockReason")
        .and_then(Value::as_str)
        .is_some_and(|reason| !reason.is_empty() && reason != "BLOCK_REASON_UNSPECIFIED")
}

fn map_tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!({ "mode": "AUTO" }),
        ToolChoice::None => json!({ "mode": "NONE" }),
        ToolChoice::Required => json!({ "mode": "ANY" }),
        ToolChoice::Named(name) => json!({ "mode": "ANY", "allowedFunctionNames": [name] }),
    }
}

fn uses_thinking_level(model: &pi_protocol::Model) -> bool {
    let id = model.id.to_ascii_lowercase();
    id.contains("gemini-3") || id.contains("gemma-4") || id == "gemini-flash-latest"
}

fn map_thinking_level(model: &pi_protocol::Model, level: ThinkingLevel) -> &'static str {
    let id = model.id.to_ascii_lowercase();
    if id.contains("gemini-3") && id.contains("pro") {
        return match level {
            ThinkingLevel::Minimal | ThinkingLevel::Low => "LOW",
            ThinkingLevel::Medium
            | ThinkingLevel::High
            | ThinkingLevel::XHigh
            | ThinkingLevel::Max => "HIGH",
        };
    }
    match level {
        ThinkingLevel::Minimal => "MINIMAL",
        ThinkingLevel::Low => "LOW",
        ThinkingLevel::Medium => "MEDIUM",
        ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => "HIGH",
    }
}

fn thinking_budget(
    model: &pi_protocol::Model,
    level: ThinkingLevel,
    budgets: Option<ThinkingBudgets>,
) -> i64 {
    let configured = budgets.and_then(|budgets| match level {
        ThinkingLevel::Minimal => budgets.minimal,
        ThinkingLevel::Low => budgets.low,
        ThinkingLevel::Medium => budgets.medium,
        ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => budgets.high,
    });
    if let Some(configured) = configured {
        return i64::try_from(configured).unwrap_or(i64::MAX);
    }
    let id = model.id.to_ascii_lowercase();
    let tier = match level {
        ThinkingLevel::Minimal => 0,
        ThinkingLevel::Low => 1,
        ThinkingLevel::Medium => 2,
        ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => 3,
    };
    if id.contains("2.5-pro") {
        [128, 2_048, 8_192, 32_768][tier]
    } else if id.contains("2.5-flash-lite") {
        [512, 2_048, 8_192, 24_576][tier]
    } else if id.contains("2.5-flash") {
        [128, 2_048, 8_192, 24_576][tier]
    } else {
        -1
    }
}

fn disabled_thinking(model: &pi_protocol::Model) -> Value {
    let id = model.id.to_ascii_lowercase();
    if id.contains("gemini-3") && id.contains("pro") {
        json!({ "thinkingLevel": "LOW" })
    } else if id.contains("gemini-3") || id.contains("gemma-4") {
        json!({ "thinkingLevel": "MINIMAL" })
    } else {
        json!({ "thinkingBudget": 0 })
    }
}

fn requires_tool_call_id(model_id: &str) -> bool {
    model_id.starts_with("claude-") || model_id.starts_with("gpt-oss-")
}

fn supports_multimodal_function_response(model_id: &str) -> bool {
    let Some(rest) = model_id
        .to_ascii_lowercase()
        .strip_prefix("gemini-")
        .map(str::to_owned)
    else {
        return true;
    };
    let major = rest
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .and_then(|value| value.parse::<u64>().ok());
    major.is_none_or(|major| major >= 3)
}

fn normalize_id(id: &str) -> String {
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

const fn completion_stop_reason(reason: CompletionReason) -> StopReason {
    match reason {
        CompletionReason::Stop => StopReason::Stop,
        CompletionReason::Length => StopReason::Length,
        CompletionReason::ToolUse => StopReason::ToolUse,
    }
}

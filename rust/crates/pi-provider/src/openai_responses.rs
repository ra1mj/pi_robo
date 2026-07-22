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
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

const MAX_SSE_EVENT_BYTES: usize = 1_048_576;
const MIN_OUTPUT_TOKENS: u64 = 16;

/// Direct OpenAI Responses protocol adapter.
#[derive(Clone)]
pub struct OpenAiResponsesAdapter {
    http: ProviderHttpClient,
    clock: Arc<dyn ProviderClock>,
}

impl OpenAiResponsesAdapter {
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

impl ModelService for OpenAiResponsesAdapter {
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
                .post_json("responses", &payload, cancellation)
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
            let state = ResponsesStreamState::new(
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

enum ResponseSlot {
    Thinking {
        content_index: usize,
        item_id: Option<String>,
    },
    Text {
        content_index: usize,
    },
    ToolCall {
        content_index: usize,
        partial_arguments: String,
    },
}

struct ResponsesStreamState<'a> {
    response: ProviderHttpResponse,
    cancellation: &'a dyn Cancellation,
    http: ProviderHttpClient,
    model: pi_protocol::Model,
    decoder: SseDecoder,
    output: AssistantMessage,
    pending: VecDeque<Result<AssistantMessageEvent, ModelServiceError>>,
    slots: BTreeMap<usize, ResponseSlot>,
    finished_slots: BTreeSet<usize>,
    reasoning_by_id: BTreeMap<String, usize>,
    terminated: bool,
}

impl<'a> ResponsesStreamState<'a> {
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
            finished_slots: BTreeSet::new(),
            reasoning_by_id: BTreeMap::new(),
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
                            "OpenAI Responses stream ended before a terminal response event",
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
            let data = event.data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                self.fail(ModelServiceError::protocol(
                    "OpenAI Responses stream ended before a terminal response event",
                    true,
                ));
                continue;
            }
            let event: Value = match serde_json::from_str(data) {
                Ok(event) => event,
                Err(_) => {
                    self.fail(ModelServiceError::protocol(
                        "OpenAI Responses stream contained malformed JSON",
                        false,
                    ));
                    continue;
                }
            };
            if let Err(error) = self.process_event(&event) {
                self.fail(error);
            }
        }
    }

    fn process_event(&mut self, event: &Value) -> Result<(), ModelServiceError> {
        let event_type = event.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelServiceError::protocol("OpenAI Responses event omitted its type", false)
        })?;
        match event_type {
            "response.created" | "response.in_progress" | "response.queued" => {
                if let Some(response) = event.get("response") {
                    self.update_response_identity(response);
                }
            }
            "response.output_item.added" => {
                let output_index = output_index(event)?;
                let item = event.get("item").ok_or_else(|| {
                    ModelServiceError::protocol(
                        "response.output_item.added omitted its item",
                        false,
                    )
                })?;
                self.create_slot(output_index, item)?;
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let output_index = output_index(event)?;
                let delta = event.get("delta").and_then(Value::as_str).ok_or_else(|| {
                    ModelServiceError::protocol("reasoning delta omitted text", false)
                })?;
                self.push_thinking_delta(output_index, delta)?;
            }
            "response.reasoning_summary_part.done" => {
                let output_index = output_index(event)?;
                self.push_thinking_delta(output_index, "\n\n")?;
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                let output_index = output_index(event)?;
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ModelServiceError::protocol("text delta omitted text", false))?;
                self.push_text_delta(output_index, delta)?;
            }
            "response.function_call_arguments.delta" => {
                let output_index = output_index(event)?;
                let delta = event.get("delta").and_then(Value::as_str).ok_or_else(|| {
                    ModelServiceError::protocol("function-call delta omitted arguments", false)
                })?;
                self.push_tool_delta(output_index, delta)?;
            }
            "response.function_call_arguments.done" => {
                let output_index = output_index(event)?;
                let arguments =
                    event
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ModelServiceError::protocol(
                                "function-call done event omitted arguments",
                                false,
                            )
                        })?;
                self.replace_tool_arguments(output_index, arguments)?;
            }
            "response.output_item.done" => {
                let output_index = output_index(event)?;
                let item = event.get("item").ok_or_else(|| {
                    ModelServiceError::protocol("response.output_item.done omitted its item", false)
                })?;
                self.finish_slot(output_index, item)?;
            }
            "response.completed" => {
                let response = event.get("response").ok_or_else(|| {
                    ModelServiceError::protocol("response.completed omitted its response", false)
                })?;
                self.finish_response(response, CompletionReason::Stop)?;
            }
            "response.incomplete" => {
                let response = event.get("response").ok_or_else(|| {
                    ModelServiceError::protocol("response.incomplete omitted its response", false)
                })?;
                self.finish_response(response, CompletionReason::Length)?;
            }
            "response.failed" => {
                let response = event.get("response").unwrap_or(event);
                self.update_response_identity(response);
                let provider_error = response.get("error").unwrap_or(response);
                return Err(provider_event_error(
                    &self.http,
                    provider_error,
                    "OpenAI Responses request failed",
                ));
            }
            "error" => {
                return Err(provider_event_error(
                    &self.http,
                    event,
                    "OpenAI Responses stream error",
                ));
            }
            "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.refusal.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done" => {}
            _ => {
                if is_significant_unknown_event(event_type) {
                    return Err(ModelServiceError::protocol(
                        format!("unsupported OpenAI Responses event type: {event_type}"),
                        false,
                    ));
                }
            }
        }
        Ok(())
    }

    fn create_slot(&mut self, output_index: usize, item: &Value) -> Result<(), ModelServiceError> {
        if self.slots.contains_key(&output_index) || self.finished_slots.contains(&output_index) {
            return Err(ModelServiceError::protocol(
                "OpenAI Responses reused an output index",
                false,
            ));
        }
        let item_type = item.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelServiceError::protocol("response output item omitted its type", false)
        })?;
        let content_index = self.output.content.len();
        let slot = match item_type {
            "reasoning" => {
                self.output
                    .content
                    .push(ContentBlock::Thinking(ThinkingBlock::new("")));
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ThinkingStart {
                        content_index,
                        partial: self.output.clone(),
                    }));
                ResponseSlot::Thinking {
                    content_index,
                    item_id: non_empty_string(item.get("id")),
                }
            }
            "message" => {
                self.output
                    .content
                    .push(ContentBlock::Text(TextBlock::new("")));
                self.pending.push_back(Ok(AssistantMessageEvent::TextStart {
                    content_index,
                    partial: self.output.clone(),
                }));
                ResponseSlot::Text { content_index }
            }
            "function_call" => {
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let item_id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                let arguments = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.output
                    .content
                    .push(ContentBlock::ToolCall(ToolCallBlock::new(
                        combine_tool_call_id(call_id, item_id),
                        name,
                        json!({}),
                    )));
                self.pending
                    .push_back(Ok(AssistantMessageEvent::ToolCallStart {
                        content_index,
                        partial: self.output.clone(),
                    }));
                ResponseSlot::ToolCall {
                    content_index,
                    partial_arguments: arguments.to_owned(),
                }
            }
            _ => {
                return Err(ModelServiceError::protocol(
                    format!("unsupported OpenAI Responses output item: {item_type}"),
                    false,
                ));
            }
        };
        self.slots.insert(output_index, slot);
        Ok(())
    }

    fn push_thinking_delta(
        &mut self,
        output_index: usize,
        delta: &str,
    ) -> Result<(), ModelServiceError> {
        let Some(ResponseSlot::Thinking { content_index, .. }) = self.slots.get(&output_index)
        else {
            return Err(slot_mismatch("reasoning", output_index));
        };
        let content_index = *content_index;
        if let Some(ContentBlock::Thinking(block)) = self.output.content.get_mut(content_index) {
            block.thinking.push_str(delta);
        }
        self.pending
            .push_back(Ok(AssistantMessageEvent::ThinkingDelta {
                content_index,
                delta: delta.to_owned(),
                partial: self.output.clone(),
            }));
        Ok(())
    }

    fn push_text_delta(
        &mut self,
        output_index: usize,
        delta: &str,
    ) -> Result<(), ModelServiceError> {
        let Some(ResponseSlot::Text { content_index }) = self.slots.get(&output_index) else {
            return Err(slot_mismatch("text", output_index));
        };
        let content_index = *content_index;
        if let Some(ContentBlock::Text(block)) = self.output.content.get_mut(content_index) {
            block.text.push_str(delta);
        }
        self.pending.push_back(Ok(AssistantMessageEvent::TextDelta {
            content_index,
            delta: delta.to_owned(),
            partial: self.output.clone(),
        }));
        Ok(())
    }

    fn push_tool_delta(
        &mut self,
        output_index: usize,
        delta: &str,
    ) -> Result<(), ModelServiceError> {
        let Some(ResponseSlot::ToolCall {
            content_index,
            partial_arguments,
        }) = self.slots.get_mut(&output_index)
        else {
            return Err(slot_mismatch("function call", output_index));
        };
        partial_arguments.push_str(delta);
        let content_index = *content_index;
        if let Ok(arguments) = serde_json::from_str::<Value>(partial_arguments)
            && let Some(ContentBlock::ToolCall(block)) = self.output.content.get_mut(content_index)
        {
            block.arguments = arguments;
        }
        self.pending
            .push_back(Ok(AssistantMessageEvent::ToolCallDelta {
                content_index,
                delta: delta.to_owned(),
                partial: self.output.clone(),
            }));
        Ok(())
    }

    fn replace_tool_arguments(
        &mut self,
        output_index: usize,
        arguments: &str,
    ) -> Result<(), ModelServiceError> {
        let Some(ResponseSlot::ToolCall {
            content_index,
            partial_arguments,
        }) = self.slots.get_mut(&output_index)
        else {
            return Err(slot_mismatch("function call", output_index));
        };
        let previous = partial_arguments.clone();
        *partial_arguments = arguments.to_owned();
        if let Some(delta) = arguments
            .strip_prefix(&previous)
            .filter(|delta| !delta.is_empty())
        {
            let content_index = *content_index;
            self.pending
                .push_back(Ok(AssistantMessageEvent::ToolCallDelta {
                    content_index,
                    delta: delta.to_owned(),
                    partial: self.output.clone(),
                }));
        }
        Ok(())
    }

    fn finish_slot(&mut self, output_index: usize, item: &Value) -> Result<(), ModelServiceError> {
        if self.finished_slots.contains(&output_index) {
            return Ok(());
        }
        if !self.slots.contains_key(&output_index) {
            self.create_slot(output_index, item)?;
        }
        let item_type = item.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelServiceError::protocol("response output item omitted its type", false)
        })?;
        match item_type {
            "reasoning" => self.finish_thinking(output_index, item)?,
            "message" => self.finish_text(output_index, item)?,
            "function_call" => self.finish_tool_call(output_index, item)?,
            _ => {
                return Err(ModelServiceError::protocol(
                    format!("unsupported OpenAI Responses output item: {item_type}"),
                    false,
                ));
            }
        }
        self.slots.remove(&output_index);
        self.finished_slots.insert(output_index);
        Ok(())
    }

    fn finish_thinking(
        &mut self,
        output_index: usize,
        item: &Value,
    ) -> Result<(), ModelServiceError> {
        let Some(ResponseSlot::Thinking {
            content_index,
            item_id,
        }) = self.slots.get(&output_index)
        else {
            return Err(slot_mismatch("reasoning", output_index));
        };
        let content_index = *content_index;
        let item_id = non_empty_string(item.get("id")).or_else(|| item_id.clone());
        let complete_text = joined_item_text(item, "summary", "text")?
            .or(joined_item_text(item, "content", "text")?);
        let signature = serde_json::to_string(item).map_err(|_| {
            ModelServiceError::protocol("reasoning item could not be serialized", false)
        })?;
        let content = if let Some(ContentBlock::Thinking(block)) =
            self.output.content.get_mut(content_index)
        {
            if let Some(complete_text) = complete_text {
                block.thinking = complete_text;
            }
            block.thinking_signature = Some(signature);
            block.thinking.clone()
        } else {
            return Err(slot_mismatch("reasoning", output_index));
        };
        if let Some(item_id) = item_id {
            self.reasoning_by_id.insert(item_id, content_index);
        }
        self.pending
            .push_back(Ok(AssistantMessageEvent::ThinkingEnd {
                content_index,
                content,
                partial: self.output.clone(),
            }));
        Ok(())
    }

    fn finish_text(&mut self, output_index: usize, item: &Value) -> Result<(), ModelServiceError> {
        let Some(ResponseSlot::Text { content_index }) = self.slots.get(&output_index) else {
            return Err(slot_mismatch("text", output_index));
        };
        let content_index = *content_index;
        let complete_text = response_message_text(item)?;
        let item_id = item.get("id").and_then(Value::as_str).unwrap_or_default();
        let phase = item.get("phase").and_then(Value::as_str);
        let signature = encode_text_signature(item_id, phase);
        let content =
            if let Some(ContentBlock::Text(block)) = self.output.content.get_mut(content_index) {
                if let Some(complete_text) = complete_text {
                    block.text = complete_text;
                }
                if !item_id.is_empty() {
                    block.text_signature = Some(signature);
                }
                block.text.clone()
            } else {
                return Err(slot_mismatch("text", output_index));
            };
        self.pending.push_back(Ok(AssistantMessageEvent::TextEnd {
            content_index,
            content,
            partial: self.output.clone(),
        }));
        Ok(())
    }

    fn finish_tool_call(
        &mut self,
        output_index: usize,
        item: &Value,
    ) -> Result<(), ModelServiceError> {
        let Some(ResponseSlot::ToolCall {
            content_index,
            partial_arguments,
        }) = self.slots.get(&output_index)
        else {
            return Err(slot_mismatch("function call", output_index));
        };
        let content_index = *content_index;
        let arguments = item
            .get("arguments")
            .and_then(Value::as_str)
            .filter(|arguments| !arguments.is_empty())
            .unwrap_or(partial_arguments);
        let arguments = match serde_json::from_str::<Value>(arguments) {
            Ok(Value::Object(arguments)) => Value::Object(arguments),
            _ => {
                return Err(ModelServiceError::protocol(
                    "OpenAI Responses function-call arguments were malformed or truncated",
                    false,
                ));
            }
        };
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let item_id = item.get("id").and_then(Value::as_str).unwrap_or_default();
        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
        let tool_call = if let Some(ContentBlock::ToolCall(block)) =
            self.output.content.get_mut(content_index)
        {
            if !call_id.is_empty() || !item_id.is_empty() {
                block.id = combine_tool_call_id(call_id, item_id);
            }
            if !name.is_empty() {
                block.name = name.to_owned();
            }
            if block.id.is_empty() || block.name.is_empty() {
                return Err(ModelServiceError::protocol(
                    "OpenAI Responses function call omitted its identifier or name",
                    false,
                ));
            }
            block.arguments = arguments;
            block.clone()
        } else {
            return Err(slot_mismatch("function call", output_index));
        };
        self.pending
            .push_back(Ok(AssistantMessageEvent::ToolCallEnd {
                content_index,
                tool_call,
                partial: self.output.clone(),
            }));
        Ok(())
    }

    fn finish_response(
        &mut self,
        response: &Value,
        event_reason: CompletionReason,
    ) -> Result<(), ModelServiceError> {
        self.update_response_identity(response);
        if let Some(items) = response.get("output").and_then(Value::as_array) {
            self.backfill_reasoning_signatures(items)?;
            for (output_index, item) in items.iter().enumerate() {
                if !self.finished_slots.contains(&output_index) {
                    self.finish_slot(output_index, item)?;
                }
            }
        }
        if !self.slots.is_empty() {
            return Err(ModelServiceError::protocol(
                "OpenAI Responses terminal event left output items unfinished",
                false,
            ));
        }
        if let Some(usage) = response.get("usage") {
            self.output.usage = parse_usage(usage, &self.model);
        }
        let reason = match response.get("status").and_then(Value::as_str) {
            Some("completed") => CompletionReason::Stop,
            Some("incomplete") => CompletionReason::Length,
            Some("failed" | "cancelled") => {
                return Err(provider_event_error(
                    &self.http,
                    response.get("error").unwrap_or(response),
                    "OpenAI Responses request failed",
                ));
            }
            Some("in_progress" | "queued") | None => event_reason,
            Some(status) => {
                return Err(ModelServiceError::protocol(
                    format!("unsupported OpenAI Responses status: {status}"),
                    false,
                ));
            }
        };
        let reason = if reason == CompletionReason::Stop
            && self
                .output
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolCall(_)))
        {
            CompletionReason::ToolUse
        } else {
            reason
        };
        self.output.stop_reason = completion_stop_reason(reason);
        self.pending.push_back(Ok(AssistantMessageEvent::Done {
            reason,
            message: self.output.clone(),
        }));
        self.terminated = true;
        Ok(())
    }

    fn backfill_reasoning_signatures(&mut self, items: &[Value]) -> Result<(), ModelServiceError> {
        for item in items {
            if item.get("type").and_then(Value::as_str) != Some("reasoning") {
                continue;
            }
            let Some(encrypted) = item
                .get("encrypted_content")
                .filter(|value| !value.is_null())
            else {
                continue;
            };
            let Some(item_id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(content_index) = self.reasoning_by_id.get(item_id).copied() else {
                continue;
            };
            let Some(ContentBlock::Thinking(block)) = self.output.content.get_mut(content_index)
            else {
                continue;
            };
            let Some(signature) = &block.thinking_signature else {
                continue;
            };
            let mut stored = serde_json::from_str::<Value>(signature).map_err(|_| {
                ModelServiceError::protocol("stored reasoning signature was malformed", false)
            })?;
            if stored.get("encrypted_content").is_none()
                && let Some(stored) = stored.as_object_mut()
            {
                stored.insert("encrypted_content".to_owned(), encrypted.clone());
                block.thinking_signature = Some(serde_json::to_string(&stored).map_err(|_| {
                    ModelServiceError::protocol(
                        "reasoning signature could not be serialized",
                        false,
                    )
                })?);
            }
        }
        Ok(())
    }

    fn update_response_identity(&mut self, response: &Value) {
        if let Some(response_id) = non_empty_string(response.get("id")) {
            self.output.response_id = Some(response_id);
        }
        if let Some(response_model) = non_empty_string(response.get("model"))
            && response_model != self.model.id
        {
            self.output.response_model = Some(response_model);
        }
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
    payload.insert("input".to_owned(), Value::Array(convert_input(request)?));
    payload.insert("stream".to_owned(), Value::Bool(true));
    payload.insert("store".to_owned(), Value::Bool(false));
    if let Some(max_tokens) = request.options.max_tokens {
        payload.insert(
            "max_output_tokens".to_owned(),
            Value::from(max_tokens.max(MIN_OUTPUT_TOKENS)),
        );
    }
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
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                            "strict": false
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(tool_choice) = &request.options.tool_choice {
        payload.insert("tool_choice".to_owned(), map_tool_choice(tool_choice));
    }
    if request.options.cache_retention != CacheRetention::None {
        if let Some(session_id) = &request.options.session_id {
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
    if request.model.reasoning {
        if let Some(reasoning) = request.options.reasoning {
            payload.insert(
                "reasoning".to_owned(),
                json!({
                    "effort": map_thinking_level(&request.model, reasoning),
                    "summary": "auto"
                }),
            );
            payload.insert("include".to_owned(), json!(["reasoning.encrypted_content"]));
        } else if let Some(off) = request
            .model
            .thinking_level_map
            .as_ref()
            .and_then(|mapping| mapping.get("off"))
            .and_then(Clone::clone)
        {
            payload.insert("reasoning".to_owned(), json!({ "effort": off }));
        }
    }
    Ok(Value::Object(payload))
}

fn convert_input(request: &ModelRequest) -> Result<Vec<Value>, ModelServiceError> {
    let mut input = Vec::new();
    if !request.system_prompt.is_empty() {
        let developer = request.model.reasoning
            && compat_bool(
                &request.model,
                "supportsDeveloperRole",
                request.model.provider == "openai",
            );
        input.push(json!({
            "role": if developer { "developer" } else { "system" },
            "content": request.system_prompt
        }));
    }

    for (message_index, message) in request.messages.iter().enumerate() {
        match message {
            Message::User(message) => {
                let content = convert_user_content(&message.content)?;
                if !content.is_empty() {
                    input.push(json!({ "role": "user", "content": content }));
                }
            }
            Message::Assistant(message) => {
                let different_model = message.model != request.model.id
                    && message.provider == request.model.provider
                    && message.api == request.model.api;
                let mut text_index = 0;
                for block in &message.content {
                    match block {
                        ContentBlock::Thinking(block) => {
                            if let Some(signature) = &block.thinking_signature {
                                let item =
                                    serde_json::from_str::<Value>(signature).map_err(|_| {
                                        ModelServiceError::new(
                                            ModelServiceErrorCategory::InvalidRequest,
                                            "assistant reasoning signature is malformed",
                                            false,
                                        )
                                    })?;
                                input.push(item);
                            }
                        }
                        ContentBlock::Text(block) => {
                            let fallback = if text_index == 0 {
                                format!("msg_pi_{message_index}")
                            } else {
                                format!("msg_pi_{message_index}_{text_index}")
                            };
                            text_index += 1;
                            let (id, phase) =
                                parse_text_signature(block.text_signature.as_deref(), &fallback);
                            let mut item = Map::from_iter([
                                ("type".to_owned(), Value::String("message".to_owned())),
                                ("role".to_owned(), Value::String("assistant".to_owned())),
                                (
                                    "content".to_owned(),
                                    json!([{
                                        "type": "output_text",
                                        "text": block.text,
                                        "annotations": []
                                    }]),
                                ),
                                ("status".to_owned(), Value::String("completed".to_owned())),
                                ("id".to_owned(), Value::String(id)),
                            ]);
                            if let Some(phase) = phase {
                                item.insert("phase".to_owned(), Value::String(phase));
                            }
                            input.push(Value::Object(item));
                        }
                        ContentBlock::ToolCall(block) => {
                            let (call_id, item_id) = split_tool_call_id(&block.id);
                            let mut item = Map::from_iter([
                                ("type".to_owned(), Value::String("function_call".to_owned())),
                                ("call_id".to_owned(), Value::String(call_id)),
                                ("name".to_owned(), Value::String(block.name.clone())),
                                (
                                    "arguments".to_owned(),
                                    Value::String(block.arguments.to_string()),
                                ),
                            ]);
                            if !different_model && let Some(item_id) = item_id {
                                item.insert("id".to_owned(), Value::String(item_id));
                            }
                            input.push(Value::Object(item));
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
            }
            Message::ToolResult(result) => {
                let (call_id, _) = split_tool_call_id(&result.tool_call_id);
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
                        ContentBlock::Image(block) => Some(block),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let output = if !images.is_empty()
                    && request
                        .model
                        .input
                        .contains(&pi_protocol::ModelInput::Image)
                {
                    let mut parts = Vec::new();
                    if !text.is_empty() {
                        parts.push(json!({ "type": "input_text", "text": text }));
                    }
                    parts.extend(images.into_iter().map(|image| {
                        json!({
                            "type": "input_image",
                            "detail": "auto",
                            "image_url": format!(
                                "data:{};base64,{}",
                                image.mime_type, image.data
                            )
                        })
                    }));
                    Value::Array(parts)
                } else if !text.is_empty() {
                    Value::String(text)
                } else if !images.is_empty() {
                    Value::String("(see attached image)".to_owned())
                } else {
                    Value::String("(no tool output)".to_owned())
                };
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output
                }));
            }
        }
    }
    Ok(input)
}

fn convert_user_content(content: &MessageContent) -> Result<Vec<Value>, ModelServiceError> {
    match content {
        MessageContent::Text(text) => Ok(vec![json!({ "type": "input_text", "text": text })]),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text(block) => {
                    Ok(json!({ "type": "input_text", "text": block.text }))
                }
                ContentBlock::Image(block) => Ok(json!({
                    "type": "input_image",
                    "detail": "auto",
                    "image_url": format!("data:{};base64,{}", block.mime_type, block.data)
                })),
                ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) => {
                    Err(ModelServiceError::new(
                        ModelServiceErrorCategory::InvalidRequest,
                        "user messages can contain only text and image blocks",
                        false,
                    ))
                }
            })
            .collect(),
    }
}

fn parse_usage(raw: &Value, model: &pi_protocol::Model) -> Usage {
    let prompt = raw.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let output = raw
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let details = raw.get("input_tokens_details");
    let cache_read = details
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write = details
        .and_then(|details| details.get("cache_write_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let input = prompt
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    let reasoning = raw
        .get("output_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64);
    let mut usage = Usage {
        input,
        output,
        cache_read,
        cache_write,
        cache_write_1h: None,
        reasoning,
        total_tokens: raw
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(input + output + cache_read + cache_write),
        cost: pi_protocol::UsageCost::default(),
    };
    calculate_cost(model, &mut usage);
    usage
}

fn response_message_text(item: &Value) -> Result<Option<String>, ModelServiceError> {
    let Some(content) = item.get("content").and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut text = String::new();
    for part in content {
        match part.get("type").and_then(Value::as_str) {
            Some("output_text") => {
                text.push_str(part.get("text").and_then(Value::as_str).unwrap_or_default());
            }
            Some("refusal") => {
                text.push_str(
                    part.get("refusal")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                );
            }
            Some(part_type) => {
                return Err(ModelServiceError::protocol(
                    format!("unsupported OpenAI Responses message content: {part_type}"),
                    false,
                ));
            }
            None => {
                return Err(ModelServiceError::protocol(
                    "OpenAI Responses message content omitted its type",
                    false,
                ));
            }
        }
    }
    Ok(Some(text))
}

fn joined_item_text(
    item: &Value,
    field: &str,
    text_field: &str,
) -> Result<Option<String>, ModelServiceError> {
    let Some(parts) = item.get(field).and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut values = Vec::new();
    for part in parts {
        let Some(text) = part.get(text_field).and_then(Value::as_str) else {
            return Err(ModelServiceError::protocol(
                format!("OpenAI Responses {field} item omitted text"),
                false,
            ));
        };
        values.push(text);
    }
    Ok((!values.is_empty()).then(|| values.join("\n\n")))
}

fn parse_text_signature(signature: Option<&str>, fallback: &str) -> (String, Option<String>) {
    let parsed = signature
        .filter(|signature| signature.starts_with('{'))
        .and_then(|signature| serde_json::from_str::<Value>(signature).ok());
    let id = parsed
        .as_ref()
        .filter(|value| value.get("v").and_then(Value::as_u64) == Some(1))
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .or(signature)
        .unwrap_or(fallback);
    let phase = parsed
        .as_ref()
        .and_then(|value| value.get("phase"))
        .and_then(Value::as_str)
        .filter(|phase| matches!(*phase, "commentary" | "final_answer"))
        .map(str::to_owned);
    (clamp_message_id(id), phase)
}

fn encode_text_signature(id: &str, phase: Option<&str>) -> String {
    let mut signature = json!({ "v": 1, "id": id });
    if matches!(phase, Some("commentary" | "final_answer"))
        && let Some(object) = signature.as_object_mut()
    {
        object.insert(
            "phase".to_owned(),
            Value::String(phase.unwrap_or_default().to_owned()),
        );
    }
    signature.to_string()
}

fn clamp_message_id(id: &str) -> String {
    if id.len() <= 64 {
        return id.to_owned();
    }
    format!("msg_{:016x}", stable_hash(id.as_bytes()))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn split_tool_call_id(id: &str) -> (String, Option<String>) {
    match id.split_once('|') {
        Some((call_id, item_id)) => (call_id.to_owned(), Some(item_id.to_owned())),
        None => (id.to_owned(), None),
    }
}

fn combine_tool_call_id(call_id: &str, item_id: &str) -> String {
    match (call_id.is_empty(), item_id.is_empty()) {
        (false, false) => format!("{call_id}|{item_id}"),
        (false, true) => call_id.to_owned(),
        (true, false) => item_id.to_owned(),
        (true, true) => String::new(),
    }
}

fn map_tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => Value::String("auto".to_owned()),
        ToolChoice::None => Value::String("none".to_owned()),
        ToolChoice::Required => Value::String("required".to_owned()),
        ToolChoice::Named(name) => json!({ "type": "function", "name": name }),
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

fn output_index(event: &Value) -> Result<usize, ModelServiceError> {
    event
        .get("output_index")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            ModelServiceError::protocol("OpenAI Responses event omitted output_index", false)
        })
}

fn slot_mismatch(expected: &str, output_index: usize) -> ModelServiceError {
    ModelServiceError::protocol(
        format!("OpenAI Responses {expected} event has no slot at index {output_index}"),
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

fn is_significant_unknown_event(event_type: &str) -> bool {
    event_type.starts_with("response.output_")
        || event_type.starts_with("response.reasoning_")
        || event_type.starts_with("response.function_call_")
        || matches!(
            event_type,
            "response.cancelled" | "response.error" | "response.done"
        )
}

//! Provider-neutral agent loop and tool execution contracts.

use futures_util::stream::{FuturesUnordered, StreamExt};
pub use pi_model::Cancellation;
use pi_model::{
    ModelRequest, ModelRequestOptions, ModelService, ModelServiceError, ModelServiceErrorCategory,
};
use pi_protocol::{
    AgentEvent, AssistantMessage, AssistantMessageEvent, ContentBlock, Message, MessageContent,
    ModelInput, StopReason, TextBlock, ToolCallBlock, ToolDefinition, ToolResultMessage, Usage,
    UserMessage,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Result returned by a tool implementation.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutput {
    pub content: Vec<ContentBlock>,
    pub details: Option<Value>,
}

impl ToolOutput {
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::Text(TextBlock::new(text))],
            details: None,
        }
    }
}

/// Object-safe future returned by a tool.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

/// Object-safe future returned by a progress sink.
pub type ToolUpdateFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ToolError>> + Send + 'a>>;

/// Stable tool failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolErrorCategory {
    InvalidArguments,
    Execution,
    Cancelled,
}

/// Structured tool failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolError {
    pub category: ToolErrorCategory,
    pub message: String,
}

impl ToolError {
    #[must_use]
    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self {
            category: ToolErrorCategory::InvalidArguments,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn execution(message: impl Into<String>) -> Self {
        Self {
            category: ToolErrorCategory::Execution,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn cancelled() -> Self {
        Self::cancelled_with("tool execution cancelled")
    }

    #[must_use]
    pub fn cancelled_with(message: impl Into<String>) -> Self {
        Self {
            category: ToolErrorCategory::Cancelled,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

/// Progress delivery boundary scoped to one tool call.
pub trait ToolUpdateSink: Send + Sync {
    fn send<'a>(&'a self, partial_result: Value) -> ToolUpdateFuture<'a>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToolExecutionMode {
    #[default]
    Parallel,
    Sequential,
}

/// Tool execution boundary used by the agent loop.
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCallBlock,
        cancellation: &'a dyn Cancellation,
        updates: &'a dyn ToolUpdateSink,
    ) -> ToolFuture<'a>;
}

/// Object-safe future returned by an event sink.
pub type EventFuture<'a> = Pin<Box<dyn Future<Output = Result<(), EventSinkError>> + Send + 'a>>;

/// Event delivery failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventSinkError {
    pub message: String,
}

impl EventSinkError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for EventSinkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EventSinkError {}

/// Ordered, backpressured event delivery boundary.
pub trait EventSink: Send + Sync {
    fn emit<'a>(&'a self, event: AgentEvent) -> EventFuture<'a>;
}

/// Sender half of a bounded agent-event channel.
#[derive(Clone, Debug)]
pub struct ChannelEventSink {
    sender: mpsc::Sender<AgentEvent>,
}

impl EventSink for ChannelEventSink {
    fn emit<'a>(&'a self, event: AgentEvent) -> EventFuture<'a> {
        Box::pin(async move {
            self.sender
                .send(event)
                .await
                .map_err(|_| EventSinkError::new("agent event receiver closed"))
        })
    }
}

/// Creates the single bounded channel used to observe one or more agent runs.
#[must_use]
pub fn event_channel(capacity: usize) -> (ChannelEventSink, mpsc::Receiver<AgentEvent>) {
    let (sender, receiver) = mpsc::channel(capacity.max(1));
    (ChannelEventSink { sender }, receiver)
}

/// Time source for deterministic message timestamps.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Wall-clock implementation used outside tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
            })
    }
}

/// Inputs for one low-level agent run.
pub struct AgentRunRequest {
    pub model_request: ModelRequest,
    pub prompt: Option<UserMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub parallel_tools: bool,
}

/// Terminal status for an agent run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentRunStatus {
    Completed,
    Failed(ModelServiceError),
    Cancelled,
}

/// Completed low-level agent run, including the canonical updated history.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentRunResult {
    pub messages: Vec<Message>,
    pub new_messages: Vec<Message>,
    pub usage: Usage,
    pub status: AgentRunStatus,
}

/// Infrastructure failure that prevents the loop from settling its contract.
#[derive(Debug)]
pub enum AgentRunError {
    EventSink(EventSinkError),
}

impl std::fmt::Display for AgentRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventSink(error) => write!(formatter, "event sink failed: {error}"),
        }
    }
}

impl std::error::Error for AgentRunError {}

impl From<EventSinkError> for AgentRunError {
    fn from(error: EventSinkError) -> Self {
        Self::EventSink(error)
    }
}

/// Provider-neutral low-level agent loop.
pub struct Agent<'a> {
    model_service: &'a dyn ModelService,
    event_sink: &'a dyn EventSink,
    clock: &'a dyn Clock,
}

impl<'a> Agent<'a> {
    #[must_use]
    pub const fn new(
        model_service: &'a dyn ModelService,
        event_sink: &'a dyn EventSink,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            model_service,
            event_sink,
            clock,
        }
    }

    pub async fn run(
        &self,
        mut request: AgentRunRequest,
        cancellation: &dyn Cancellation,
    ) -> Result<AgentRunResult, AgentRunError> {
        let mut history = std::mem::take(&mut request.model_request.messages);
        let mut new_messages = Vec::new();
        let mut total_usage = Usage::default();
        let tool_registry = request
            .tools
            .iter()
            .map(|tool| (tool.definition().name, Arc::clone(tool)))
            .collect::<BTreeMap<_, _>>();
        request.model_request.tools = request.tools.iter().map(|tool| tool.definition()).collect();

        self.emit(AgentEvent::AgentStart).await?;
        self.emit(AgentEvent::TurnStart).await?;

        if let Some(prompt) = request.prompt.take() {
            let message = Message::User(prompt);
            self.emit(AgentEvent::MessageStart {
                message: message.clone(),
            })
            .await?;
            history.push(message.clone());
            new_messages.push(message.clone());
            self.emit(AgentEvent::MessageEnd { message }).await?;
        }

        let status = loop {
            if cancellation.is_cancelled() {
                let error = ModelServiceError::cancelled();
                let assistant = self.error_message(&request.model_request, None, &error);
                let message = Message::Assistant(assistant.clone());
                self.emit(AgentEvent::MessageStart {
                    message: message.clone(),
                })
                .await?;
                self.emit(AgentEvent::MessageEnd { message }).await?;
                self.record_terminal_assistant(
                    &mut history,
                    &mut new_messages,
                    &mut total_usage,
                    assistant,
                )
                .await?;
                break AgentRunStatus::Cancelled;
            }

            request.model_request.messages = prepare_messages_for_model(
                &history,
                request
                    .model_request
                    .model
                    .input
                    .contains(&ModelInput::Image),
            );
            let streamed = self
                .stream_assistant(&request.model_request, cancellation)
                .await?;
            match streamed {
                StreamedAssistant::Completed(assistant) => {
                    add_usage(&mut total_usage, &assistant.usage);
                    let assistant_message = Message::Assistant(assistant.clone());
                    history.push(assistant_message.clone());
                    new_messages.push(assistant_message.clone());

                    let tool_calls = assistant
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::ToolCall(call) => Some(call.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    let mut tool_results = Vec::new();

                    if !tool_calls.is_empty() {
                        tool_results = if assistant.stop_reason == StopReason::Length {
                            self.reject_truncated_calls(&tool_calls).await?
                        } else {
                            self.execute_tool_calls(
                                &tool_calls,
                                &tool_registry,
                                request.parallel_tools,
                                cancellation,
                            )
                            .await?
                        };

                        for result in &tool_results {
                            let message = Message::ToolResult(result.clone());
                            self.emit(AgentEvent::MessageStart {
                                message: message.clone(),
                            })
                            .await?;
                            history.push(message.clone());
                            new_messages.push(message.clone());
                            self.emit(AgentEvent::MessageEnd { message }).await?;
                        }
                    }

                    self.emit(AgentEvent::TurnEnd {
                        message: assistant_message,
                        tool_results: tool_results
                            .iter()
                            .map(|result| serde_json::to_value(result).unwrap_or(Value::Null))
                            .collect(),
                    })
                    .await?;

                    if tool_calls.is_empty() {
                        break AgentRunStatus::Completed;
                    }
                    if cancellation.is_cancelled() {
                        break AgentRunStatus::Cancelled;
                    }
                    self.emit(AgentEvent::TurnStart).await?;
                }
                StreamedAssistant::Failed { assistant, error } => {
                    let status = if error.category == ModelServiceErrorCategory::Cancelled {
                        AgentRunStatus::Cancelled
                    } else {
                        AgentRunStatus::Failed(error)
                    };
                    self.record_terminal_assistant(
                        &mut history,
                        &mut new_messages,
                        &mut total_usage,
                        assistant,
                    )
                    .await?;
                    break status;
                }
            }
        };

        self.emit(AgentEvent::AgentEnd {
            messages: new_messages.clone(),
        })
        .await?;

        Ok(AgentRunResult {
            messages: history,
            new_messages,
            usage: total_usage,
            status,
        })
    }

    async fn stream_assistant(
        &self,
        request: &ModelRequest,
        cancellation: &dyn Cancellation,
    ) -> Result<StreamedAssistant, AgentRunError> {
        let mut stream = match self
            .model_service
            .stream(request.clone(), cancellation)
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                let assistant = self.error_message(request, None, &error);
                let message = Message::Assistant(assistant.clone());
                self.emit(AgentEvent::MessageStart {
                    message: message.clone(),
                })
                .await?;
                self.emit(AgentEvent::MessageEnd { message }).await?;
                return Ok(StreamedAssistant::Failed { assistant, error });
            }
        };
        let mut partial = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(AssistantMessageEvent::Start {
                    partial: started, ..
                }) => {
                    partial = Some(started.clone());
                    self.emit(AgentEvent::MessageStart {
                        message: Message::Assistant(started),
                    })
                    .await?;
                }
                Ok(AssistantMessageEvent::Done { message, .. }) => {
                    self.emit(AgentEvent::MessageEnd {
                        message: Message::Assistant(message.clone()),
                    })
                    .await?;
                    return Ok(StreamedAssistant::Completed(message));
                }
                Ok(event) => {
                    if let Some(updated) = event_partial(&event) {
                        partial = Some(updated.clone());
                        self.emit(AgentEvent::MessageUpdate {
                            message: Message::Assistant(updated),
                            assistant_message_event: Box::new(event),
                        })
                        .await?;
                    }
                }
                Err(error) => {
                    let assistant = self.error_message(request, partial, &error);
                    self.emit(AgentEvent::MessageEnd {
                        message: Message::Assistant(assistant.clone()),
                    })
                    .await?;
                    return Ok(StreamedAssistant::Failed { assistant, error });
                }
            }
        }
        unreachable!("ModelEventStream guarantees one terminal outcome")
    }

    async fn record_terminal_assistant(
        &self,
        history: &mut Vec<Message>,
        new_messages: &mut Vec<Message>,
        total_usage: &mut Usage,
        assistant: AssistantMessage,
    ) -> Result<(), AgentRunError> {
        add_usage(total_usage, &assistant.usage);
        let message = Message::Assistant(assistant);
        history.push(message.clone());
        new_messages.push(message.clone());
        self.emit(AgentEvent::TurnEnd {
            message,
            tool_results: Vec::new(),
        })
        .await
    }

    fn error_message(
        &self,
        request: &ModelRequest,
        partial: Option<AssistantMessage>,
        error: &ModelServiceError,
    ) -> AssistantMessage {
        let stop_reason = if error.category == ModelServiceErrorCategory::Cancelled {
            StopReason::Aborted
        } else {
            StopReason::Error
        };
        let mut assistant = partial.unwrap_or_else(|| {
            AssistantMessage::new(
                Vec::new(),
                request.model.api.clone(),
                request.model.provider.clone(),
                request.model.id.clone(),
                Usage::default(),
                stop_reason,
                self.clock.now_ms(),
            )
        });
        assistant.stop_reason = stop_reason;
        assistant.error_message = Some(error.message.clone());
        assistant
    }

    async fn reject_truncated_calls(
        &self,
        calls: &[ToolCallBlock],
    ) -> Result<Vec<ToolResultMessage>, AgentRunError> {
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            self.emit(AgentEvent::ToolExecutionStart {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                args: call.arguments.clone(),
            })
            .await?;
            let result = tool_error_result(
                call,
                "Tool call was not executed because the assistant response reached its output limit",
                self.clock.now_ms(),
            );
            self.emit(AgentEvent::ToolExecutionEnd {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                result: serde_json::to_value(&result).unwrap_or(Value::Null),
                is_error: true,
            })
            .await?;
            results.push(result);
        }
        Ok(results)
    }

    async fn execute_tool_calls(
        &self,
        calls: &[ToolCallBlock],
        tools: &BTreeMap<String, Arc<dyn Tool>>,
        parallel: bool,
        cancellation: &dyn Cancellation,
    ) -> Result<Vec<ToolResultMessage>, AgentRunError> {
        let parallel = parallel
            && calls.iter().all(|call| {
                tools
                    .get(&call.name)
                    .is_none_or(|tool| tool.execution_mode() == ToolExecutionMode::Parallel)
            });
        if parallel {
            for call in calls {
                self.emit_tool_start(call).await?;
            }
            let mut pending = FuturesUnordered::new();
            for (index, call) in calls.iter().enumerate() {
                pending.push(self.execute_one_tool(index, call, tools, cancellation));
            }
            let mut source_order = vec![None; calls.len()];
            while let Some((index, call, result)) = pending.next().await {
                self.emit_tool_end(&call, &result).await?;
                source_order[index] = Some(result);
            }
            Ok(source_order.into_iter().flatten().collect())
        } else {
            let mut results = Vec::with_capacity(calls.len());
            for (index, call) in calls.iter().enumerate() {
                self.emit_tool_start(call).await?;
                let (_, call, result) = self
                    .execute_one_tool(index, call, tools, cancellation)
                    .await;
                self.emit_tool_end(&call, &result).await?;
                results.push(result);
            }
            Ok(results)
        }
    }

    async fn emit_tool_start(&self, call: &ToolCallBlock) -> Result<(), AgentRunError> {
        self.emit(AgentEvent::ToolExecutionStart {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            args: call.arguments.clone(),
        })
        .await
    }

    async fn execute_one_tool(
        &self,
        index: usize,
        call: &ToolCallBlock,
        tools: &BTreeMap<String, Arc<dyn Tool>>,
        cancellation: &dyn Cancellation,
    ) -> (usize, ToolCallBlock, ToolResultMessage) {
        let result = if cancellation.is_cancelled() {
            tool_error_result(call, "Tool execution cancelled", self.clock.now_ms())
        } else if let Some(tool) = tools.get(&call.name) {
            let updates = AgentToolUpdateSink {
                event_sink: self.event_sink,
                call,
            };
            match tool.execute(call, cancellation, &updates).await {
                Ok(output) => {
                    let mut result = ToolResultMessage::new(
                        call.id.clone(),
                        call.name.clone(),
                        output.content,
                        false,
                        self.clock.now_ms(),
                    );
                    result.details = output.details;
                    result
                }
                Err(error) => tool_error_result(call, error.message, self.clock.now_ms()),
            }
        } else {
            tool_error_result(
                call,
                format!("Unknown tool: {}", call.name),
                self.clock.now_ms(),
            )
        };
        (index, call.clone(), result)
    }

    async fn emit_tool_end(
        &self,
        call: &ToolCallBlock,
        result: &ToolResultMessage,
    ) -> Result<(), AgentRunError> {
        self.emit(AgentEvent::ToolExecutionEnd {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            result: serde_json::to_value(result).unwrap_or(Value::Null),
            is_error: result.is_error,
        })
        .await
    }

    async fn emit(&self, event: AgentEvent) -> Result<(), AgentRunError> {
        self.event_sink.emit(event).await.map_err(Into::into)
    }
}

struct AgentToolUpdateSink<'a> {
    event_sink: &'a dyn EventSink,
    call: &'a ToolCallBlock,
}

impl ToolUpdateSink for AgentToolUpdateSink<'_> {
    fn send<'a>(&'a self, partial_result: Value) -> ToolUpdateFuture<'a> {
        Box::pin(async move {
            self.event_sink
                .emit(AgentEvent::ToolExecutionUpdate {
                    tool_call_id: self.call.id.clone(),
                    tool_name: self.call.name.clone(),
                    args: self.call.arguments.clone(),
                    partial_result,
                })
                .await
                .map_err(|error| ToolError::execution(format!("event sink failed: {error}")))
        })
    }
}

enum StreamedAssistant {
    Completed(AssistantMessage),
    Failed {
        assistant: AssistantMessage,
        error: ModelServiceError,
    },
}

fn event_partial(event: &AssistantMessageEvent) -> Option<AssistantMessage> {
    match event {
        AssistantMessageEvent::TextStart { partial, .. }
        | AssistantMessageEvent::TextDelta { partial, .. }
        | AssistantMessageEvent::TextEnd { partial, .. }
        | AssistantMessageEvent::ThinkingStart { partial, .. }
        | AssistantMessageEvent::ThinkingDelta { partial, .. }
        | AssistantMessageEvent::ThinkingEnd { partial, .. }
        | AssistantMessageEvent::ToolCallStart { partial, .. }
        | AssistantMessageEvent::ToolCallDelta { partial, .. }
        | AssistantMessageEvent::ToolCallEnd { partial, .. } => Some(partial.clone()),
        AssistantMessageEvent::Start { .. }
        | AssistantMessageEvent::Done { .. }
        | AssistantMessageEvent::Error { .. } => None,
    }
}

fn tool_error_result(
    call: &ToolCallBlock,
    message: impl Into<String>,
    timestamp: u64,
) -> ToolResultMessage {
    ToolResultMessage::new(
        call.id.clone(),
        call.name.clone(),
        vec![ContentBlock::Text(TextBlock::new(message))],
        true,
        timestamp,
    )
}

fn add_usage(total: &mut Usage, usage: &Usage) {
    total.input = total.input.saturating_add(usage.input);
    total.output = total.output.saturating_add(usage.output);
    total.cache_read = total.cache_read.saturating_add(usage.cache_read);
    total.cache_write = total.cache_write.saturating_add(usage.cache_write);
    total.cache_write_1h = add_optional(total.cache_write_1h, usage.cache_write_1h);
    total.reasoning = add_optional(total.reasoning, usage.reasoning);
    total.total_tokens = total.total_tokens.saturating_add(usage.total_tokens);
    total.cost.input += usage.cost.input;
    total.cost.output += usage.cost.output;
    total.cost.cache_read += usage.cost.cache_read;
    total.cost.cache_write += usage.cost.cache_write;
    total.cost.total += usage.cost.total;
}

fn add_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0).saturating_add(right.unwrap_or(0))),
    }
}

const NON_VISION_IMAGE_NOTE: &str =
    "[Current model does not support images. The image will be omitted from this request.]";

fn prepare_messages_for_model(messages: &[Message], supports_images: bool) -> Vec<Message> {
    if supports_images {
        return messages.to_vec();
    }
    messages
        .iter()
        .cloned()
        .map(|message| match message {
            Message::User(mut message) => {
                if let MessageContent::Blocks(blocks) = &mut message.content {
                    omit_images(blocks);
                }
                Message::User(message)
            }
            Message::Assistant(mut message) => {
                omit_images(&mut message.content);
                Message::Assistant(message)
            }
            Message::ToolResult(mut message) => {
                omit_images(&mut message.content);
                Message::ToolResult(message)
            }
        })
        .collect()
}

fn omit_images(blocks: &mut Vec<ContentBlock>) {
    let image_count = blocks
        .iter()
        .filter(|block| matches!(block, ContentBlock::Image(_)))
        .count();
    if image_count == 0 {
        return;
    }
    blocks.retain(|block| !matches!(block, ContentBlock::Image(_)));
    blocks.push(ContentBlock::Text(TextBlock::new(NON_VISION_IMAGE_NOTE)));
}

/// Convenience constructor for an agent model request.
#[must_use]
pub fn model_request(
    model: pi_protocol::Model,
    system_prompt: impl Into<String>,
    messages: Vec<Message>,
) -> ModelRequest {
    ModelRequest {
        model,
        system_prompt: system_prompt.into(),
        messages,
        tools: Vec::new(),
        options: ModelRequestOptions::default(),
    }
}

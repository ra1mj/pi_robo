//! High-level agent retry and context-compaction runtime.

use pi_agent::{
    Agent, AgentRunError, AgentRunRequest, AgentRunResult, AgentRunStatus, Clock, EventFuture,
    EventSink, EventSinkError, Tool,
};
use pi_model::{Cancellation, ModelRequest, ModelService, ModelServiceErrorCategory};
use pi_protocol::{
    AgentEvent, ContentBlock, Message, MessageContent, StopReason, Usage, UserMessage,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use pi_agent::{ChannelEventSink, SystemClock, event_channel};

/// Runtime-level events layered around canonical agent events.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeEvent {
    Agent(AgentEvent),
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        delay: Duration,
        error_message: String,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
    CompactionStart {
        reason: CompactionReason,
    },
    CompactionEnd {
        reason: CompactionReason,
        result: Option<CompactionRecord>,
        aborted: bool,
        will_retry: bool,
        error_message: Option<String>,
    },
}

pub type RuntimeEventFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), RuntimeBoundaryError>> + Send + 'a>>;

pub trait RuntimeEventSink: Send + Sync {
    fn emit<'a>(&'a self, event: RuntimeEvent) -> RuntimeEventFuture<'a>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeBoundaryError {
    pub message: String,
}

impl RuntimeBoundaryError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RuntimeBoundaryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeBoundaryError {}

/// Retry settings applied above provider adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_retries: u32,
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            base_delay: Duration::from_secs(2),
        }
    }
}

/// Automatic compaction settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactionPolicy {
    pub enabled: bool,
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            reserve_tokens: 16_384,
            keep_recent_tokens: 20_000,
        }
    }
}

/// Retry and compaction policies applied by a runtime instance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimePolicies {
    pub retry: RetryPolicy,
    pub compaction: CompactionPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionReason {
    Threshold,
    Overflow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompactionRequest {
    pub reason: CompactionReason,
    pub messages: Vec<Message>,
    pub tokens_before: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompactionOutput {
    pub summary: String,
    pub usage: Usage,
}

pub type CompactionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CompactionOutput, RuntimeBoundaryError>> + Send + 'a>>;

pub trait Compactor: Send + Sync {
    fn compact<'a>(
        &'a self,
        request: CompactionRequest,
        cancellation: &'a dyn Cancellation,
    ) -> CompactionFuture<'a>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompactionRecord {
    pub reason: CompactionReason,
    pub summary: String,
    pub tokens_before: u64,
    pub estimated_tokens_after: u64,
    pub usage: Usage,
}

pub type SessionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), RuntimeBoundaryError>> + Send + 'a>>;

/// Abstract persistence boundary. Implementations may append to disk or retain records in memory.
pub trait SessionSink: Send + Sync {
    fn record_run<'a>(&'a self, run: AgentRunResult) -> SessionFuture<'a>;
    fn record_compaction<'a>(&'a self, record: CompactionRecord) -> SessionFuture<'a>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InMemorySessionSnapshot {
    pub runs: Vec<AgentRunResult>,
    pub compactions: Vec<CompactionRecord>,
}

#[derive(Debug, Default)]
pub struct InMemorySessionSink {
    state: Mutex<InMemorySessionSnapshot>,
}

impl InMemorySessionSink {
    pub fn snapshot(&self) -> Result<InMemorySessionSnapshot, RuntimeBoundaryError> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| RuntimeBoundaryError::new("in-memory session lock poisoned"))
    }
}

impl SessionSink for InMemorySessionSink {
    fn record_run<'a>(&'a self, run: AgentRunResult) -> SessionFuture<'a> {
        Box::pin(async move {
            self.state
                .lock()
                .map_err(|_| RuntimeBoundaryError::new("in-memory session lock poisoned"))?
                .runs
                .push(run);
            Ok(())
        })
    }

    fn record_compaction<'a>(&'a self, record: CompactionRecord) -> SessionFuture<'a> {
        Box::pin(async move {
            self.state
                .lock()
                .map_err(|_| RuntimeBoundaryError::new("in-memory session lock poisoned"))?
                .compactions
                .push(record);
            Ok(())
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepOutcome {
    Completed,
    Cancelled,
}

pub type SleepFuture<'a> = Pin<Box<dyn Future<Output = SleepOutcome> + Send + 'a>>;

pub trait Sleeper: Send + Sync {
    fn sleep<'a>(
        &'a self,
        duration: Duration,
        cancellation: &'a dyn Cancellation,
    ) -> SleepFuture<'a>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep<'a>(
        &'a self,
        duration: Duration,
        cancellation: &'a dyn Cancellation,
    ) -> SleepFuture<'a> {
        Box::pin(async move {
            tokio::select! {
                () = tokio::time::sleep(duration) => SleepOutcome::Completed,
                () = cancellation.cancelled() => SleepOutcome::Cancelled,
            }
        })
    }
}

pub struct RuntimeRequest {
    pub model_request: ModelRequest,
    pub prompt: Option<UserMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub parallel_tools: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeRunResult {
    pub messages: Vec<Message>,
    pub usage: Usage,
    pub status: RuntimeStatus,
    pub retry_attempts: u32,
    pub compactions: Vec<CompactionRecord>,
}

#[derive(Debug)]
pub enum RuntimeError {
    Agent(AgentRunError),
    Boundary(RuntimeBoundaryError),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Agent(error) => write!(formatter, "agent run failed: {error}"),
            Self::Boundary(error) => write!(formatter, "runtime boundary failed: {error}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<AgentRunError> for RuntimeError {
    fn from(error: AgentRunError) -> Self {
        Self::Agent(error)
    }
}

impl From<RuntimeBoundaryError> for RuntimeError {
    fn from(error: RuntimeBoundaryError) -> Self {
        Self::Boundary(error)
    }
}

/// Provider-neutral high-level runtime.
pub struct Runtime<'a> {
    model_service: &'a dyn ModelService,
    event_sink: &'a dyn RuntimeEventSink,
    session_sink: &'a dyn SessionSink,
    compactor: &'a dyn Compactor,
    sleeper: &'a dyn Sleeper,
    clock: &'a dyn Clock,
    retry_policy: RetryPolicy,
    compaction_policy: CompactionPolicy,
}

impl<'a> Runtime<'a> {
    #[must_use]
    pub const fn new(
        model_service: &'a dyn ModelService,
        event_sink: &'a dyn RuntimeEventSink,
        session_sink: &'a dyn SessionSink,
        compactor: &'a dyn Compactor,
        sleeper: &'a dyn Sleeper,
        clock: &'a dyn Clock,
        policies: RuntimePolicies,
    ) -> Self {
        Self {
            model_service,
            event_sink,
            session_sink,
            compactor,
            sleeper,
            clock,
            retry_policy: policies.retry,
            compaction_policy: policies.compaction,
        }
    }

    pub async fn run(
        &self,
        mut request: RuntimeRequest,
        cancellation: &dyn Cancellation,
    ) -> Result<RuntimeRunResult, RuntimeError> {
        let agent_sink = RuntimeAgentEventSink {
            inner: self.event_sink,
        };
        let agent = Agent::new(self.model_service, &agent_sink, self.clock);
        let mut total_usage = Usage::default();
        let mut retry_attempts = 0;
        let mut retry_in_progress = false;
        let mut overflow_recovery_attempted = false;
        let mut compactions = Vec::new();
        let mut prompt = request.prompt.take();
        let mut history = std::mem::take(&mut request.model_request.messages);

        loop {
            request.model_request.messages.clone_from(&history);
            let run = agent
                .run(
                    AgentRunRequest {
                        model_request: request.model_request.clone(),
                        prompt: prompt.take(),
                        tools: request.tools.clone(),
                        parallel_tools: request.parallel_tools,
                    },
                    cancellation,
                )
                .await?;
            add_usage(&mut total_usage, &run.usage);
            self.session_sink.record_run(run.clone()).await?;
            history.clone_from(&run.messages);

            match &run.status {
                AgentRunStatus::Completed => {
                    if retry_in_progress {
                        self.emit(RuntimeEvent::AutoRetryEnd {
                            success: true,
                            attempt: retry_attempts,
                            final_error: None,
                        })
                        .await?;
                    }
                    if self.should_compact(&history, request.model_request.model.context_window) {
                        if let Some(record) = self
                            .compact_history(
                                CompactionReason::Threshold,
                                false,
                                &mut history,
                                cancellation,
                            )
                            .await?
                        {
                            add_usage(&mut total_usage, &record.usage);
                            compactions.push(record);
                        }
                        if cancellation.is_cancelled() {
                            return Ok(RuntimeRunResult {
                                messages: history,
                                usage: total_usage,
                                status: RuntimeStatus::Cancelled,
                                retry_attempts,
                                compactions,
                            });
                        }
                    }
                    return Ok(RuntimeRunResult {
                        messages: history,
                        usage: total_usage,
                        status: RuntimeStatus::Completed,
                        retry_attempts,
                        compactions,
                    });
                }
                AgentRunStatus::Cancelled => {
                    if retry_in_progress {
                        self.emit(RuntimeEvent::AutoRetryEnd {
                            success: false,
                            attempt: retry_attempts,
                            final_error: Some("Retry cancelled".to_owned()),
                        })
                        .await?;
                    }
                    return Ok(RuntimeRunResult {
                        messages: history,
                        usage: total_usage,
                        status: RuntimeStatus::Cancelled,
                        retry_attempts,
                        compactions,
                    });
                }
                AgentRunStatus::Failed(error)
                    if error.category == ModelServiceErrorCategory::ContextOverflow =>
                {
                    if overflow_recovery_attempted || !self.compaction_policy.enabled {
                        self.emit(RuntimeEvent::CompactionEnd {
                            reason: CompactionReason::Overflow,
                            result: None,
                            aborted: false,
                            will_retry: false,
                            error_message: Some(
                                "Context overflow recovery failed after one compact-and-retry attempt"
                                    .to_owned(),
                            ),
                        })
                        .await?;
                        return Ok(failed_result(
                            history,
                            total_usage,
                            retry_attempts,
                            compactions,
                        ));
                    }
                    overflow_recovery_attempted = true;
                    remove_terminal_error(&mut history);
                    let Some(record) = self
                        .compact_history(
                            CompactionReason::Overflow,
                            true,
                            &mut history,
                            cancellation,
                        )
                        .await?
                    else {
                        if cancellation.is_cancelled() {
                            return Ok(RuntimeRunResult {
                                messages: history,
                                usage: total_usage,
                                status: RuntimeStatus::Cancelled,
                                retry_attempts,
                                compactions,
                            });
                        }
                        return Ok(failed_result(
                            history,
                            total_usage,
                            retry_attempts,
                            compactions,
                        ));
                    };
                    add_usage(&mut total_usage, &record.usage);
                    compactions.push(record);
                }
                AgentRunStatus::Failed(error)
                    if self.retry_policy.enabled
                        && error.retryable
                        && retry_attempts < self.retry_policy.max_retries =>
                {
                    retry_attempts += 1;
                    retry_in_progress = true;
                    remove_terminal_error(&mut history);
                    let delay = error.retry_after_ms.map_or_else(
                        || exponential_delay(self.retry_policy.base_delay, retry_attempts),
                        Duration::from_millis,
                    );
                    self.emit(RuntimeEvent::AutoRetryStart {
                        attempt: retry_attempts,
                        max_attempts: self.retry_policy.max_retries,
                        delay,
                        error_message: error.message.clone(),
                    })
                    .await?;
                    if self.sleeper.sleep(delay, cancellation).await == SleepOutcome::Cancelled {
                        self.emit(RuntimeEvent::AutoRetryEnd {
                            success: false,
                            attempt: retry_attempts,
                            final_error: Some("Retry cancelled".to_owned()),
                        })
                        .await?;
                        return Ok(RuntimeRunResult {
                            messages: history,
                            usage: total_usage,
                            status: RuntimeStatus::Cancelled,
                            retry_attempts,
                            compactions,
                        });
                    }
                }
                AgentRunStatus::Failed(error) => {
                    if retry_in_progress {
                        self.emit(RuntimeEvent::AutoRetryEnd {
                            success: false,
                            attempt: retry_attempts,
                            final_error: Some(error.message.clone()),
                        })
                        .await?;
                    }
                    return Ok(failed_result(
                        history,
                        total_usage,
                        retry_attempts,
                        compactions,
                    ));
                }
            }
        }
    }

    async fn compact_history(
        &self,
        reason: CompactionReason,
        will_retry: bool,
        history: &mut Vec<Message>,
        cancellation: &dyn Cancellation,
    ) -> Result<Option<CompactionRecord>, RuntimeError> {
        self.emit(RuntimeEvent::CompactionStart { reason }).await?;
        let tokens_before = estimate_messages_tokens(history);
        let output = match self
            .compactor
            .compact(
                CompactionRequest {
                    reason,
                    messages: history.clone(),
                    tokens_before,
                },
                cancellation,
            )
            .await
        {
            Ok(output) => output,
            Err(error) => {
                let aborted = cancellation.is_cancelled();
                self.emit(RuntimeEvent::CompactionEnd {
                    reason,
                    result: None,
                    aborted,
                    will_retry: false,
                    error_message: Some(error.message),
                })
                .await?;
                return Ok(None);
            }
        };
        if cancellation.is_cancelled() {
            self.emit(RuntimeEvent::CompactionEnd {
                reason,
                result: None,
                aborted: true,
                will_retry: false,
                error_message: None,
            })
            .await?;
            return Ok(None);
        }
        let kept = keep_recent_messages(history, self.compaction_policy.keep_recent_tokens);
        let summary_message = UserMessage::new(
            MessageContent::Text(format!("Context summary:\n{}", output.summary)),
            self.clock.now_ms(),
        );
        *history = std::iter::once(Message::User(summary_message))
            .chain(kept)
            .collect();
        let record = CompactionRecord {
            reason,
            summary: output.summary,
            tokens_before,
            estimated_tokens_after: estimate_messages_tokens(history),
            usage: output.usage,
        };
        self.session_sink.record_compaction(record.clone()).await?;
        self.emit(RuntimeEvent::CompactionEnd {
            reason,
            result: Some(record.clone()),
            aborted: false,
            will_retry,
            error_message: None,
        })
        .await?;
        Ok(Some(record))
    }

    fn should_compact(&self, messages: &[Message], context_window: u64) -> bool {
        self.compaction_policy.enabled
            && estimate_context_tokens(messages)
                > context_window.saturating_sub(self.compaction_policy.reserve_tokens)
    }

    async fn emit(&self, event: RuntimeEvent) -> Result<(), RuntimeError> {
        self.event_sink.emit(event).await.map_err(Into::into)
    }
}

struct RuntimeAgentEventSink<'a> {
    inner: &'a dyn RuntimeEventSink,
}

impl EventSink for RuntimeAgentEventSink<'_> {
    fn emit<'a>(&'a self, event: AgentEvent) -> EventFuture<'a> {
        Box::pin(async move {
            self.inner
                .emit(RuntimeEvent::Agent(event))
                .await
                .map_err(|error| EventSinkError::new(error.message))
        })
    }
}

fn failed_result(
    messages: Vec<Message>,
    usage: Usage,
    retry_attempts: u32,
    compactions: Vec<CompactionRecord>,
) -> RuntimeRunResult {
    RuntimeRunResult {
        messages,
        usage,
        status: RuntimeStatus::Failed,
        retry_attempts,
        compactions,
    }
}

fn remove_terminal_error(messages: &mut Vec<Message>) {
    if matches!(
        messages.last(),
        Some(Message::Assistant(message))
            if matches!(message.stop_reason, StopReason::Error | StopReason::Aborted)
    ) {
        messages.pop();
    }
}

fn exponential_delay(base: Duration, attempt: u32) -> Duration {
    base.saturating_mul(2_u32.saturating_pow(attempt.saturating_sub(1)))
}

fn estimate_context_tokens(messages: &[Message]) -> u64 {
    messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::Assistant(message)
                if !matches!(message.stop_reason, StopReason::Error | StopReason::Aborted)
                    && message.usage.total_tokens > 0 =>
            {
                Some(message.usage.total_tokens)
            }
            _ => None,
        })
        .unwrap_or_else(|| estimate_messages_tokens(messages))
}

fn estimate_messages_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_message_tokens(message: &Message) -> u64 {
    let characters = match message {
        Message::User(message) => estimate_message_content(&message.content),
        Message::Assistant(message) => message
            .content
            .iter()
            .map(estimate_content_block)
            .sum::<usize>(),
        Message::ToolResult(message) => message
            .content
            .iter()
            .map(estimate_content_block)
            .sum::<usize>(),
    };
    u64::try_from(characters.div_ceil(4)).unwrap_or(u64::MAX)
}

fn estimate_message_content(content: &MessageContent) -> usize {
    match content {
        MessageContent::Text(text) => text.chars().count(),
        MessageContent::Blocks(blocks) => blocks.iter().map(estimate_content_block).sum(),
    }
}

fn estimate_content_block(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text(block) => block.text.chars().count(),
        ContentBlock::Thinking(block) => block.thinking.chars().count(),
        ContentBlock::Image(_) => 4_800,
        ContentBlock::ToolCall(block) => {
            block.name.chars().count() + block.arguments.to_string().chars().count()
        }
    }
}

fn keep_recent_messages(messages: &[Message], budget: u64) -> Vec<Message> {
    if budget == 0 {
        return Vec::new();
    }
    let mut tokens = 0_u64;
    let mut start = messages.len();
    for (index, message) in messages.iter().enumerate().rev() {
        tokens = tokens.saturating_add(estimate_message_tokens(message));
        start = index;
        if tokens >= budget {
            break;
        }
    }
    while start > 0 && matches!(messages.get(start), Some(Message::ToolResult(_))) {
        start -= 1;
    }
    messages[start..].to_vec()
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

//! Model selection and provider-service contracts without a runtime dependency.

use futures_core::Stream;
use pi_protocol::{
    AssistantMessageEvent, CompletionReason, Message, Model, StopReason, ToolDefinition,
};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Future resolved when a cooperative cancellation request is made.
pub type CancellationFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Cooperative cancellation boundary supplied by the runtime.
pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;

    /// Waits without polling until cancellation is requested.
    fn cancelled(&self) -> CancellationFuture<'_>;
}

/// Provider-independent reasoning effort levels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThinkingLevel {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

/// Optional per-level token budgets for providers that support explicit thinking limits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ThinkingBudgets {
    pub minimal: Option<u64>,
    pub low: Option<u64>,
    pub medium: Option<u64>,
    pub high: Option<u64>,
}

/// Provider-independent prompt-cache retention intent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CacheRetention {
    None,
    #[default]
    Short,
    Long,
}

/// Provider-independent tool-selection intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}

/// Optional controls applied to one model request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelRequestOptions {
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub reasoning: Option<ThinkingLevel>,
    pub thinking_budgets: Option<ThinkingBudgets>,
    pub cache_retention: CacheRetention,
    pub session_id: Option<String>,
    pub tool_choice: Option<ToolChoice>,
}

impl ModelRequestOptions {
    /// Rejects values that no provider can represent safely.
    pub fn validate(&self) -> Result<(), ModelServiceError> {
        if self.temperature.is_some_and(|value| !value.is_finite()) {
            return Err(ModelServiceError::new(
                ModelServiceErrorCategory::Configuration,
                "temperature must be finite",
                false,
            ));
        }
        Ok(())
    }
}

/// One model request after model selection and prompt construction.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRequest {
    pub model: Model,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub options: ModelRequestOptions,
}

/// Stable failure categories shared by every provider adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelServiceErrorCategory {
    Configuration,
    Authentication,
    Permission,
    InvalidRequest,
    NotFound,
    ContextOverflow,
    RateLimit,
    QuotaExceeded,
    Timeout,
    Network,
    Unavailable,
    Server,
    Protocol,
    Cancelled,
    Unknown,
}

/// Provider-independent service failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelServiceError {
    pub category: ModelServiceErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub http_status: Option<u16>,
    pub provider_code: Option<String>,
    pub retry_after_ms: Option<u64>,
}

impl ModelServiceError {
    #[must_use]
    pub fn new(
        category: ModelServiceErrorCategory,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            category,
            message: message.into(),
            retryable,
            http_status: None,
            provider_code: None,
            retry_after_ms: None,
        }
    }

    #[must_use]
    pub fn with_http_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    #[must_use]
    pub fn with_provider_code(mut self, code: impl Into<String>) -> Self {
        self.provider_code = Some(code.into());
        self
    }

    #[must_use]
    pub const fn with_retry_after_ms(mut self, retry_after_ms: u64) -> Self {
        self.retry_after_ms = Some(retry_after_ms);
        self
    }

    #[must_use]
    pub fn protocol(message: impl Into<String>, retryable: bool) -> Self {
        Self::new(ModelServiceErrorCategory::Protocol, message, retryable)
    }

    #[must_use]
    pub fn cancelled() -> Self {
        Self::new(
            ModelServiceErrorCategory::Cancelled,
            "request cancelled",
            false,
        )
    }
}

impl std::fmt::Display for ModelServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ModelServiceError {}

type BoxedModelEventStream<'a> =
    Pin<Box<dyn Stream<Item = Result<AssistantMessageEvent, ModelServiceError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModelStreamPhase {
    AwaitingStart,
    Streaming,
    Terminated,
}

/// Validated provider event stream with one start and one terminal outcome.
pub struct ModelEventStream<'a> {
    inner: BoxedModelEventStream<'a>,
    phase: ModelStreamPhase,
}

impl<'a> ModelEventStream<'a> {
    #[must_use]
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<AssistantMessageEvent, ModelServiceError>> + Send + 'a,
    {
        Self {
            inner: Box::pin(stream),
            phase: ModelStreamPhase::AwaitingStart,
        }
    }

    fn terminate_with_protocol_error(
        &mut self,
        message: &'static str,
        retryable: bool,
    ) -> Poll<Option<Result<AssistantMessageEvent, ModelServiceError>>> {
        self.phase = ModelStreamPhase::Terminated;
        Poll::Ready(Some(Err(ModelServiceError::protocol(message, retryable))))
    }
}

impl Stream for ModelEventStream<'_> {
    type Item = Result<AssistantMessageEvent, ModelServiceError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.phase == ModelStreamPhase::Terminated {
            return Poll::Ready(None);
        }

        match self.inner.as_mut().poll_next(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => self.terminate_with_protocol_error(
                "model event stream ended without a terminal outcome",
                true,
            ),
            Poll::Ready(Some(Err(error))) => {
                if self.phase == ModelStreamPhase::AwaitingStart {
                    self.terminate_with_protocol_error(
                        "model event stream failed before the start event",
                        false,
                    )
                } else {
                    self.phase = ModelStreamPhase::Terminated;
                    Poll::Ready(Some(Err(error)))
                }
            }
            Poll::Ready(Some(Ok(event))) => match (&self.phase, &event) {
                (ModelStreamPhase::AwaitingStart, AssistantMessageEvent::Start { .. }) => {
                    self.phase = ModelStreamPhase::Streaming;
                    Poll::Ready(Some(Ok(event)))
                }
                (ModelStreamPhase::AwaitingStart, _) => self.terminate_with_protocol_error(
                    "model event stream did not begin with a start event",
                    false,
                ),
                (ModelStreamPhase::Streaming, AssistantMessageEvent::Start { .. }) => self
                    .terminate_with_protocol_error(
                        "model event stream emitted more than one start event",
                        false,
                    ),
                (ModelStreamPhase::Streaming, AssistantMessageEvent::Error { .. }) => self
                    .terminate_with_protocol_error(
                        "provider adapters must report stream failures as service errors",
                        false,
                    ),
                (ModelStreamPhase::Streaming, AssistantMessageEvent::Done { reason, message }) => {
                    let matching_reason = matches!(
                        (reason, message.stop_reason),
                        (CompletionReason::Stop, StopReason::Stop)
                            | (CompletionReason::Length, StopReason::Length)
                            | (CompletionReason::ToolUse, StopReason::ToolUse)
                    );
                    if matching_reason {
                        self.phase = ModelStreamPhase::Terminated;
                        Poll::Ready(Some(Ok(event)))
                    } else {
                        self.terminate_with_protocol_error(
                            "done reason does not match the assistant message stop reason",
                            false,
                        )
                    }
                }
                (ModelStreamPhase::Streaming, _) => Poll::Ready(Some(Ok(event))),
                (ModelStreamPhase::Terminated, _) => Poll::Ready(None),
            },
        }
    }
}

/// Boxed future returned by an object-safe model service.
pub type ModelFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ModelEventStream<'a>, ModelServiceError>> + Send + 'a>>;

/// Provider/model service boundary used by later runtime milestones.
pub trait ModelService: Send + Sync {
    /// Returns events in provider order and preserves exactly one terminal outcome.
    /// Implementations must observe cancellation before dispatch and while streaming.
    /// Pre-stream failures are returned by the future; stream failures are returned as items.
    fn stream<'a>(
        &'a self,
        request: ModelRequest,
        cancellation: &'a dyn Cancellation,
    ) -> ModelFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::{ModelRequestOptions, ModelService, ModelServiceErrorCategory};

    fn accept_object_safe(_: &dyn ModelService) {}

    #[test]
    fn service_is_object_safe() {
        let _ = accept_object_safe;
    }

    #[test]
    fn request_options_reject_non_finite_temperature() {
        let options = ModelRequestOptions {
            temperature: Some(f64::NAN),
            ..ModelRequestOptions::default()
        };
        let error = options.validate().expect_err("NaN must be rejected");
        assert_eq!(error.category, ModelServiceErrorCategory::Configuration);
    }
}

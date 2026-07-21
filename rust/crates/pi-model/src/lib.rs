//! Model selection and provider-service contracts without a runtime dependency.

use futures_core::Stream;
use pi_protocol::{AssistantMessageEvent, Message, Model, ToolDefinition};
use std::future::Future;
use std::pin::Pin;

/// Cooperative cancellation boundary supplied by the runtime.
pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

/// One model request after model selection and prompt construction.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRequest {
    pub model: Model,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}

/// Provider-independent service failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelServiceError {
    pub category: String,
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for ModelServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ModelServiceError {}

/// Boxed stream keeps provider implementations runtime-agnostic and object-safe.
pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<AssistantMessageEvent, ModelServiceError>> + Send>>;

/// Boxed future returned by an object-safe model service.
pub type ModelFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ModelEventStream, ModelServiceError>> + Send + 'a>>;

/// Provider/model service boundary used by later runtime milestones.
pub trait ModelService: Send + Sync {
    /// Returns events in provider order and preserves exactly one terminal event.
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
    use super::ModelService;

    fn accept_object_safe(_: &dyn ModelService) {}

    #[test]
    fn service_is_object_safe() {
        let _ = accept_object_safe;
    }
}

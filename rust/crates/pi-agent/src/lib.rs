//! Agent and tool execution contracts without orchestration behavior.

use pi_model::Cancellation;
use pi_protocol::ToolCallBlock;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// Result returned by a tool implementation.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutput {
    pub content: Value,
    pub details: Option<Value>,
}

/// Object-safe future returned by a tool.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

/// Structured tool failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolError {
    pub message: String,
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

/// Tool execution boundary used by the runtime.
pub trait Tool: Send + Sync {
    fn execute<'a>(
        &'a self,
        call: &'a ToolCallBlock,
        cancellation: &'a dyn Cancellation,
    ) -> ToolFuture<'a>;
}

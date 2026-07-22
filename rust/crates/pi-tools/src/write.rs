use crate::{MutationCoordinator, resolve_path};
use pi_agent::{Cancellation, Tool, ToolError, ToolFuture, ToolOutput, ToolUpdateSink};
use pi_protocol::{Extensions, ToolCallBlock, ToolDefinition};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct WriteTool {
    cwd: PathBuf,
    mutations: MutationCoordinator,
}

impl WriteTool {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>, mutations: MutationCoordinator) -> Self {
        Self {
            cwd: cwd.into(),
            mutations,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
}

impl Tool for WriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write".to_owned(),
            description: "Write complete UTF-8 content to a file, creating parent directories."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            extensions: Extensions::new(),
        }
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCallBlock,
        cancellation: &'a dyn Cancellation,
        _updates: &'a dyn ToolUpdateSink,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: WriteArgs =
                serde_json::from_value(call.arguments.clone()).map_err(|error| {
                    ToolError::invalid_arguments(format!("invalid write arguments: {error}"))
                })?;
            let path = resolve_path(&args.path, &self.cwd)?;
            let _lease = self.mutations.acquire(&path, cancellation).await?;
            if cancellation.is_cancelled() {
                return Err(ToolError::cancelled());
            }
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|error| {
                    ToolError::execution(format!(
                        "failed to create parent directory {}: {error}",
                        parent.display()
                    ))
                })?;
            }
            if cancellation.is_cancelled() {
                return Err(ToolError::cancelled());
            }
            tokio::fs::write(&path, args.content.as_bytes())
                .await
                .map_err(|error| {
                    ToolError::execution(format!("failed to write {}: {error}", path.display()))
                })?;
            if cancellation.is_cancelled() {
                return Err(ToolError::cancelled());
            }
            let javascript_length = args.content.encode_utf16().count();
            Ok(ToolOutput::text(format!(
                "Successfully wrote {javascript_length} bytes to {}",
                args.path
            )))
        })
    }
}

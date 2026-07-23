use crate::CliArgs;
use pi_agent::{Tool, ToolError, ToolErrorCategory, ToolUpdateFuture, ToolUpdateSink};
use pi_model::Cancellation;
use pi_protocol::{ContentBlock, ImageBlock, ToolCallBlock};
use pi_store::{StoreError, StoreErrorCategory, StorePaths};
use pi_tools::{ImagePolicy, ReadTool, detect_supported_image_mime_type};
use serde_json::json;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedPrompt {
    pub text: String,
    pub images: Vec<ImageBlock>,
}

pub async fn prepare_prompts(
    args: &CliArgs,
    paths: &StorePaths,
    stdin: Option<&str>,
    image_policy: ImagePolicy,
    cancellation: &dyn Cancellation,
) -> Result<Vec<PreparedPrompt>, StoreError> {
    let mut file_text = String::new();
    let mut images = Vec::new();
    let reader = ReadTool::new(&paths.cwd, image_policy);
    let updates = IgnoreUpdates;
    for file in &args.files {
        let path = paths.resolve_user_path(file)?;
        let bytes = std::fs::read(&path).map_err(|error| StoreError::io(error, &path))?;
        if bytes.is_empty() {
            continue;
        }
        if detect_supported_image_mime_type(&bytes).is_some() {
            let call = ToolCallBlock::new(
                "cli-input",
                "read",
                json!({ "path": path.display().to_string() }),
            );
            let output = reader
                .execute(&call, cancellation, &updates)
                .await
                .map_err(|error| tool_store_error(error, &path))?;
            let mut notes = Vec::new();
            for block in output.content {
                match block {
                    ContentBlock::Text(text) => notes.push(text.text),
                    ContentBlock::Image(image) => images.push(image),
                    ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) => {}
                }
            }
            file_text.push_str(&format!(
                "<file name=\"{}\">{}</file>\n",
                path.display(),
                notes.join("\n")
            ));
            continue;
        }
        let content = String::from_utf8(bytes).map_err(|_| {
            StoreError::new(
                StoreErrorCategory::InvalidShape,
                format!(
                    "file is neither supported image data nor valid UTF-8 text: {}",
                    path.display()
                ),
            )
            .with_path(&path)
        })?;
        file_text.push_str(&format!(
            "<file name=\"{}\">\n{content}\n</file>\n",
            path.display()
        ));
    }

    let mut prompts = Vec::new();
    let mut initial = String::new();
    if let Some(stdin) = stdin.map(str::trim).filter(|value| !value.is_empty()) {
        initial.push_str(stdin);
    }
    initial.push_str(&file_text);
    if let Some(message) = args.messages.first() {
        initial.push_str(message);
    }
    if !initial.is_empty() || !images.is_empty() {
        prompts.push(PreparedPrompt {
            text: initial,
            images,
        });
    }
    prompts.extend(args.messages.iter().skip(1).map(|message| PreparedPrompt {
        text: message.clone(),
        images: Vec::new(),
    }));
    if prompts.is_empty() {
        return Err(StoreError::new(
            StoreErrorCategory::InvalidShape,
            "headless mode requires a prompt from an argument, stdin, or @file",
        ));
    }
    Ok(prompts)
}

fn tool_store_error(error: ToolError, path: &Path) -> StoreError {
    let category = match error.category {
        ToolErrorCategory::Cancelled => StoreErrorCategory::Cancelled,
        ToolErrorCategory::InvalidArguments => StoreErrorCategory::InvalidShape,
        ToolErrorCategory::Execution => StoreErrorCategory::Io,
    };
    StoreError::new(category, error.message).with_path(path)
}

struct IgnoreUpdates;

impl ToolUpdateSink for IgnoreUpdates {
    fn send<'a>(&'a self, _partial_result: serde_json::Value) -> ToolUpdateFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

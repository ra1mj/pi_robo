use crate::image_processing::{ImagePolicy, detect_supported_image_mime_type, process_image};
use crate::path::resolve_path;
use crate::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, truncate_head};
use pi_agent::{
    Cancellation, Tool, ToolError, ToolErrorCategory, ToolFuture, ToolOutput, ToolUpdateSink,
};
use pi_protocol::{ContentBlock, Extensions, ImageBlock, TextBlock, ToolCallBlock, ToolDefinition};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ReadTool {
    cwd: PathBuf,
    image_policy: ImagePolicy,
}

impl ReadTool {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>, image_policy: ImagePolicy) -> Self {
        Self {
            cwd: cwd.into(),
            image_policy,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
    offset: Option<u64>,
    limit: Option<u64>,
}

impl Tool for ReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read".to_owned(),
            description: format!(
                "Read a text or image file. Text output is limited to {DEFAULT_MAX_LINES} lines or {}KB.",
                DEFAULT_MAX_BYTES / 1_024
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "number" },
                    "limit": { "type": "number" }
                },
                "required": ["path"],
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
            let args: ReadArgs =
                serde_json::from_value(call.arguments.clone()).map_err(|error| {
                    ToolError::invalid_arguments(format!("invalid read arguments: {error}"))
                })?;
            let path = resolve_path(&args.path, &self.cwd)?;
            if cancellation.is_cancelled() {
                return Err(ToolError::cancelled());
            }
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|error| file_error("read", &path, error))?;
            if cancellation.is_cancelled() {
                return Err(ToolError::cancelled());
            }
            if let Some(mime_type) = detect_supported_image_mime_type(&bytes) {
                return self.read_image(bytes, mime_type, cancellation).await;
            }
            read_text(&bytes, &path, &args)
        })
    }
}

impl ReadTool {
    async fn read_image(
        &self,
        bytes: Vec<u8>,
        mime_type: &str,
        cancellation: &dyn Cancellation,
    ) -> Result<ToolOutput, ToolError> {
        if self.image_policy.block_images {
            return Ok(ToolOutput::text(format!(
                "Read image file [{mime_type}]\n[Image omitted because image output is disabled.]"
            )));
        }
        match process_image(bytes, mime_type, self.image_policy.clone(), cancellation).await {
            Ok(Some(image)) => {
                let mut note = format!("Read image file [{}]", image.mime_type);
                if let Some(hint) = image.hint {
                    note.push('\n');
                    note.push_str(&hint);
                }
                Ok(ToolOutput {
                    content: vec![
                        ContentBlock::Text(TextBlock::new(note)),
                        ContentBlock::Image(ImageBlock::new(image.data, image.mime_type)),
                    ],
                    details: None,
                })
            }
            Ok(None) => Ok(ToolOutput::text(format!(
                "Read image file [{mime_type}]\n[Image omitted: could not be resized below the inline image size limit.]"
            ))),
            Err(error) if error.category == ToolErrorCategory::Cancelled => Err(error),
            Err(error) => Ok(ToolOutput::text(format!(
                "Read image file [{mime_type}]\n[Image omitted: {}]",
                error.message
            ))),
        }
    }
}

fn read_text(bytes: &[u8], path: &Path, args: &ReadArgs) -> Result<ToolOutput, ToolError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        ToolError::execution(format!("{} is not valid UTF-8: {error}", path.display()))
    })?;
    let lines = text.split('\n').collect::<Vec<_>>();
    let total_lines = lines.len();
    let offset = args.offset.unwrap_or(1).max(1);
    let start = usize::try_from(offset - 1)
        .map_err(|_| ToolError::invalid_arguments("offset is too large"))?;
    if start >= total_lines {
        return Err(ToolError::execution(format!(
            "Offset {offset} is beyond end of file ({total_lines} lines total)"
        )));
    }
    let end = match args.limit {
        Some(limit) => start.saturating_add(
            usize::try_from(limit)
                .map_err(|_| ToolError::invalid_arguments("limit is too large"))?,
        ),
        None => total_lines,
    }
    .min(total_lines);
    let selected = lines[start..end].join("\n");
    let truncation = truncate_head(&selected, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
    let mut output = if truncation.first_line_exceeds_limit {
        format!(
            "[Line {} is {} bytes, exceeds {DEFAULT_MAX_BYTES} byte limit. Use bash to read a bounded prefix.]",
            start + 1,
            lines[start].len()
        )
    } else {
        truncation.content.clone()
    };
    if truncation.truncated && !truncation.first_line_exceeds_limit {
        let first = start + 1;
        let last = first + truncation.output_lines.saturating_sub(1);
        let next = last + 1;
        match truncation.truncated_by {
            Some(TruncatedBy::Lines) => output.push_str(&format!(
                "\n\n[Showing lines {first}-{last} of {total_lines}. Use offset={next} to continue.]"
            )),
            Some(TruncatedBy::Bytes) => output.push_str(&format!(
                "\n\n[Showing lines {first}-{last} of {total_lines} ({DEFAULT_MAX_BYTES} byte limit). Use offset={next} to continue.]"
            )),
            None => {}
        }
    } else if args.limit.is_some() && end < total_lines {
        output.push_str(&format!(
            "\n\n[{} more lines in file. Use offset={} to continue.]",
            total_lines - end,
            end + 1
        ));
    }
    let details = truncation.truncated.then(|| {
        json!({
            "truncation": {
                "truncated": true,
                "truncatedBy": match truncation.truncated_by { Some(TruncatedBy::Lines) => "lines", Some(TruncatedBy::Bytes) => "bytes", None => "none" },
                "totalLines": truncation.total_lines,
                "totalBytes": truncation.total_bytes,
                "outputLines": truncation.output_lines,
                "outputBytes": truncation.output_bytes,
                "firstLineExceedsLimit": truncation.first_line_exceeds_limit,
                "maxLines": truncation.max_lines,
                "maxBytes": truncation.max_bytes
            }
        })
    });
    Ok(ToolOutput {
        content: vec![ContentBlock::Text(TextBlock::new(output))],
        details,
    })
}

fn file_error(operation: &str, path: &Path, error: std::io::Error) -> ToolError {
    ToolError::execution(format!("failed to {operation} {}: {error}", path.display()))
}

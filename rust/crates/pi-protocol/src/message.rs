use crate::Extensions;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextBlockKind {
    #[serde(rename = "text")]
    Text,
}

/// Plain text content.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBlock {
    #[serde(rename = "type")]
    kind: TextBlockKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_signature: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl TextBlock {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: TextBlockKind::Text,
            text: text.into(),
            text_signature: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ThinkingBlockKind {
    #[serde(rename = "thinking")]
    Thinking,
}

/// Provider reasoning content.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingBlock {
    #[serde(rename = "type")]
    kind: ThinkingBlockKind,
    pub thinking: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ThinkingBlock {
    #[must_use]
    pub fn new(thinking: impl Into<String>) -> Self {
        Self {
            kind: ThinkingBlockKind::Thinking,
            thinking: thinking.into(),
            thinking_signature: None,
            redacted: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ImageBlockKind {
    #[serde(rename = "image")]
    Image,
}

/// Base64-encoded image content.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageBlock {
    #[serde(rename = "type")]
    kind: ImageBlockKind,
    pub data: String,
    pub mime_type: String,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ImageBlock {
    #[must_use]
    pub fn new(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            kind: ImageBlockKind::Image,
            data: data.into(),
            mime_type: mime_type.into(),
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ToolCallBlockKind {
    #[serde(rename = "toolCall")]
    ToolCall,
}

/// A model-requested tool invocation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallBlock {
    #[serde(rename = "type")]
    kind: ToolCallBlockKind,
    pub id: String,
    pub name: String,
    pub arguments: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ToolCallBlock {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            kind: ToolCallBlockKind::ToolCall,
            id: id.into(),
            name: name.into(),
            arguments,
            thought_signature: None,
            extensions: Extensions::new(),
        }
    }
}

/// Canonical content union used by messages and tool results.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ContentBlock {
    Text(TextBlock),
    Thinking(ThinkingBlock),
    Image(ImageBlock),
    ToolCall(ToolCallBlock),
}

/// User input can be a string or a typed content list.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum UserRole {
    #[serde(rename = "user")]
    User,
}

/// Canonical user message.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct UserMessage {
    role: UserRole,
    pub content: MessageContent,
    pub timestamp: u64,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl UserMessage {
    #[must_use]
    pub fn new(content: MessageContent, timestamp: u64) -> Self {
        Self {
            role: UserRole::User,
            content,
            timestamp,
            extensions: Extensions::new(),
        }
    }
}

/// Why an assistant response ended.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
}

/// Monetary usage totals in provider billing units.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

/// Token and cost accounting attached to assistant messages.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_1h: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
    pub total_tokens: u64,
    pub cost: UsageCost,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum AssistantRole {
    #[serde(rename = "assistant")]
    Assistant,
}

/// Canonical completed or partial assistant message.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    role: AssistantRole,
    pub content: Vec<ContentBlock>,
    pub api: String,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    pub usage: Usage,
    pub stop_reason: StopReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Value>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl AssistantMessage {
    #[must_use]
    pub fn new(
        content: Vec<ContentBlock>,
        api: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        usage: Usage,
        stop_reason: StopReason,
        timestamp: u64,
    ) -> Self {
        Self {
            role: AssistantRole::Assistant,
            content,
            api: api.into(),
            provider: provider.into(),
            model: model.into(),
            response_model: None,
            response_id: None,
            usage,
            stop_reason,
            error_message: None,
            timestamp,
            diagnostics: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ToolResultRole {
    #[serde(rename = "toolResult")]
    ToolResult,
}

/// Result of one tool call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultMessage {
    role: ToolResultRole,
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_tool_names: Option<Vec<String>>,
    pub is_error: bool,
    pub timestamp: u64,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ToolResultMessage {
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: Vec<ContentBlock>,
        is_error: bool,
        timestamp: u64,
    ) -> Self {
        Self {
            role: ToolResultRole::ToolResult,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            content,
            details: None,
            added_tool_names: None,
            is_error,
            timestamp,
            extensions: Extensions::new(),
        }
    }
}

/// Canonical model message union.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
}

/// Tool schema presented to a model.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

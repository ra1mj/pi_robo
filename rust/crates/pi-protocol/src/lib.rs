//! Canonical, runtime-neutral contracts shared by the Rust workspace.

mod error;
mod events;
mod message;
mod model;
mod session;
mod settings;

pub use error::{ContractError, ContractErrorCategory};
pub use events::{AgentEvent, AssistantMessageEvent};
pub use message::{
    AssistantMessage, ContentBlock, ImageBlock, Message, MessageContent, StopReason, TextBlock,
    ThinkingBlock, ToolCallBlock, ToolDefinition, ToolResultMessage, Usage, UsageCost, UserMessage,
};
pub use model::{Model, ModelCatalog, ModelCost, ModelInput, ProviderCatalog, ProviderId};
pub use session::{
    BranchSummaryEntry, CURRENT_SESSION_VERSION, CompactionEntry, CustomEntry, CustomMessageEntry,
    KnownSessionRecord, LabelEntry, MessageEntry, ModelChangeEntry, PersistedSessionRecord,
    SessionEntryBase, SessionHeader, SessionInfoEntry, ThinkingLevelChangeEntry,
};
pub use settings::{
    BranchSummarySettings, CompactionSettings, DefaultProjectTrust, ImageSettings,
    MarkdownSettings, PackageSource, ProviderRetrySettings, RetrySettings, Settings,
    TerminalSettings, ThinkingBudgetsSettings, WarningSettings,
};

/// Unknown fields retained while decoding extensible JSON contracts.
pub type Extensions = std::collections::BTreeMap<String, serde_json::Value>;

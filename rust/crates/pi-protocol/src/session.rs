use crate::{ContractError, Extensions};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current TypeScript session format version.
pub const CURRENT_SESSION_VERSION: u32 = 3;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum SessionKind {
    #[serde(rename = "session")]
    Session,
}

/// Session JSONL header.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    #[serde(rename = "type")]
    kind: SessionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl SessionHeader {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        timestamp: impl Into<String>,
        cwd: impl Into<String>,
    ) -> Self {
        Self {
            kind: SessionKind::Session,
            version: Some(CURRENT_SESSION_VERSION),
            id: id.into(),
            timestamp: timestamp.into(),
            cwd: cwd.into(),
            parent_session: None,
            extensions: Extensions::new(),
        }
    }
}

/// Fields shared by append-only session entries.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntryBase {
    pub id: String,
    pub parent_id: Option<String>,
    pub timestamp: String,
}

macro_rules! entry_kind {
    ($name:ident, $value:literal) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        enum $name {
            #[serde(rename = $value)]
            Value,
        }
    };
}

entry_kind!(MessageEntryKind, "message");
entry_kind!(ThinkingLevelEntryKind, "thinking_level_change");
entry_kind!(ModelEntryKind, "model_change");
entry_kind!(CompactionEntryKind, "compaction");
entry_kind!(BranchSummaryEntryKind, "branch_summary");
entry_kind!(CustomEntryKind, "custom");
entry_kind!(LabelEntryKind, "label");
entry_kind!(SessionInfoEntryKind, "session_info");
entry_kind!(CustomMessageEntryKind, "custom_message");

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MessageEntry {
    #[serde(rename = "type")]
    kind: MessageEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub message: Value,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl MessageEntry {
    #[must_use]
    pub fn new(base: SessionEntryBase, message: Value) -> Self {
        Self {
            kind: MessageEntryKind::Value,
            base,
            message,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingLevelChangeEntry {
    #[serde(rename = "type")]
    kind: ThinkingLevelEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub thinking_level: String,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ThinkingLevelChangeEntry {
    #[must_use]
    pub fn new(base: SessionEntryBase, thinking_level: impl Into<String>) -> Self {
        Self {
            kind: ThinkingLevelEntryKind::Value,
            base,
            thinking_level: thinking_level.into(),
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChangeEntry {
    #[serde(rename = "type")]
    kind: ModelEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub provider: String,
    pub model_id: String,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ModelChangeEntry {
    #[must_use]
    pub fn new(
        base: SessionEntryBase,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: ModelEntryKind::Value,
            base,
            provider: provider.into(),
            model_id: model_id.into(),
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionEntry {
    #[serde(rename = "type")]
    kind: CompactionEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_hook: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl CompactionEntry {
    #[must_use]
    pub fn new(
        base: SessionEntryBase,
        summary: impl Into<String>,
        first_kept_entry_id: impl Into<String>,
        tokens_before: u64,
    ) -> Self {
        Self {
            kind: CompactionEntryKind::Value,
            base,
            summary: summary.into(),
            first_kept_entry_id: first_kept_entry_id.into(),
            tokens_before,
            details: None,
            from_hook: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryEntry {
    #[serde(rename = "type")]
    kind: BranchSummaryEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub from_id: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_hook: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl BranchSummaryEntry {
    #[must_use]
    pub fn new(
        base: SessionEntryBase,
        from_id: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            kind: BranchSummaryEntryKind::Value,
            base,
            from_id: from_id.into(),
            summary: summary.into(),
            details: None,
            from_hook: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEntry {
    #[serde(rename = "type")]
    kind: CustomEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub custom_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl CustomEntry {
    #[must_use]
    pub fn new(base: SessionEntryBase, custom_type: impl Into<String>) -> Self {
        Self {
            kind: CustomEntryKind::Value,
            base,
            custom_type: custom_type.into(),
            data: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelEntry {
    #[serde(rename = "type")]
    kind: LabelEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl LabelEntry {
    #[must_use]
    pub fn new(base: SessionEntryBase, target_id: impl Into<String>) -> Self {
        Self {
            kind: LabelEntryKind::Value,
            base,
            target_id: target_id.into(),
            label: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfoEntry {
    #[serde(rename = "type")]
    kind: SessionInfoEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl SessionInfoEntry {
    #[must_use]
    pub fn new(base: SessionEntryBase) -> Self {
        Self {
            kind: SessionInfoEntryKind::Value,
            base,
            name: None,
            extensions: Extensions::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomMessageEntry {
    #[serde(rename = "type")]
    kind: CustomMessageEntryKind,
    #[serde(flatten)]
    pub base: SessionEntryBase,
    pub custom_type: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl CustomMessageEntry {
    #[must_use]
    pub fn new(
        base: SessionEntryBase,
        custom_type: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            kind: CustomMessageEntryKind::Value,
            base,
            custom_type: custom_type.into(),
            content: content.into(),
            details: None,
            display: None,
            extensions: Extensions::new(),
        }
    }
}

/// Typed view of recognized session records.
#[derive(Clone, Debug, PartialEq)]
pub enum KnownSessionRecord {
    Header(SessionHeader),
    Message(MessageEntry),
    ThinkingLevelChange(ThinkingLevelChangeEntry),
    ModelChange(ModelChangeEntry),
    Compaction(CompactionEntry),
    BranchSummary(BranchSummaryEntry),
    Custom(CustomEntry),
    Label(LabelEntry),
    SessionInfo(SessionInfoEntry),
    CustomMessage(CustomMessageEntry),
}

impl KnownSessionRecord {
    fn to_value(&self) -> Result<Value, ContractError> {
        let result = match self {
            Self::Header(value) => serde_json::to_value(value),
            Self::Message(value) => serde_json::to_value(value),
            Self::ThinkingLevelChange(value) => serde_json::to_value(value),
            Self::ModelChange(value) => serde_json::to_value(value),
            Self::Compaction(value) => serde_json::to_value(value),
            Self::BranchSummary(value) => serde_json::to_value(value),
            Self::Custom(value) => serde_json::to_value(value),
            Self::Label(value) => serde_json::to_value(value),
            Self::SessionInfo(value) => serde_json::to_value(value),
            Self::CustomMessage(value) => serde_json::to_value(value),
        };
        result.map_err(ContractError::invalid_json)
    }
}

/// Raw-plus-typed representation that never drops unknown session records.
#[derive(Clone, Debug, PartialEq)]
pub struct PersistedSessionRecord {
    raw: Value,
    known: Option<KnownSessionRecord>,
}

impl PersistedSessionRecord {
    /// Construct an appendable record from a canonical typed representation.
    pub fn from_known(known: KnownSessionRecord) -> Result<Self, ContractError> {
        let raw = known.to_value()?;
        Ok(Self {
            raw,
            known: Some(known),
        })
    }

    /// Parse one JSONL record and produce a typed view when its type is recognized.
    pub fn parse(line: &str) -> Result<Self, ContractError> {
        let raw: Value = serde_json::from_str(line).map_err(ContractError::invalid_json)?;
        let record_type = raw
            .as_object()
            .ok_or_else(|| ContractError::invalid_shape("session record must be an object", "$"))?
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ContractError::invalid_shape("session record type must be a string", "$.type")
            })?;

        macro_rules! decode {
            ($record:ident, $variant:ident) => {
                serde_json::from_value::<$record>(raw.clone())
                    .map(KnownSessionRecord::$variant)
                    .map_err(ContractError::invalid_record)?
            };
        }

        let known = match record_type {
            "session" => Some(decode!(SessionHeader, Header)),
            "message" => Some(decode!(MessageEntry, Message)),
            "thinking_level_change" => Some(decode!(ThinkingLevelChangeEntry, ThinkingLevelChange)),
            "model_change" => Some(decode!(ModelChangeEntry, ModelChange)),
            "compaction" => Some(decode!(CompactionEntry, Compaction)),
            "branch_summary" => Some(decode!(BranchSummaryEntry, BranchSummary)),
            "custom" => Some(decode!(CustomEntry, Custom)),
            "label" => Some(decode!(LabelEntry, Label)),
            "session_info" => Some(decode!(SessionInfoEntry, SessionInfo)),
            "custom_message" => Some(decode!(CustomMessageEntry, CustomMessage)),
            _ => None,
        };

        Ok(Self { raw, known })
    }

    /// Original JSON value before typed decoding.
    #[must_use]
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// Typed record for recognized record types.
    #[must_use]
    pub fn known(&self) -> Option<&KnownSessionRecord> {
        self.known.as_ref()
    }

    /// Serialize the typed representation, retaining unknown fields and unknown records.
    pub fn to_value(&self) -> Result<Value, ContractError> {
        match &self.known {
            Some(known) => known.to_value(),
            None => Ok(self.raw.clone()),
        }
    }
}

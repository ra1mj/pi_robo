use serde::{Deserialize, Serialize};

/// Stable categories for boundary validation failures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractErrorCategory {
    InvalidJson,
    InvalidShape,
    MissingField,
    UnsupportedVersion,
}

/// A structured error safe to pass across crate boundaries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractError {
    pub category: ContractErrorCategory,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl ContractError {
    pub(crate) fn invalid_json(error: serde_json::Error) -> Self {
        Self {
            category: ContractErrorCategory::InvalidJson,
            message: error.to_string(),
            path: None,
        }
    }

    pub(crate) fn invalid_shape(message: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            category: ContractErrorCategory::InvalidShape,
            message: message.into(),
            path: Some(path.into()),
        }
    }

    pub(crate) fn invalid_record(error: serde_json::Error) -> Self {
        Self {
            category: ContractErrorCategory::InvalidShape,
            message: error.to_string(),
            path: Some("$".to_owned()),
        }
    }
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(path) = &self.path {
            write!(formatter, "{} at {path}", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for ContractError {}

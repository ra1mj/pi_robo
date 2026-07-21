use crate::Extensions;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Extensible provider identifier.
pub type ProviderId = String;

/// Modalities accepted by a model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelInput {
    Text,
    Image,
}

/// Per-million-token cost metadata.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Canonical model descriptor derived from the TypeScript catalog.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: ProviderId,
    pub base_url: String,
    pub reasoning: bool,
    pub input: Vec<ModelInput>,
    pub cost: ModelCost,
    pub context_window: u64,
    pub max_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level_map: Option<BTreeMap<String, Option<String>>>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// Models owned by one provider.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalog {
    pub id: ProviderId,
    pub models: BTreeMap<String, Model>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// Versioned deterministic catalog artifact.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub schema_version: u32,
    pub providers: BTreeMap<ProviderId, ProviderCatalog>,
    #[serde(default, flatten, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

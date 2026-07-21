use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

/// Implementation state for one compatibility row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityState {
    Planned,
    InProgress,
    Verified,
    Blocked,
}

/// One TypeScript-to-Rust compatibility obligation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityEntry {
    pub id: String,
    pub milestone: String,
    pub area: String,
    pub owner: String,
    pub oracle: String,
    pub fixture: String,
    pub runner: String,
    #[serde(default)]
    pub normalizers: Vec<String>,
    pub state: CompatibilityState,
}

/// Versioned catalog tracking parity evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityCatalog {
    pub schema_version: u32,
    pub entries: Vec<CompatibilityEntry>,
}

/// Validate schema, ownership, unique IDs, and evidence paths.
pub fn validate_compatibility_catalog(path: &Path) -> Result<CompatibilityCatalog, Vec<String>> {
    let content = fs::read_to_string(path).map_err(|error| vec![error.to_string()])?;
    let catalog: CompatibilityCatalog =
        serde_json::from_str(&content).map_err(|error| vec![error.to_string()])?;
    let mut errors = Vec::new();
    if catalog.schema_version != 1 {
        errors.push(format!(
            "unsupported compatibility catalog schema {}",
            catalog.schema_version
        ));
    }

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let owners = [
        "pi-agent",
        "pi-cli",
        "pi-model",
        "pi-protocol",
        "pi-provider",
        "pi-resources",
        "pi-runtime",
        "pi-store",
        "pi-test-support",
        "pi-tools",
    ];
    let mut ids = BTreeSet::new();
    for entry in &catalog.entries {
        if entry.id.trim().is_empty() {
            errors.push("compatibility entry has an empty id".to_owned());
        } else if !ids.insert(entry.id.as_str()) {
            errors.push(format!("duplicate compatibility id: {}", entry.id));
        }
        if !owners.contains(&entry.owner.as_str()) {
            errors.push(format!("unknown owner for {}: {}", entry.id, entry.owner));
        }
        if entry.oracle.trim().is_empty() {
            errors.push(format!("missing oracle for {}", entry.id));
        }
        validate_evidence_path(
            &repository_root,
            &entry.id,
            "fixture",
            &entry.fixture,
            &mut errors,
        );
        validate_evidence_path(
            &repository_root,
            &entry.id,
            "runner",
            &entry.runner,
            &mut errors,
        );
        for normalizer in &entry.normalizers {
            if !normalizer.starts_with('/') || normalizer == "/" {
                errors.push(format!(
                    "normalizer for {} must be a non-root JSON pointer: {normalizer}",
                    entry.id
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(catalog)
    } else {
        Err(errors)
    }
}

fn validate_evidence_path(
    repository_root: &Path,
    id: &str,
    field: &str,
    value: &str,
    errors: &mut Vec<String>,
) {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        errors.push(format!("invalid {field} path for {id}: {value}"));
    } else if !repository_root.join(path).is_file() {
        errors.push(format!("missing {field} path for {id}: {value}"));
    }
}

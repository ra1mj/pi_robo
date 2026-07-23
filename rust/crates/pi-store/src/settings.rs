use crate::{StoreDiagnostic, StoreError, StorePaths};
use pi_protocol::{DefaultProjectTrust, Settings};
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsSnapshot {
    pub global: Settings,
    pub project: Option<Settings>,
    pub merged: Settings,
    pub raw_global: Value,
    pub raw_project: Option<Value>,
    pub merged_raw: Value,
    pub project_loaded: bool,
    pub diagnostics: Vec<StoreDiagnostic>,
}

impl SettingsSnapshot {
    #[must_use]
    pub fn default_project_trust(&self) -> DefaultProjectTrust {
        self.merged
            .default_project_trust
            .unwrap_or(DefaultProjectTrust::Ask)
    }
}

pub fn load_settings(
    paths: &StorePaths,
    project_trusted: bool,
) -> Result<SettingsSnapshot, StoreError> {
    let mut diagnostics = Vec::new();
    let (raw_global, global) = read_settings(&paths.settings_file(), &mut diagnostics)?;
    let (raw_project, project) = if project_trusted {
        let (raw, typed) = read_settings(&paths.project_settings_file(), &mut diagnostics)?;
        (Some(raw), Some(typed))
    } else {
        (None, None)
    };

    let merged_raw = match &raw_project {
        Some(project_value) => merge_settings(&raw_global, project_value),
        None => raw_global.clone(),
    };
    let merged = decode_settings(&merged_raw, None, &mut diagnostics);

    Ok(SettingsSnapshot {
        global,
        project,
        merged,
        raw_global,
        raw_project,
        merged_raw,
        project_loaded: project_trusted,
        diagnostics,
    })
}

fn read_settings(
    path: &Path,
    diagnostics: &mut Vec<StoreDiagnostic>,
) -> Result<(Value, Settings), StoreError> {
    let content = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Value::Object(Map::new()), Settings::default()));
        }
        Err(error) => return Err(StoreError::io(error, path)),
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(StoreDiagnostic::error(&StoreError::json(error, path)));
            return Ok((Value::Object(Map::new()), Settings::default()));
        }
    };
    if !value.is_object() {
        let error = StoreError::new(
            crate::StoreErrorCategory::InvalidShape,
            "settings document must be a JSON object",
        )
        .with_path(path);
        diagnostics.push(StoreDiagnostic::error(&error));
        return Ok((Value::Object(Map::new()), Settings::default()));
    }
    let typed = decode_settings(&value, Some(path), diagnostics);
    Ok((value, typed))
}

fn decode_settings(
    value: &Value,
    path: Option<&Path>,
    diagnostics: &mut Vec<StoreDiagnostic>,
) -> Settings {
    match serde_json::from_value(value.clone()) {
        Ok(settings) => settings,
        Err(error) => {
            let mut store_error = StoreError::new(
                crate::StoreErrorCategory::InvalidShape,
                format!("invalid settings value: {error}"),
            );
            if let Some(path) = path {
                store_error = store_error.with_path(path);
            }
            diagnostics.push(StoreDiagnostic::error(&store_error));
            Settings::default()
        }
    }
}

fn merge_settings(global: &Value, project: &Value) -> Value {
    let mut merged = global.as_object().cloned().unwrap_or_default();
    for (key, project_value) in project.as_object().into_iter().flatten() {
        let value = match (merged.get(key), project_value) {
            (Some(Value::Object(global_nested)), Value::Object(project_nested)) => {
                let mut nested = global_nested.clone();
                nested.extend(project_nested.clone());
                Value::Object(nested)
            }
            _ => project_value.clone(),
        };
        merged.insert(key.clone(), value);
    }
    Value::Object(merged)
}

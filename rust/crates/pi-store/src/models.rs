use crate::{StoreDiagnostic, StoreError, StoreErrorCategory, StorePaths};
use pi_protocol::{Model, ModelCatalog};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const EMBEDDED_CATALOG: &str = include_str!("../../../assets/models.json");
const SUPPORTED_APIS: [&str; 4] = [
    "anthropic-messages",
    "openai-completions",
    "openai-responses",
    "google-generative-ai",
];

#[derive(Clone, PartialEq)]
pub struct ModelSourceSnapshot {
    pub catalog: ModelCatalog,
    pub raw_models: Value,
    provider_api_keys: BTreeMap<String, String>,
    pub custom_providers: BTreeSet<String>,
    pub diagnostics: Vec<StoreDiagnostic>,
}

impl ModelSourceSnapshot {
    #[must_use]
    pub fn configured_api_key(&self, provider: &str) -> Option<&str> {
        self.provider_api_keys.get(provider).map(String::as_str)
    }

    #[must_use]
    pub fn supported_model(&self, provider: &str, model_id: &str) -> Option<&Model> {
        if !matches!(provider, "openai" | "anthropic" | "google")
            && !self.custom_providers.contains(provider)
        {
            return None;
        }
        self.catalog
            .providers
            .get(provider)
            .and_then(|catalog| catalog.models.get(model_id))
    }
}

impl std::fmt::Debug for ModelSourceSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelSourceSnapshot")
            .field("catalog_schema_version", &self.catalog.schema_version)
            .field("configured_providers", &self.provider_api_keys.keys())
            .field("custom_providers", &self.custom_providers)
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

pub fn load_model_sources(paths: &StorePaths) -> Result<ModelSourceSnapshot, StoreError> {
    let mut catalog: ModelCatalog = serde_json::from_str(EMBEDDED_CATALOG).map_err(|error| {
        StoreError::new(
            StoreErrorCategory::InvalidJson,
            format!("embedded model catalog is invalid: {error}"),
        )
    })?;
    if catalog.schema_version != 1 {
        return Err(StoreError::new(
            StoreErrorCategory::UnsupportedVersion,
            format!(
                "unsupported embedded model catalog schema version {}",
                catalog.schema_version
            ),
        ));
    }

    let models_path = paths.models_file();
    let source = match std::fs::read_to_string(&models_path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ModelSourceSnapshot {
                catalog,
                raw_models: Value::Object(Map::new()),
                provider_api_keys: BTreeMap::new(),
                custom_providers: BTreeSet::new(),
                diagnostics: Vec::new(),
            });
        }
        Err(error) => return Err(StoreError::io(error, &models_path)),
    };
    let stripped = strip_json_comments(&source);
    let raw_models: Value =
        serde_json::from_str(&stripped).map_err(|error| StoreError::json(error, &models_path))?;
    let providers = raw_models
        .as_object()
        .and_then(|root| root.get("providers"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            StoreError::new(
                StoreErrorCategory::InvalidShape,
                "models.json must contain a providers object",
            )
            .with_path(&models_path)
        })?;

    let mut diagnostics = Vec::new();
    let mut provider_api_keys = BTreeMap::new();
    let mut custom_providers = BTreeSet::new();
    for (provider_id, provider_value) in providers {
        let Some(provider) = provider_value.as_object() else {
            diagnostics.push(
                StoreDiagnostic::warning(format!("provider {provider_id:?} must be a JSON object"))
                    .with_path(&models_path),
            );
            continue;
        };
        if let Some(api_key) = provider.get("apiKey").and_then(Value::as_str) {
            provider_api_keys.insert(provider_id.clone(), api_key.to_owned());
        }
        let was_builtin = catalog.providers.contains_key(provider_id);
        if was_builtin && !matches!(provider_id.as_str(), "openai" | "anthropic" | "google") {
            diagnostics.push(
                StoreDiagnostic::warning(format!(
                    "provider brand {provider_id:?} is not directly supported by Rust milestone 1"
                ))
                .with_path(&models_path),
            );
        }
        apply_provider(
            &mut catalog,
            provider_id,
            provider,
            &models_path,
            &mut diagnostics,
        );
        if !was_builtin && catalog.providers.contains_key(provider_id) {
            custom_providers.insert(provider_id.clone());
        }
    }

    Ok(ModelSourceSnapshot {
        catalog,
        raw_models,
        provider_api_keys,
        custom_providers,
        diagnostics,
    })
}

fn apply_provider(
    catalog: &mut ModelCatalog,
    provider_id: &str,
    config: &Map<String, Value>,
    models_path: &Path,
    diagnostics: &mut Vec<StoreDiagnostic>,
) {
    let inherited = catalog.providers.get(provider_id).and_then(|provider| {
        provider
            .models
            .values()
            .next()
            .map(|model| (model.api.clone(), model.base_url.clone()))
    });

    if let Some(provider) = catalog.providers.get_mut(provider_id) {
        for (key, value) in config {
            if !matches!(key.as_str(), "models" | "modelOverrides" | "apiKey") {
                provider.extensions.insert(key.clone(), value.clone());
            }
        }
        for model in provider.models.values_mut() {
            apply_provider_values(model, config);
        }
    }

    if let Some(custom_models) = config.get("models").and_then(Value::as_array) {
        for raw_model in custom_models {
            match compose_custom_model(provider_id, config, raw_model, inherited.as_ref()) {
                Ok(model) => {
                    let provider = catalog
                        .providers
                        .entry(provider_id.to_owned())
                        .or_insert_with(|| pi_protocol::ProviderCatalog {
                            id: provider_id.to_owned(),
                            models: BTreeMap::new(),
                            extensions: provider_extensions(config),
                        });
                    provider.models.insert(model.id.clone(), model);
                }
                Err(message) => diagnostics.push(
                    StoreDiagnostic::warning(format!("provider {provider_id:?}: {message}"))
                        .with_path(models_path),
                ),
            }
        }
    }

    if let Some(overrides) = config.get("modelOverrides").and_then(Value::as_object)
        && let Some(provider) = catalog.providers.get_mut(provider_id)
    {
        for (model_id, model_override) in overrides {
            let Some(existing) = provider.models.get(model_id) else {
                continue;
            };
            let mut value = match serde_json::to_value(existing) {
                Ok(value) => value,
                Err(error) => {
                    diagnostics.push(
                        StoreDiagnostic::warning(format!(
                            "could not prepare override for {provider_id}/{model_id}: {error}"
                        ))
                        .with_path(models_path),
                    );
                    continue;
                }
            };
            deep_merge(&mut value, model_override);
            if let Some(object) = value.as_object_mut() {
                object.insert("id".to_owned(), Value::String(model_id.clone()));
                object.insert("provider".to_owned(), Value::String(provider_id.to_owned()));
            }
            match serde_json::from_value(value) {
                Ok(model) => {
                    provider.models.insert(model_id.clone(), model);
                }
                Err(error) => diagnostics.push(
                    StoreDiagnostic::warning(format!(
                        "invalid override for {provider_id}/{model_id}: {error}"
                    ))
                    .with_path(models_path),
                ),
            }
        }
    }
}

fn provider_extensions(config: &Map<String, Value>) -> BTreeMap<String, Value> {
    config
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "models" | "modelOverrides" | "apiKey"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn apply_provider_values(model: &mut Model, config: &Map<String, Value>) {
    if let Some(base_url) = config.get("baseUrl").and_then(Value::as_str) {
        model.base_url = base_url.to_owned();
    }
    if let Some(headers) = string_map(config.get("headers")) {
        model
            .headers
            .get_or_insert_with(BTreeMap::new)
            .extend(headers);
    }
    if let Some(provider_compat) = config.get("compat") {
        let compat = model
            .compat
            .get_or_insert_with(|| Value::Object(Map::new()));
        deep_merge(compat, provider_compat);
    }
}

fn compose_custom_model(
    provider_id: &str,
    provider: &Map<String, Value>,
    raw_model: &Value,
    inherited: Option<&(String, String)>,
) -> Result<Model, String> {
    let object = raw_model
        .as_object()
        .ok_or_else(|| "custom model must be a JSON object".to_owned())?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "custom model id is required".to_owned())?;
    let api = object
        .get("api")
        .and_then(Value::as_str)
        .or_else(|| provider.get("api").and_then(Value::as_str))
        .or_else(|| inherited.map(|value| value.0.as_str()))
        .ok_or_else(|| format!("model {id:?} has no api"))?;
    if !SUPPORTED_APIS.contains(&api) {
        return Err(format!(
            "model {id:?} uses unsupported milestone-1 api {api:?}"
        ));
    }
    let base_url = object
        .get("baseUrl")
        .and_then(Value::as_str)
        .or_else(|| provider.get("baseUrl").and_then(Value::as_str))
        .or_else(|| inherited.map(|value| value.1.as_str()))
        .ok_or_else(|| format!("model {id:?} has no baseUrl"))?;

    let mut value = json!({
        "id": id,
        "name": id,
        "api": api,
        "provider": provider_id,
        "baseUrl": base_url,
        "reasoning": false,
        "input": ["text"],
        "cost": {
            "input": 0.0,
            "output": 0.0,
            "cacheRead": 0.0,
            "cacheWrite": 0.0
        },
        "contextWindow": 128000,
        "maxTokens": 16384
    });
    if let Some(headers) = provider.get("headers") {
        value
            .as_object_mut()
            .expect("model template is an object")
            .insert("headers".to_owned(), headers.clone());
    }
    if let Some(compat) = provider.get("compat") {
        value
            .as_object_mut()
            .expect("model template is an object")
            .insert("compat".to_owned(), compat.clone());
    }
    deep_merge(&mut value, raw_model);
    let value_object = value
        .as_object_mut()
        .expect("custom model merge preserves object");
    value_object.insert("id".to_owned(), Value::String(id.to_owned()));
    value_object.insert("provider".to_owned(), Value::String(provider_id.to_owned()));
    value_object.insert("api".to_owned(), Value::String(api.to_owned()));
    value_object.insert("baseUrl".to_owned(), Value::String(base_url.to_owned()));
    serde_json::from_value(value).map_err(|error| format!("invalid model {id:?}: {error}"))
}

fn string_map(value: Option<&Value>) -> Option<BTreeMap<String, String>> {
    value.and_then(Value::as_object).map(|object| {
        object
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
            .collect()
    })
}

fn deep_merge(target: &mut Value, overlay: &Value) {
    match (target, overlay) {
        (Value::Object(target), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match target.get_mut(key) {
                    Some(existing) => deep_merge(existing, value),
                    None => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, overlay) => *target = overlay.clone(),
    }
}

/// Strip `//` comments and trailing commas without changing string literals.
#[must_use]
pub fn strip_json_comments(input: &str) -> String {
    let mut without_comments = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(character) = characters.next() {
        if in_string {
            without_comments.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            without_comments.push(character);
            continue;
        }
        if character == '/' && characters.peek() == Some(&'/') {
            characters.next();
            for comment_character in characters.by_ref() {
                if comment_character == '\n' {
                    without_comments.push('\n');
                    break;
                }
            }
            continue;
        }
        without_comments.push(character);
    }

    let characters: Vec<char> = without_comments.chars().collect();
    let mut result = String::with_capacity(without_comments.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < characters.len() {
        let character = characters[index];
        if in_string {
            result.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if character == '"' {
            in_string = true;
            result.push(character);
            index += 1;
            continue;
        }
        if character == ',' {
            let mut next = index + 1;
            while next < characters.len() && characters[next].is_whitespace() {
                next += 1;
            }
            if next < characters.len() && matches!(characters[next], '}' | ']') {
                index += 1;
                continue;
            }
        }
        result.push(character);
        index += 1;
    }
    result
}

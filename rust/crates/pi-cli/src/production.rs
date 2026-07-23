use pi_model::{ModelService, ModelServiceError, ModelServiceErrorCategory};
use pi_protocol::{Model, Settings};
use pi_provider::{
    AnthropicMessagesAdapter, GoogleGenerativeLanguageAdapter, OpenAiChatAdapter,
    OpenAiResponsesAdapter, ProviderAdapterConfig, ProviderTimeouts,
    SecretString as ProviderSecret,
};
use pi_store::ResolvedCredential;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

pub trait ModelServiceFactory: Send + Sync {
    fn requires_credential(&self) -> bool;

    fn create(
        &self,
        model: &Model,
        credential: Option<&ResolvedCredential>,
        settings: &Settings,
    ) -> Result<Arc<dyn ModelService>, ModelServiceError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProductionModelServiceFactory;

impl ModelServiceFactory for ProductionModelServiceFactory {
    fn requires_credential(&self) -> bool {
        true
    }

    fn create(
        &self,
        model: &Model,
        credential: Option<&ResolvedCredential>,
        settings: &Settings,
    ) -> Result<Arc<dyn ModelService>, ModelServiceError> {
        let credential = credential.ok_or_else(|| {
            ModelServiceError::new(
                ModelServiceErrorCategory::Authentication,
                format!("no API key was resolved for provider {:?}", model.provider),
                false,
            )
        })?;
        let idle_timeout = settings
            .http_idle_timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(60));
        let timeouts = ProviderTimeouts::new(Duration::from_secs(10), idle_timeout, idle_timeout);
        let mut headers = model.headers.clone().unwrap_or_default();
        let secret = credential.secret.expose_secret();
        let mut config = match model.api.as_str() {
            "openai-completions" | "openai-responses" => {
                ProviderAdapterConfig::new(&model.base_url, timeouts)
                    .with_authorization(ProviderSecret::new(format!("Bearer {secret}")))
            }
            "anthropic-messages" => {
                headers.insert("x-api-key".to_owned(), secret.to_owned());
                ProviderAdapterConfig::new(&model.base_url, timeouts)
            }
            "google-generative-ai" => {
                headers.insert("x-goog-api-key".to_owned(), secret.to_owned());
                ProviderAdapterConfig::new(&model.base_url, timeouts)
            }
            api => {
                return Err(ModelServiceError::new(
                    ModelServiceErrorCategory::Configuration,
                    format!("unsupported pi-rs milestone-1 provider protocol {api:?}"),
                    false,
                ));
            }
        };
        config = config.with_headers(normalize_headers(headers));
        if let Some(proxy) = settings
            .http_proxy
            .as_deref()
            .filter(|proxy| !proxy.trim().is_empty())
        {
            config = config.with_proxy(ProviderSecret::new(proxy));
        }

        match model.api.as_str() {
            "openai-completions" => Ok(Arc::new(OpenAiChatAdapter::new(&config)?)),
            "openai-responses" => Ok(Arc::new(OpenAiResponsesAdapter::new(&config)?)),
            "anthropic-messages" => Ok(Arc::new(AnthropicMessagesAdapter::new(&config)?)),
            "google-generative-ai" => Ok(Arc::new(GoogleGenerativeLanguageAdapter::new(&config)?)),
            _ => unreachable!("unsupported protocol returned above"),
        }
    }
}

fn normalize_headers(headers: BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .into_iter()
        .filter(|(name, value)| !name.trim().is_empty() && !value.contains(['\r', '\n']))
        .collect()
}

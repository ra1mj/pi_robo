use crate::{
    ProviderAdapterConfig, ProviderErrorContext, ProviderFailureKind, SecretString,
    normalize_provider_error,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use pi_model::{Cancellation, ModelServiceError};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::BTreeMap;
use std::pin::Pin;
use std::time::Duration;

type ResponseBodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

/// Captured response bytes with an explicit truncation marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedBody {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

impl BoundedBody {
    #[must_use]
    pub fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

/// Shared Reqwest client with provider-safe retry, proxy, TLS, and timeout policy.
#[derive(Clone)]
pub struct ProviderHttpClient {
    client: reqwest::Client,
    base_url: String,
    response_header_timeout: Duration,
    body_idle_timeout: Duration,
    secrets: Vec<SecretString>,
}

impl ProviderHttpClient {
    pub fn new(config: &ProviderAdapterConfig) -> Result<Self, ModelServiceError> {
        validate_timeouts(config)?;
        let mut secrets = collect_secrets(config);
        let base_url = reqwest::Url::parse(config.base_url()).map_err(|_| {
            ModelServiceError::new(
                pi_model::ModelServiceErrorCategory::Configuration,
                "provider base URL is invalid",
                false,
            )
        })?;
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
        {
            secrets.push(SecretString::new(config.base_url()));
        }

        let default_headers = build_headers(config)?;
        let mut builder = reqwest::Client::builder()
            .connect_timeout(config.timeouts().connect())
            .retry(reqwest::retry::never())
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .default_headers(default_headers);
        if let Some(proxy) = config.proxy() {
            let proxy = reqwest::Proxy::all(proxy.expose_secret()).map_err(|error| {
                normalize_provider_error(
                    &ProviderErrorContext::new(
                        ProviderFailureKind::Configuration,
                        format!("provider proxy configuration failed: {error}"),
                    ),
                    &secret_refs(&secrets),
                )
            })?;
            builder = builder.proxy(proxy);
        }

        let client = builder.build().map_err(|error| {
            normalize_provider_error(
                &ProviderErrorContext::new(
                    ProviderFailureKind::Configuration,
                    format!("provider HTTP client configuration failed: {error}"),
                ),
                &secret_refs(&secrets),
            )
        })?;

        Ok(Self {
            client,
            base_url: config.base_url().trim_end_matches('/').to_owned(),
            response_header_timeout: config.timeouts().response_header(),
            body_idle_timeout: config.timeouts().body_idle(),
            secrets,
        })
    }

    /// Sends a JSON POST and returns once response headers are available.
    pub async fn post_json(
        &self,
        path: &str,
        body: &Value,
        cancellation: &dyn Cancellation,
    ) -> Result<ProviderHttpResponse, ModelServiceError> {
        if cancellation.is_cancelled() {
            return Err(ModelServiceError::cancelled());
        }
        let endpoint = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let request = self.client.post(endpoint).json(body);
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(ModelServiceError::cancelled()),
            result = tokio::time::timeout(self.response_header_timeout, request.send()) => {
                match result {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => return Err(self.normalize_reqwest_error(&error)),
                    Err(_) => {
                        return Err(normalize_provider_error(
                            &ProviderErrorContext::new(
                                ProviderFailureKind::Timeout,
                                "provider response-header timeout",
                            ),
                            &secret_refs(&self.secrets),
                        ));
                    }
                }
            }
        };

        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect();
        Ok(ProviderHttpResponse {
            status,
            headers,
            body: Box::pin(response.bytes_stream()),
            body_idle_timeout: self.body_idle_timeout,
            secrets: self.secrets.clone(),
            terminated: false,
        })
    }

    fn normalize_reqwest_error(&self, error: &reqwest::Error) -> ModelServiceError {
        let kind = if error.is_timeout() {
            ProviderFailureKind::Timeout
        } else {
            ProviderFailureKind::Network
        };
        normalize_provider_error(
            &ProviderErrorContext::new(kind, format!("provider HTTP request failed: {error}")),
            &secret_refs(&self.secrets),
        )
    }

    pub(crate) fn normalize_error(&self, context: &ProviderErrorContext) -> ModelServiceError {
        normalize_provider_error(context, &secret_refs(&self.secrets))
    }
}

/// Streaming HTTP response whose reads race cancellation and an idle timeout.
pub struct ProviderHttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: ResponseBodyStream,
    body_idle_timeout: Duration,
    secrets: Vec<SecretString>,
    terminated: bool,
}

impl ProviderHttpResponse {
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    #[must_use]
    pub const fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    pub async fn next_chunk(
        &mut self,
        cancellation: &dyn Cancellation,
    ) -> Result<Option<Bytes>, ModelServiceError> {
        if self.terminated {
            return Ok(None);
        }
        if cancellation.is_cancelled() {
            self.terminated = true;
            return Err(ModelServiceError::cancelled());
        }

        tokio::select! {
            () = cancellation.cancelled() => {
                self.terminated = true;
                Err(ModelServiceError::cancelled())
            }
            result = tokio::time::timeout(self.body_idle_timeout, self.body.next()) => {
                match result {
                    Ok(Some(Ok(bytes))) => Ok(Some(bytes)),
                    Ok(Some(Err(error))) => {
                        self.terminated = true;
                        Err(normalize_provider_error(
                            &ProviderErrorContext::new(
                                if error.is_timeout() {
                                    ProviderFailureKind::Timeout
                                } else {
                                    ProviderFailureKind::Network
                                },
                                format!("provider response body failed: {error}"),
                            ),
                            &secret_refs(&self.secrets),
                        ))
                    }
                    Ok(None) => {
                        self.terminated = true;
                        Ok(None)
                    }
                    Err(_) => {
                        self.terminated = true;
                        Err(normalize_provider_error(
                            &ProviderErrorContext::new(
                                ProviderFailureKind::Timeout,
                                "provider response body idle timeout",
                            ),
                            &secret_refs(&self.secrets),
                        ))
                    }
                }
            }
        }
    }

    /// Reads no more than `limit` bytes and drops the remaining response on overflow.
    pub async fn read_body_bounded(
        mut self,
        limit: usize,
        cancellation: &dyn Cancellation,
    ) -> Result<BoundedBody, ModelServiceError> {
        let mut bytes = Vec::with_capacity(limit.min(8_192));
        while let Some(chunk) = self.next_chunk(cancellation).await? {
            let remaining = limit.saturating_sub(bytes.len());
            if chunk.len() > remaining {
                bytes.extend_from_slice(&chunk[..remaining]);
                return Ok(BoundedBody {
                    bytes,
                    truncated: true,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(BoundedBody {
            bytes,
            truncated: false,
        })
    }
}

fn validate_timeouts(config: &ProviderAdapterConfig) -> Result<(), ModelServiceError> {
    let timeouts = config.timeouts();
    if timeouts.connect().is_zero()
        || timeouts.response_header().is_zero()
        || timeouts.body_idle().is_zero()
    {
        return Err(ModelServiceError::new(
            pi_model::ModelServiceErrorCategory::Configuration,
            "provider timeouts must be greater than zero",
            false,
        ));
    }
    Ok(())
}

fn build_headers(config: &ProviderAdapterConfig) -> Result<HeaderMap, ModelServiceError> {
    let mut headers = HeaderMap::new();
    for (name, value) in config.headers() {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            ModelServiceError::new(
                pi_model::ModelServiceErrorCategory::Configuration,
                "provider header name is invalid",
                false,
            )
        })?;
        let value = HeaderValue::from_str(value).map_err(|_| {
            ModelServiceError::new(
                pi_model::ModelServiceErrorCategory::Configuration,
                "provider header value is invalid",
                false,
            )
        })?;
        headers.insert(name, value);
    }
    if let Some(authorization) = config.authorization() {
        let value = HeaderValue::from_str(authorization.expose_secret()).map_err(|_| {
            ModelServiceError::new(
                pi_model::ModelServiceErrorCategory::Configuration,
                "provider authorization value is invalid",
                false,
            )
        })?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

fn collect_secrets(config: &ProviderAdapterConfig) -> Vec<SecretString> {
    let mut secrets = Vec::new();
    if let Some(authorization) = config.authorization() {
        secrets.push(authorization.clone());
    }
    if let Some(proxy) = config.proxy() {
        secrets.push(proxy.clone());
    }
    for (name, value) in config.headers() {
        let lower_name = name.to_ascii_lowercase();
        if lower_name == "authorization"
            || lower_name == "cookie"
            || lower_name == "x-api-key"
            || lower_name.ends_with("-token")
            || lower_name.ends_with("-key")
        {
            secrets.push(SecretString::new(value));
        }
    }
    secrets
}

fn secret_refs(secrets: &[SecretString]) -> Vec<&SecretString> {
    secrets.iter().collect()
}

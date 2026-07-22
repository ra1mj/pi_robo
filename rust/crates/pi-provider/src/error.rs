use crate::SecretString;
use pi_model::{ModelServiceError, ModelServiceErrorCategory};

const MAX_ERROR_MESSAGE_CHARS: usize = 4_096;

/// Transport or provider failure source before canonical classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderFailureKind {
    Configuration,
    Http,
    Timeout,
    Network,
    Protocol,
    Cancelled,
    Provider,
}

/// Redaction-ready failure details captured at a provider boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderErrorContext {
    pub kind: ProviderFailureKind,
    pub message: String,
    pub http_status: Option<u16>,
    pub provider_code: Option<String>,
    pub retry_after_ms: Option<u64>,
    pub retryable: Option<bool>,
}

impl ProviderErrorContext {
    #[must_use]
    pub fn new(kind: ProviderFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            http_status: None,
            provider_code: None,
            retry_after_ms: None,
            retryable: None,
        }
    }

    #[must_use]
    pub const fn with_http_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    #[must_use]
    pub fn with_provider_code(mut self, code: impl Into<String>) -> Self {
        self.provider_code = Some(code.into());
        self
    }

    #[must_use]
    pub const fn with_retry_after_ms(mut self, retry_after_ms: u64) -> Self {
        self.retry_after_ms = Some(retry_after_ms);
        self
    }

    #[must_use]
    pub const fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }
}

/// Converts transport/provider details into the sole retry classification contract.
#[must_use]
pub fn normalize_provider_error(
    context: &ProviderErrorContext,
    secrets: &[&SecretString],
) -> ModelServiceError {
    let searchable = format!(
        "{} {}",
        context.provider_code.as_deref().unwrap_or_default(),
        context.message
    )
    .to_ascii_lowercase();
    let category = classify(context, &searchable);
    let default_retryable = matches!(
        category,
        ModelServiceErrorCategory::RateLimit
            | ModelServiceErrorCategory::Timeout
            | ModelServiceErrorCategory::Network
            | ModelServiceErrorCategory::Unavailable
            | ModelServiceErrorCategory::Server
    );
    let retryable = if is_hard_non_retryable(category) {
        false
    } else {
        context.retryable.unwrap_or(default_retryable)
    };
    let message = redact_and_bound(&context.message, secrets);
    let mut error = ModelServiceError::new(category, message, retryable);
    error.http_status = context.http_status;
    error.provider_code = context
        .provider_code
        .as_deref()
        .map(|code| redact_and_bound(code, secrets));
    error.retry_after_ms = context.retry_after_ms;
    error
}

fn classify(context: &ProviderErrorContext, searchable: &str) -> ModelServiceErrorCategory {
    match context.kind {
        ProviderFailureKind::Configuration => ModelServiceErrorCategory::Configuration,
        ProviderFailureKind::Timeout => ModelServiceErrorCategory::Timeout,
        ProviderFailureKind::Network => ModelServiceErrorCategory::Network,
        ProviderFailureKind::Protocol => ModelServiceErrorCategory::Protocol,
        ProviderFailureKind::Cancelled => ModelServiceErrorCategory::Cancelled,
        ProviderFailureKind::Http | ProviderFailureKind::Provider => {
            classify_provider_failure(context.http_status, searchable)
        }
    }
}

fn classify_provider_failure(
    http_status: Option<u16>,
    searchable: &str,
) -> ModelServiceErrorCategory {
    if contains_any(
        searchable,
        &[
            "context_length",
            "context window",
            "maximum context",
            "too many tokens",
            "input is too long",
        ],
    ) {
        return ModelServiceErrorCategory::ContextOverflow;
    }
    if contains_any(
        searchable,
        &[
            "insufficient_quota",
            "quota exceeded",
            "billing quota",
            "credit balance",
        ],
    ) {
        return ModelServiceErrorCategory::QuotaExceeded;
    }
    if contains_any(searchable, &["rate_limit", "rate limit", "throttl"]) {
        return ModelServiceErrorCategory::RateLimit;
    }
    if contains_any(
        searchable,
        &["invalid_api_key", "authentication", "unauthorized"],
    ) {
        return ModelServiceErrorCategory::Authentication;
    }
    if contains_any(searchable, &["permission_denied", "permission denied"]) {
        return ModelServiceErrorCategory::Permission;
    }
    if contains_any(searchable, &["model_not_found", "not found"]) {
        return ModelServiceErrorCategory::NotFound;
    }
    if contains_any(searchable, &["request_timeout", "timed out", "timeout"]) {
        return ModelServiceErrorCategory::Timeout;
    }
    if contains_any(searchable, &["service_unavailable"]) {
        return ModelServiceErrorCategory::Unavailable;
    }
    if contains_any(
        searchable,
        &["server_error", "internal_error", "overloaded_error"],
    ) {
        return ModelServiceErrorCategory::Server;
    }
    if contains_any(searchable, &["invalid_request", "bad request"]) {
        return ModelServiceErrorCategory::InvalidRequest;
    }

    match http_status {
        Some(400 | 405 | 409 | 410 | 413 | 415 | 422) => ModelServiceErrorCategory::InvalidRequest,
        Some(401) => ModelServiceErrorCategory::Authentication,
        Some(403) => ModelServiceErrorCategory::Permission,
        Some(404) => ModelServiceErrorCategory::NotFound,
        Some(408) => ModelServiceErrorCategory::Timeout,
        Some(429) => ModelServiceErrorCategory::RateLimit,
        Some(502..=504) => ModelServiceErrorCategory::Unavailable,
        Some(500..=599) => ModelServiceErrorCategory::Server,
        _ => ModelServiceErrorCategory::Unknown,
    }
}

fn contains_any(value: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| value.contains(pattern))
}

fn is_hard_non_retryable(category: ModelServiceErrorCategory) -> bool {
    matches!(
        category,
        ModelServiceErrorCategory::Configuration
            | ModelServiceErrorCategory::Authentication
            | ModelServiceErrorCategory::Permission
            | ModelServiceErrorCategory::InvalidRequest
            | ModelServiceErrorCategory::NotFound
            | ModelServiceErrorCategory::ContextOverflow
            | ModelServiceErrorCategory::QuotaExceeded
            | ModelServiceErrorCategory::Cancelled
    )
}

fn redact_and_bound(value: &str, secrets: &[&SecretString]) -> String {
    let mut redacted = value.to_owned();
    for secret in secrets {
        let exposed = secret.expose_secret();
        if !exposed.is_empty() {
            redacted = redacted.replace(exposed, "[REDACTED]");
        }
    }
    let bounded: String = redacted.chars().take(MAX_ERROR_MESSAGE_CHARS).collect();
    if bounded.is_empty() {
        "provider request failed".to_owned()
    } else {
        bounded
    }
}

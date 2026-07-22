use pi_model::ModelServiceErrorCategory;
use pi_provider::{
    ProviderAdapterConfig, ProviderErrorContext, ProviderFailureKind, ProviderTimeouts,
    SecretString, normalize_provider_error,
};
use std::collections::BTreeMap;
use std::time::Duration;

fn normalize(context: ProviderErrorContext) -> pi_model::ModelServiceError {
    normalize_provider_error(&context, &[])
}

#[test]
fn http_statuses_have_stable_categories() {
    let cases = [
        (401, ModelServiceErrorCategory::Authentication, false),
        (403, ModelServiceErrorCategory::Permission, false),
        (404, ModelServiceErrorCategory::NotFound, false),
        (429, ModelServiceErrorCategory::RateLimit, true),
        (500, ModelServiceErrorCategory::Server, true),
        (503, ModelServiceErrorCategory::Unavailable, true),
    ];

    for (status, category, retryable) in cases {
        let error = normalize(
            ProviderErrorContext::new(ProviderFailureKind::Http, "provider failure")
                .with_http_status(status),
        );
        assert_eq!(error.category, category);
        assert_eq!(error.retryable, retryable);
        assert_eq!(error.http_status, Some(status));
    }
}

#[test]
fn provider_semantics_override_ambiguous_http_statuses() {
    let quota = normalize(
        ProviderErrorContext::new(ProviderFailureKind::Http, "billing quota exceeded")
            .with_http_status(429)
            .with_provider_code("insufficient_quota")
            .with_retryable(true),
    );
    assert_eq!(quota.category, ModelServiceErrorCategory::QuotaExceeded);
    assert!(!quota.retryable);

    let overflow = normalize(
        ProviderErrorContext::new(
            ProviderFailureKind::Provider,
            "maximum context window reached",
        )
        .with_provider_code("context_length_exceeded"),
    );
    assert_eq!(
        overflow.category,
        ModelServiceErrorCategory::ContextOverflow
    );
    assert!(!overflow.retryable);
}

#[test]
fn transport_protocol_and_cancellation_failures_remain_distinct() {
    let cases = [
        (
            ProviderErrorContext::new(ProviderFailureKind::Timeout, "header timeout"),
            ModelServiceErrorCategory::Timeout,
            true,
        ),
        (
            ProviderErrorContext::new(ProviderFailureKind::Network, "connection reset"),
            ModelServiceErrorCategory::Network,
            true,
        ),
        (
            ProviderErrorContext::new(ProviderFailureKind::Protocol, "abrupt EOF")
                .with_retryable(true),
            ModelServiceErrorCategory::Protocol,
            true,
        ),
        (
            ProviderErrorContext::new(ProviderFailureKind::Cancelled, "request cancelled")
                .with_retryable(true),
            ModelServiceErrorCategory::Cancelled,
            false,
        ),
    ];

    for (context, category, retryable) in cases {
        let error = normalize(context);
        assert_eq!(error.category, category);
        assert_eq!(error.retryable, retryable);
    }
}

#[test]
fn error_details_are_bounded_and_redacted() {
    let secret = SecretString::new("test-secret-value");
    let context = ProviderErrorContext::new(
        ProviderFailureKind::Http,
        format!("authorization test-secret-value {}", "x".repeat(5_000)),
    )
    .with_http_status(429)
    .with_provider_code("code-test-secret-value")
    .with_retry_after_ms(2_500);
    let error = normalize_provider_error(&context, &[&secret]);

    assert!(!error.message.contains(secret.expose_secret()));
    assert!(error.message.contains("[REDACTED]"));
    assert!(error.message.chars().count() <= 4_096);
    assert_eq!(error.provider_code.as_deref(), Some("code-[REDACTED]"));
    assert_eq!(error.retry_after_ms, Some(2_500));
    assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
}

#[test]
fn resolved_provider_configuration_stays_explicit() {
    let timeouts = ProviderTimeouts::new(
        Duration::from_secs(5),
        Duration::from_secs(15),
        Duration::from_secs(30),
    );
    let headers = BTreeMap::from([("x-client".to_owned(), "pi".to_owned())]);
    let config = ProviderAdapterConfig::new("http://127.0.0.1:8080", timeouts)
        .with_authorization(SecretString::new("Bearer synthetic"))
        .with_headers(headers.clone())
        .with_proxy(SecretString::new("http://proxy.invalid"));

    assert_eq!(config.base_url(), "http://127.0.0.1:8080");
    assert_eq!(config.headers(), &headers);
    assert_eq!(config.timeouts(), timeouts);
    assert_eq!(config.timeouts().connect(), Duration::from_secs(5));
    assert_eq!(
        config.authorization().map(SecretString::expose_secret),
        Some("Bearer synthetic")
    );
    assert_eq!(
        config.proxy().map(SecretString::expose_secret),
        Some("http://proxy.invalid")
    );
}

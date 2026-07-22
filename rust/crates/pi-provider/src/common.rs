use crate::{ProviderErrorContext, ProviderFailureKind, ProviderHttpClient, ProviderHttpResponse};
use pi_model::{Cancellation, ModelServiceError};
use pi_protocol::{Model, Usage};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::SystemTime;

const MAX_ERROR_BODY_BYTES: usize = 65_536;

pub(crate) async fn read_json_http_error(
    http: &ProviderHttpClient,
    response: ProviderHttpResponse,
    cancellation: &dyn Cancellation,
) -> ModelServiceError {
    let status = response.status();
    let retry_after_ms = parse_retry_after_ms(response.headers());
    let body = match response
        .read_body_bounded(MAX_ERROR_BODY_BYTES, cancellation)
        .await
    {
        Ok(body) => body.text_lossy(),
        Err(error) => return error,
    };
    let parsed = serde_json::from_str::<Value>(&body).ok();
    let provider_error = parsed.as_ref().and_then(|value| value.get("error"));
    let message = provider_error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| {
            if body.is_empty() {
                "provider request failed"
            } else {
                &body
            }
        });
    let code = provider_error
        .and_then(|error| {
            error
                .get("code")
                .filter(|code| code.is_string())
                .or_else(|| error.get("type"))
                .or_else(|| error.get("status"))
        })
        .or_else(|| parsed.as_ref().and_then(|value| value.get("code")))
        .and_then(Value::as_str);
    let mut context =
        ProviderErrorContext::new(ProviderFailureKind::Http, message).with_http_status(status);
    if let Some(code) = code {
        context = context.with_provider_code(code);
    }
    if let Some(retry_after_ms) = retry_after_ms {
        context = context.with_retry_after_ms(retry_after_ms);
    }
    http.normalize_error(&context)
}

pub(crate) fn provider_event_error(
    http: &ProviderHttpClient,
    error: &Value,
    fallback: &'static str,
) -> ModelServiceError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(fallback);
    let mut context = ProviderErrorContext::new(ProviderFailureKind::Provider, message);
    if let Some(code) = error
        .get("code")
        .filter(|code| code.is_string())
        .or_else(|| error.get("type"))
        .or_else(|| error.get("status"))
        .and_then(Value::as_str)
    {
        context = context.with_provider_code(code);
    }
    http.normalize_error(&context)
}

pub(crate) fn calculate_cost(model: &Model, usage: &mut Usage) {
    const MILLION: f64 = 1_000_000.0;
    usage.cost.input = usage.input as f64 * model.cost.input / MILLION;
    usage.cost.output = usage.output as f64 * model.cost.output / MILLION;
    usage.cost.cache_read = usage.cache_read as f64 * model.cost.cache_read / MILLION;
    usage.cost.cache_write = usage.cache_write as f64 * model.cost.cache_write / MILLION;
    usage.cost.total =
        usage.cost.input + usage.cost.output + usage.cost.cache_read + usage.cost.cache_write;
}

pub(crate) fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_retry_after_ms(headers: &BTreeMap<String, String>) -> Option<u64> {
    let value = headers.get("retry-after")?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1_000));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    let delay = date.duration_since(SystemTime::now()).ok()?;
    Some(u64::try_from(delay.as_millis()).unwrap_or(u64::MAX))
}

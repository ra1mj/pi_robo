mod support;

use pi_agent::model_request;
use pi_model::{ModelServiceError, ModelServiceErrorCategory};
use pi_protocol::{Message, Usage};
use pi_runtime::{
    CompactionPolicy, InMemorySessionSink, RetryPolicy, Runtime, RuntimeEvent, RuntimePolicies,
    RuntimeRequest, RuntimeStatus,
};
use pi_test_support::{FakeCancellation, FakeClock, FauxModelService, FauxResponse};
use std::sync::Arc;
use std::time::Duration;
use support::{
    FixedClock, FixedCompactor, RecordingEvents, RecordingSleeper, assistant, model, prompt,
};

fn retryable(message: &str) -> ModelServiceError {
    ModelServiceError::new(ModelServiceErrorCategory::Unavailable, message, true)
}

#[test]
fn default_policy_matches_the_captured_typescript_contract() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/runtime/policies.json"))
            .expect("runtime policy fixture");
    let policy = RetryPolicy::default();
    assert_eq!(fixture["retry"]["maxRetries"], policy.max_retries);
    let delays = (1..=policy.max_retries)
        .map(|attempt| {
            policy
                .base_delay
                .saturating_mul(2_u32.saturating_pow(attempt - 1))
                .as_millis()
        })
        .collect::<Vec<_>>();
    let expected = fixture["retry"]["delaysMs"]
        .as_array()
        .expect("delay fixture")
        .iter()
        .map(|value| u128::from(value.as_u64().expect("delay")))
        .collect::<Vec<_>>();
    assert_eq!(delays, expected);
    assert_eq!(fixture["compaction"]["overflowMaxCompactAndRetry"], 1);
}

#[tokio::test]
async fn retries_with_exponential_backoff_and_succeeds() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![
            FauxResponse::Error(retryable("overloaded")),
            FauxResponse::Error(retryable("network")),
            FauxResponse::Message(Box::new(assistant("recovered"))),
        ])
        .expect("queue responses");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("unused", Usage::default());
    let sleeper = RecordingSleeper::default();
    let clock = FixedClock(100);
    let selected_model = model(200_000);
    let runtime = Runtime::new(
        &provider,
        &events,
        &session,
        &compactor,
        &sleeper,
        &clock,
        RuntimePolicies {
            retry: RetryPolicy::default(),
            compaction: CompactionPolicy {
                enabled: false,
                ..CompactionPolicy::default()
            },
        },
    );

    let result = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(selected_model, "test", Vec::new()),
                prompt: Some(prompt("hello", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("runtime result");

    assert_eq!(result.status, RuntimeStatus::Completed);
    assert_eq!(result.retry_attempts, 2);
    assert_eq!(provider.call_count(), 3);
    assert_eq!(
        sleeper.snapshot(),
        vec![Duration::from_secs(2), Duration::from_secs(4)]
    );
    assert_eq!(session.snapshot().expect("session").runs.len(), 3);
    assert!(
        matches!(result.messages.last(), Some(Message::Assistant(message)) if message.error_message.is_none())
    );
    let retry_events = events
        .snapshot()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                RuntimeEvent::AutoRetryStart { .. } | RuntimeEvent::AutoRetryEnd { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        retry_events.as_slice(),
        [
            RuntimeEvent::AutoRetryStart { attempt: 1, .. },
            RuntimeEvent::AutoRetryStart { attempt: 2, .. },
            RuntimeEvent::AutoRetryEnd {
                success: true,
                attempt: 2,
                ..
            }
        ]
    ));
}

#[tokio::test]
async fn respects_retry_after_and_exhausts_the_configured_bound() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![
            FauxResponse::Error(retryable("rate limited").with_retry_after_ms(1_500)),
            FauxResponse::Error(retryable("still unavailable")),
            FauxResponse::Error(retryable("terminal")),
        ])
        .expect("queue responses");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("unused", Usage::default());
    let sleeper = RecordingSleeper::default();
    let clock = FixedClock(100);
    let runtime = Runtime::new(
        &provider,
        &events,
        &session,
        &compactor,
        &sleeper,
        &clock,
        RuntimePolicies {
            retry: RetryPolicy {
                max_retries: 2,
                ..RetryPolicy::default()
            },
            compaction: CompactionPolicy {
                enabled: false,
                ..CompactionPolicy::default()
            },
        },
    );
    let result = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(200_000), "test", Vec::new()),
                prompt: Some(prompt("hello", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("runtime result");

    assert_eq!(result.status, RuntimeStatus::Failed);
    assert_eq!(result.retry_attempts, 2);
    assert_eq!(provider.call_count(), 3);
    assert_eq!(
        sleeper.snapshot(),
        vec![Duration::from_millis(1_500), Duration::from_secs(4)]
    );
    assert!(
        matches!(result.messages.last(), Some(Message::Assistant(message)) if message.error_message.as_deref() == Some("terminal"))
    );
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        RuntimeEvent::AutoRetryEnd {
            success: false,
            attempt: 2,
            final_error: Some(message)
        } if message == "terminal"
    )));
}

#[tokio::test]
async fn does_not_retry_nonretryable_failures_and_cancels_backoff() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![FauxResponse::Error(ModelServiceError::new(
            ModelServiceErrorCategory::Authentication,
            "invalid key",
            false,
        ))])
        .expect("queue response");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("unused", Usage::default());
    let sleeper = RecordingSleeper::default();
    let clock = FixedClock(100);
    let runtime = Runtime::new(
        &provider,
        &events,
        &session,
        &compactor,
        &sleeper,
        &clock,
        RuntimePolicies {
            retry: RetryPolicy::default(),
            compaction: CompactionPolicy {
                enabled: false,
                ..CompactionPolicy::default()
            },
        },
    );
    let result = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(200_000), "test", Vec::new()),
                prompt: Some(prompt("hello", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("runtime result");
    assert_eq!(result.status, RuntimeStatus::Failed);
    assert_eq!(provider.call_count(), 1);
    assert!(sleeper.snapshot().is_empty());

    let cancelling_provider = FauxModelService::new(Arc::new(FakeClock::new(20, 1)), 8);
    cancelling_provider
        .set_responses(vec![FauxResponse::Error(retryable("overloaded"))])
        .expect("queue response");
    let cancelling_sleeper = RecordingSleeper::cancelled();
    let runtime = Runtime::new(
        &cancelling_provider,
        &events,
        &session,
        &compactor,
        &cancelling_sleeper,
        &clock,
        RuntimePolicies {
            retry: RetryPolicy::default(),
            compaction: CompactionPolicy {
                enabled: false,
                ..CompactionPolicy::default()
            },
        },
    );
    let cancelled = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(200_000), "test", Vec::new()),
                prompt: Some(prompt("hello", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("cancelled retry");
    assert_eq!(cancelled.status, RuntimeStatus::Cancelled);
    assert_eq!(cancelling_provider.call_count(), 1);
}

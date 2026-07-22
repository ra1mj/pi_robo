mod support;

use pi_agent::model_request;
use pi_model::{Cancellation, ModelServiceError, ModelServiceErrorCategory};
use pi_protocol::{Message, MessageContent, Usage};
use pi_runtime::{
    CompactionFuture, CompactionPolicy, CompactionReason, CompactionRequest, Compactor,
    InMemorySessionSink, RetryPolicy, Runtime, RuntimeBoundaryError, RuntimeEvent, RuntimePolicies,
    RuntimeRequest, RuntimeStatus,
};
use pi_test_support::{FakeCancellation, FakeClock, FauxModelService, FauxResponse};
use std::sync::Arc;
use support::{
    FixedClock, FixedCompactor, RecordingEvents, RecordingSleeper, assistant, message_text, model,
    prompt,
};

fn summary_usage() -> Usage {
    Usage {
        input: 3,
        output: 2,
        total_tokens: 5,
        ..Usage::default()
    }
}

#[tokio::test]
async fn threshold_compaction_inserts_summary_accounts_usage_and_does_not_start_a_turn() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![FauxResponse::Message(Box::new(assistant(
            "completed response",
        )))])
        .expect("queue response");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("compact summary", summary_usage());
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
                enabled: true,
                reserve_tokens: 99,
                keep_recent_tokens: 1,
            },
        },
    );
    let result = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(100), "test", Vec::new()),
                prompt: Some(prompt("a prompt long enough", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("runtime result");

    assert_eq!(result.status, RuntimeStatus::Completed);
    assert_eq!(
        provider.call_count(),
        1,
        "threshold compaction must not start another turn"
    );
    assert_eq!(compactor.requests().len(), 1);
    assert_eq!(compactor.requests()[0].reason, CompactionReason::Threshold);
    assert_eq!(result.compactions.len(), 1);
    assert!(result.usage.total_tokens >= summary_usage().total_tokens);
    assert!(
        matches!(result.messages.first(), Some(Message::User(message)) if matches!(&message.content, MessageContent::Text(text) if text == "Context summary:\ncompact summary"))
    );
    let snapshot = session.snapshot().expect("session snapshot");
    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.compactions, result.compactions);
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        RuntimeEvent::CompactionEnd {
            reason: CompactionReason::Threshold,
            result: Some(_),
            will_retry: false,
            ..
        }
    )));
}

#[tokio::test]
async fn compacted_history_can_continue_in_a_later_in_memory_run() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![FauxResponse::Message(Box::new(assistant("first")))])
        .expect("queue first response");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("summary", Usage::default());
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
                enabled: true,
                reserve_tokens: 99,
                keep_recent_tokens: 1,
            },
        },
    );
    let first = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(100), "test", Vec::new()),
                prompt: Some(prompt("first prompt", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("first run");
    provider
        .append_responses(vec![FauxResponse::Message(Box::new(assistant("second")))])
        .expect("queue second response");
    let second_runtime = Runtime::new(
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
    let second = second_runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(100), "test", first.messages),
                prompt: Some(prompt("continue", 2)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("second run");
    assert_eq!(second.status, RuntimeStatus::Completed);
    assert_eq!(
        message_text(second.messages.last().expect("last message")),
        Some("second")
    );
    assert_eq!(provider.call_count(), 2);
}

#[tokio::test]
async fn overflow_compacts_and_retries_exactly_once() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![
            FauxResponse::Error(ModelServiceError::new(
                ModelServiceErrorCategory::ContextOverflow,
                "prompt is too long",
                false,
            )),
            FauxResponse::Message(Box::new(assistant("recovered"))),
        ])
        .expect("queue responses");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("overflow summary", summary_usage());
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
                enabled: true,
                reserve_tokens: 10,
                keep_recent_tokens: 1,
            },
        },
    );
    let result = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(100), "test", Vec::new()),
                prompt: Some(prompt("large prompt", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("overflow recovery");
    assert_eq!(result.status, RuntimeStatus::Completed);
    assert_eq!(provider.call_count(), 2);
    assert_eq!(compactor.requests().len(), 1);
    assert_eq!(compactor.requests()[0].reason, CompactionReason::Overflow);
    assert!(result.messages.iter().all(|message| !matches!(message, Message::Assistant(message) if message.error_message.as_deref() == Some("prompt is too long"))));
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        RuntimeEvent::CompactionEnd {
            reason: CompactionReason::Overflow,
            will_retry: true,
            result: Some(_),
            ..
        }
    )));
}

#[tokio::test]
async fn second_overflow_is_terminal_without_another_compaction() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    let overflow = || {
        FauxResponse::Error(ModelServiceError::new(
            ModelServiceErrorCategory::ContextOverflow,
            "prompt is too long",
            false,
        ))
    };
    provider
        .set_responses(vec![overflow(), overflow()])
        .expect("queue responses");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("summary", Usage::default());
    let sleeper = RecordingSleeper::default();
    let clock = FixedClock(100);
    let runtime = Runtime::new(
        &provider,
        &events,
        &session,
        &compactor,
        &sleeper,
        &clock,
        RuntimePolicies::default(),
    );
    let result = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(100), "test", Vec::new()),
                prompt: Some(prompt("large prompt", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("overflow result");
    assert_eq!(result.status, RuntimeStatus::Failed);
    assert_eq!(provider.call_count(), 2);
    assert_eq!(compactor.requests().len(), 1);
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        RuntimeEvent::CompactionEnd { error_message: Some(message), .. }
            if message.contains("after one compact-and-retry attempt")
    )));
}

#[derive(Debug)]
struct CancellingCompactor {
    cancellation: Arc<FakeCancellation>,
}

impl Compactor for CancellingCompactor {
    fn compact<'a>(
        &'a self,
        _request: CompactionRequest,
        _cancellation: &'a dyn Cancellation,
    ) -> CompactionFuture<'a> {
        Box::pin(async move {
            self.cancellation.cancel();
            Err(RuntimeBoundaryError::new("cancelled compaction"))
        })
    }
}

#[tokio::test]
async fn cancellation_during_compaction_is_typed_and_not_persisted() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![FauxResponse::Message(Box::new(assistant("done")))])
        .expect("queue response");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let cancellation = Arc::new(FakeCancellation::default());
    let compactor = CancellingCompactor {
        cancellation: Arc::clone(&cancellation),
    };
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
                enabled: true,
                reserve_tokens: 99,
                keep_recent_tokens: 1,
            },
        },
    );
    let result = runtime
        .run(
            RuntimeRequest {
                model_request: model_request(model(100), "test", Vec::new()),
                prompt: Some(prompt("prompt", 1)),
                tools: Vec::new(),
                parallel_tools: true,
            },
            cancellation.as_ref(),
        )
        .await
        .expect("cancelled compaction result");
    assert_eq!(result.status, RuntimeStatus::Cancelled);
    assert!(session.snapshot().expect("session").compactions.is_empty());
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        RuntimeEvent::CompactionEnd {
            aborted: true,
            result: None,
            ..
        }
    )));
}

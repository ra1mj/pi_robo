mod support;

use pi_agent::model_request;
use pi_model::{ModelServiceError, ModelServiceErrorCategory};
use pi_protocol::{AgentEvent, Message, Usage};
use pi_runtime::{
    CompactionPolicy, InMemorySessionSink, RetryPolicy, Runtime, RuntimeEvent, RuntimePolicies,
    RuntimeRequest,
};
use pi_test_support::{FakeCancellation, FakeClock, FauxModelService, FauxResponse};
use std::sync::Arc;
use support::{
    FixedClock, FixedCompactor, RecordingEvents, RecordingSleeper, assistant, model, prompt,
};

fn normalize(events: &[RuntimeEvent]) -> Vec<String> {
    let mut labels = Vec::new();
    for event in events {
        let label = match event {
            RuntimeEvent::Agent(AgentEvent::AgentStart) => "agent_start".to_owned(),
            RuntimeEvent::Agent(AgentEvent::AgentEnd { .. }) => "agent_end".to_owned(),
            RuntimeEvent::Agent(AgentEvent::TurnStart) => "turn_start".to_owned(),
            RuntimeEvent::Agent(AgentEvent::TurnEnd { .. }) => "turn_end".to_owned(),
            RuntimeEvent::Agent(AgentEvent::MessageStart { message }) => {
                format!("message_start:{}", role(message))
            }
            RuntimeEvent::Agent(AgentEvent::MessageUpdate { .. }) => "message_update".to_owned(),
            RuntimeEvent::Agent(AgentEvent::MessageEnd { message }) => {
                format!("message_end:{}", role(message))
            }
            RuntimeEvent::Agent(AgentEvent::ToolExecutionStart { .. }) => "tool_start".to_owned(),
            RuntimeEvent::Agent(AgentEvent::ToolExecutionUpdate { .. }) => "tool_update".to_owned(),
            RuntimeEvent::Agent(AgentEvent::ToolExecutionEnd { .. }) => "tool_end".to_owned(),
            RuntimeEvent::AutoRetryStart { attempt, .. } => format!("retry_start:{attempt}"),
            RuntimeEvent::AutoRetryEnd {
                success, attempt, ..
            } => format!("retry_end:{attempt}:{success}"),
            RuntimeEvent::CompactionStart { .. } => "compaction_start".to_owned(),
            RuntimeEvent::CompactionEnd { .. } => "compaction_end".to_owned(),
        };
        if label == "message_update" && labels.last() == Some(&label) {
            continue;
        }
        labels.push(label);
    }
    labels
}

fn role(message: &Message) -> &'static str {
    match message {
        Message::User(_) => "user",
        Message::Assistant(_) => "assistant",
        Message::ToolResult(_) => "toolResult",
    }
}

#[tokio::test]
async fn retry_events_wrap_two_fully_settled_agent_runs() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 8);
    provider
        .set_responses(vec![
            FauxResponse::Error(ModelServiceError::new(
                ModelServiceErrorCategory::Unavailable,
                "overloaded",
                true,
            )),
            FauxResponse::Message(Box::new(assistant("recovered"))),
        ])
        .expect("queue responses");
    let events = RecordingEvents::default();
    let session = InMemorySessionSink::default();
    let compactor = FixedCompactor::new("unused", Usage::default());
    let sleeper = RecordingSleeper::default();
    let clock = FixedClock(100);
    Runtime::new(
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
    )
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

    assert_eq!(
        normalize(&events.snapshot()),
        vec![
            "agent_start",
            "turn_start",
            "message_start:user",
            "message_end:user",
            "message_start:assistant",
            "message_end:assistant",
            "turn_end",
            "agent_end",
            "retry_start:1",
            "agent_start",
            "turn_start",
            "message_start:assistant",
            "message_update",
            "message_end:assistant",
            "turn_end",
            "agent_end",
            "retry_end:1:true",
        ]
    );
}

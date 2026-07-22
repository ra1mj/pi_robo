use pi_agent::{
    Agent, AgentRunRequest, Clock, EventFuture, EventSink, EventSinkError, Tool, ToolExecutionMode,
    ToolFuture, ToolOutput, ToolUpdateSink, model_request,
};
use pi_protocol::{
    AgentEvent, AssistantMessage, ContentBlock, Extensions, ImageBlock, Message, MessageContent,
    Model, ModelCost, ModelInput, StopReason, TextBlock, ToolCallBlock, ToolDefinition, Usage,
    UserMessage,
};
use pi_test_support::{FakeCancellation, FakeClock, FauxModelService, FauxResponse};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Default)]
struct CollectingSink {
    events: Mutex<Vec<AgentEvent>>,
    delay_agent_end: bool,
}

impl CollectingSink {
    fn delayed() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            delay_agent_end: true,
        }
    }

    fn snapshot(&self) -> Vec<AgentEvent> {
        self.events.lock().expect("event lock").clone()
    }
}

impl EventSink for CollectingSink {
    fn emit<'a>(&'a self, event: AgentEvent) -> EventFuture<'a> {
        Box::pin(async move {
            if self.delay_agent_end && matches!(event, AgentEvent::AgentEnd { .. }) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            self.events
                .lock()
                .map_err(|_| EventSinkError::new("event lock poisoned"))?
                .push(event);
            Ok(())
        })
    }
}

#[derive(Debug)]
struct FixedClock(u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
struct EchoTool;

impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_owned(),
            description: "Echo text".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
                "additionalProperties": false
            }),
            extensions: Extensions::new(),
        }
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCallBlock,
        _cancellation: &'a dyn pi_model::Cancellation,
        _updates: &'a dyn ToolUpdateSink,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let text = call
                .arguments
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| pi_agent::ToolError::invalid_arguments("text must be a string"))?;
            Ok(ToolOutput::text(format!("echo:{text}")))
        })
    }
}

#[derive(Debug)]
struct DelayTool {
    sequential: bool,
}

impl Tool for DelayTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "delay_echo".to_owned(),
            description: "Echo after a delay".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" },
                    "delayMs": { "type": "integer" }
                },
                "required": ["text", "delayMs"],
                "additionalProperties": false
            }),
            extensions: Extensions::new(),
        }
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        if self.sequential {
            ToolExecutionMode::Sequential
        } else {
            ToolExecutionMode::Parallel
        }
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCallBlock,
        _cancellation: &'a dyn pi_model::Cancellation,
        _updates: &'a dyn ToolUpdateSink,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let text = call
                .arguments
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| pi_agent::ToolError::invalid_arguments("text must be a string"))?;
            let delay_ms = call
                .arguments
                .get("delayMs")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    pi_agent::ToolError::invalid_arguments("delayMs must be an integer")
                })?;
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            Ok(ToolOutput::text(text))
        })
    }
}

#[derive(Debug)]
struct ImageTool;

impl Tool for ImageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "image".to_owned(),
            description: "Return an image".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            extensions: Extensions::new(),
        }
    }

    fn execute<'a>(
        &'a self,
        _call: &'a ToolCallBlock,
        _cancellation: &'a dyn pi_model::Cancellation,
        _updates: &'a dyn ToolUpdateSink,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            Ok(ToolOutput {
                content: vec![ContentBlock::Image(ImageBlock::new(
                    "iVBORw0KGgo=",
                    "image/png",
                ))],
                details: None,
            })
        })
    }
}

#[derive(Debug)]
struct InspectingFaux {
    inner: FauxModelService,
    requests: Mutex<Vec<pi_model::ModelRequest>>,
}

impl pi_model::ModelService for InspectingFaux {
    fn stream<'a>(
        &'a self,
        request: pi_model::ModelRequest,
        cancellation: &'a dyn pi_model::Cancellation,
    ) -> pi_model::ModelFuture<'a> {
        self.requests
            .lock()
            .expect("request lock")
            .push(request.clone());
        pi_model::ModelService::stream(&self.inner, request, cancellation)
    }
}

fn model() -> Model {
    Model {
        id: "faux-1".to_owned(),
        name: "Faux".to_owned(),
        api: "faux".to_owned(),
        provider: "faux".to_owned(),
        base_url: "http://localhost.invalid".to_owned(),
        reasoning: true,
        input: vec![ModelInput::Text, ModelInput::Image],
        cost: ModelCost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
        },
        context_window: 200_000,
        max_tokens: 8_192,
        headers: None,
        compat: None,
        thinking_level_map: None,
        extensions: BTreeMap::new(),
    }
}

fn assistant(content: Vec<ContentBlock>, stop_reason: StopReason) -> AssistantMessage {
    AssistantMessage::new(
        content,
        "faux",
        "faux",
        "faux-1",
        Usage::default(),
        stop_reason,
        0,
    )
}

fn prompt() -> UserMessage {
    UserMessage::new(MessageContent::Text("hi".to_owned()), 1)
}

fn normalize(events: &[AgentEvent]) -> Vec<String> {
    let mut normalized = Vec::new();
    for event in events {
        let label = match event {
            AgentEvent::AgentStart => "agent_start".to_owned(),
            AgentEvent::AgentEnd { .. } => "agent_end".to_owned(),
            AgentEvent::TurnStart => "turn_start".to_owned(),
            AgentEvent::TurnEnd { .. } => "turn_end".to_owned(),
            AgentEvent::MessageStart { message } => {
                format!("message_start:{}", message_role(message))
            }
            AgentEvent::MessageUpdate { .. } => "message_update".to_owned(),
            AgentEvent::MessageEnd { message } => {
                format!("message_end:{}", message_role(message))
            }
            AgentEvent::ToolExecutionStart { tool_name, .. } => {
                format!("tool_execution_start:{tool_name}")
            }
            AgentEvent::ToolExecutionUpdate { tool_name, .. } => {
                format!("tool_execution_update:{tool_name}")
            }
            AgentEvent::ToolExecutionEnd { tool_name, .. } => {
                format!("tool_execution_end:{tool_name}")
            }
        };
        if label == "message_update" && normalized.last() == Some(&label) {
            continue;
        }
        normalized.push(label);
    }
    normalized
}

fn message_role(message: &Message) -> &'static str {
    match message {
        Message::User(_) => "user",
        Message::Assistant(_) => "assistant",
        Message::ToolResult(_) => "toolResult",
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/agent/event-traces.json"))
        .expect("event trace fixture")
}

#[tokio::test]
async fn matches_single_prompt_trace_and_awaits_agent_end() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 2);
    provider
        .set_responses(vec![FauxResponse::Message(Box::new(assistant(
            vec![ContentBlock::Text(TextBlock::new("hello"))],
            StopReason::Stop,
        )))])
        .expect("queue response");
    let sink = CollectingSink::delayed();
    let clock = FixedClock(20);
    let agent = Agent::new(&provider, &sink, &clock);

    let result = agent
        .run(
            AgentRunRequest {
                model_request: model_request(model(), "test", Vec::new()),
                prompt: Some(prompt()),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("agent run");

    assert_eq!(result.messages.len(), 2);
    let expected: Vec<String> =
        serde_json::from_value(fixture()["singlePrompt"].clone()).expect("single prompt trace");
    assert_eq!(normalize(&sink.snapshot()), expected);
    assert!(matches!(
        sink.snapshot().last(),
        Some(AgentEvent::AgentEnd { .. })
    ));
}

#[tokio::test]
async fn completes_parallel_tools_in_completion_order_but_appends_source_order() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 64);
    provider
        .set_responses(vec![
            FauxResponse::Message(Box::new(assistant(
                vec![
                    ContentBlock::ToolCall(ToolCallBlock::new(
                        "first",
                        "delay_echo",
                        json!({ "text": "first", "delayMs": 20 }),
                    )),
                    ContentBlock::ToolCall(ToolCallBlock::new(
                        "second",
                        "delay_echo",
                        json!({ "text": "second", "delayMs": 1 }),
                    )),
                ],
                StopReason::ToolUse,
            ))),
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::Text(TextBlock::new("done"))],
                StopReason::Stop,
            ))),
        ])
        .expect("queue responses");
    let sink = CollectingSink::default();
    let clock = FixedClock(20);
    let agent = Agent::new(&provider, &sink, &clock);

    let result = agent
        .run(
            AgentRunRequest {
                model_request: model_request(model(), "test", Vec::new()),
                prompt: Some(prompt()),
                tools: vec![Arc::new(DelayTool { sequential: false })],
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("agent run");

    let completion = sink
        .snapshot()
        .into_iter()
        .filter_map(|event| match event {
            AgentEvent::ToolExecutionEnd { tool_call_id, .. } => Some(tool_call_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let messages = result
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult(result) => Some(result.tool_call_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expected = &fixture()["parallelResultOrder"];
    let expected_completion: Vec<String> =
        serde_json::from_value(expected["completion"].clone()).expect("completion");
    let expected_messages: Vec<String> =
        serde_json::from_value(expected["messages"].clone()).expect("messages");
    assert_eq!(completion, expected_completion);
    assert_eq!(messages, expected_messages);
}

#[tokio::test]
async fn a_sequential_tool_override_serializes_the_entire_batch() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 64);
    provider
        .set_responses(vec![
            FauxResponse::Message(Box::new(assistant(
                vec![
                    ContentBlock::ToolCall(ToolCallBlock::new(
                        "first",
                        "delay_echo",
                        json!({ "text": "first", "delayMs": 10 }),
                    )),
                    ContentBlock::ToolCall(ToolCallBlock::new(
                        "second",
                        "delay_echo",
                        json!({ "text": "second", "delayMs": 1 }),
                    )),
                ],
                StopReason::ToolUse,
            ))),
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::Text(TextBlock::new("done"))],
                StopReason::Stop,
            ))),
        ])
        .expect("queue responses");
    let sink = CollectingSink::default();
    let clock = FixedClock(20);
    Agent::new(&provider, &sink, &clock)
        .run(
            AgentRunRequest {
                model_request: model_request(model(), "test", Vec::new()),
                prompt: Some(prompt()),
                tools: vec![Arc::new(DelayTool { sequential: true })],
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("agent run");
    let completion = sink
        .snapshot()
        .into_iter()
        .filter_map(|event| match event {
            AgentEvent::ToolExecutionEnd { tool_call_id, .. } => Some(tool_call_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(completion, vec!["first", "second"]);
}

#[tokio::test]
async fn returns_error_results_for_unknown_and_truncated_tool_calls() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 64);
    provider
        .set_responses(vec![
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::ToolCall(ToolCallBlock::new(
                    "unknown",
                    "missing",
                    json!({}),
                ))],
                StopReason::ToolUse,
            ))),
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::ToolCall(ToolCallBlock::new(
                    "truncated",
                    "echo",
                    json!({ "text": "never" }),
                ))],
                StopReason::Length,
            ))),
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::Text(TextBlock::new("done"))],
                StopReason::Stop,
            ))),
        ])
        .expect("queue responses");
    let sink = CollectingSink::default();
    let clock = FixedClock(20);
    let agent = Agent::new(&provider, &sink, &clock);

    let result = agent
        .run(
            AgentRunRequest {
                model_request: model_request(model(), "test", Vec::new()),
                prompt: Some(prompt()),
                tools: vec![Arc::new(EchoTool)],
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("agent run");

    let error_text = result
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult(result) => result.content.first(),
            _ => None,
        })
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        error_text,
        vec![
            fixture()["unknownToolError"].as_str().unwrap(),
            fixture()["lengthToolError"].as_str().unwrap()
        ]
    );
    assert_eq!(provider.call_count(), 3);
}

#[tokio::test]
async fn preserves_image_tool_results_and_invalid_argument_errors() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 64);
    provider
        .set_responses(vec![
            FauxResponse::Message(Box::new(assistant(
                vec![
                    ContentBlock::ToolCall(ToolCallBlock::new("bad", "echo", json!({ "text": 1 }))),
                    ContentBlock::ToolCall(ToolCallBlock::new("image", "image", json!({}))),
                ],
                StopReason::ToolUse,
            ))),
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::Text(TextBlock::new("done"))],
                StopReason::Stop,
            ))),
        ])
        .expect("queue responses");
    let sink = CollectingSink::default();
    let clock = FixedClock(20);
    let agent = Agent::new(&provider, &sink, &clock);
    let result = agent
        .run(
            AgentRunRequest {
                model_request: model_request(model(), "test", Vec::new()),
                prompt: Some(prompt()),
                tools: vec![Arc::new(EchoTool), Arc::new(ImageTool)],
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("agent run");

    let tool_results = result
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(tool_results[0].is_error);
    assert!(
        matches!(tool_results[0].content.first(), Some(ContentBlock::Text(text)) if text.text == "text must be a string")
    );
    assert!(!tool_results[1].is_error);
    assert!(
        matches!(tool_results[1].content.first(), Some(ContentBlock::Image(image)) if image.data == "iVBORw0KGgo=" && image.mime_type == "image/png")
    );
}

#[tokio::test]
async fn cancellation_is_a_typed_terminal_run_and_settles_events() {
    let provider = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 64);
    let sink = CollectingSink::default();
    let clock = FixedClock(20);
    let agent = Agent::new(&provider, &sink, &clock);
    let cancellation = FakeCancellation::default();
    cancellation.cancel();
    let result = agent
        .run(
            AgentRunRequest {
                model_request: model_request(model(), "test", Vec::new()),
                prompt: Some(prompt()),
                tools: Vec::new(),
                parallel_tools: true,
            },
            &cancellation,
        )
        .await
        .expect("cancelled run");
    assert_eq!(result.status, pi_agent::AgentRunStatus::Cancelled);
    assert!(
        matches!(result.messages.last(), Some(Message::Assistant(message)) if message.stop_reason == StopReason::Aborted)
    );
    assert!(matches!(
        sink.snapshot().last(),
        Some(AgentEvent::AgentEnd { .. })
    ));
}

#[tokio::test]
async fn nonvision_requests_omit_images_with_an_explicit_note_but_preserve_history() {
    let inner = FauxModelService::new(Arc::new(FakeClock::new(10, 1)), 64);
    inner
        .set_responses(vec![
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::ToolCall(ToolCallBlock::new(
                    "image",
                    "image",
                    json!({}),
                ))],
                StopReason::ToolUse,
            ))),
            FauxResponse::Message(Box::new(assistant(
                vec![ContentBlock::Text(TextBlock::new("done"))],
                StopReason::Stop,
            ))),
        ])
        .expect("queue responses");
    let provider = InspectingFaux {
        inner,
        requests: Mutex::new(Vec::new()),
    };
    let sink = CollectingSink::default();
    let clock = FixedClock(20);
    let mut text_model = model();
    text_model.input = vec![ModelInput::Text];
    let result = Agent::new(&provider, &sink, &clock)
        .run(
            AgentRunRequest {
                model_request: model_request(text_model, "test", Vec::new()),
                prompt: Some(prompt()),
                tools: vec![Arc::new(ImageTool)],
                parallel_tools: true,
            },
            &FakeCancellation::default(),
        )
        .await
        .expect("agent run");
    assert!(result.messages.iter().any(|message| matches!(
        message,
        Message::ToolResult(result)
            if result.content.iter().any(|block| matches!(block, ContentBlock::Image(_)))
    )));
    let requests = provider.requests.lock().expect("requests");
    let model_tool_result = requests[1]
        .messages
        .iter()
        .find_map(|message| match message {
            Message::ToolResult(result) => Some(result),
            _ => None,
        })
        .expect("model tool result");
    assert!(
        model_tool_result
            .content
            .iter()
            .all(|block| !matches!(block, ContentBlock::Image(_)))
    );
    assert!(model_tool_result.content.iter().any(|block| matches!(
        block,
        ContentBlock::Text(text) if text.text.contains("does not support images")
    )));
}

use futures_util::StreamExt;
use pi_model::{
    CacheRetention, ModelRequest, ModelRequestOptions, ModelService, ModelServiceErrorCategory,
    ThinkingLevel, ToolChoice,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, CompletionReason, ContentBlock, Extensions,
    ImageBlock, Message, MessageContent, Model, ModelCost, ModelInput, StopReason, ThinkingBlock,
    ToolCallBlock, ToolDefinition, ToolResultMessage, Usage, UserMessage,
};
use pi_provider::{
    AnthropicMessagesAdapter, ProviderAdapterConfig, ProviderClock, ProviderTimeouts,
};
use pi_test_support::{AsyncLocalHttpServer, FakeCancellation};
use serde_json::{Value, json};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

struct FixedClock(u64);

impl ProviderClock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

fn timeouts() -> ProviderTimeouts {
    ProviderTimeouts::new(
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
}

fn model() -> Model {
    Model {
        id: "claude-model".to_owned(),
        name: "Claude Model".to_owned(),
        api: "anthropic-messages".to_owned(),
        provider: "anthropic".to_owned(),
        base_url: "https://api.anthropic.com/v1".to_owned(),
        reasoning: true,
        input: vec![ModelInput::Text, ModelInput::Image],
        cost: ModelCost {
            input: 1.0,
            output: 2.0,
            cache_read: 0.5,
            cache_write: 1.5,
        },
        context_window: 200_000,
        max_tokens: 4_096,
        headers: None,
        compat: Some(json!({
            "forceAdaptiveThinking": true,
            "supportsEagerToolInputStreaming": true,
            "supportsCacheControlOnTools": true
        })),
        thinking_level_map: None,
        extensions: Extensions::new(),
    }
}

fn request() -> ModelRequest {
    let mut thinking = ThinkingBlock::new("prior reasoning");
    thinking.thinking_signature = Some("signed-reasoning".to_owned());
    let tool_call = ToolCallBlock::new("toolu_1", "inspect", json!({}));
    let assistant = AssistantMessage::new(
        vec![
            ContentBlock::Thinking(thinking),
            ContentBlock::ToolCall(tool_call),
        ],
        "anthropic-messages",
        "anthropic",
        "claude-model",
        Usage::default(),
        StopReason::ToolUse,
        2,
    );
    let tool_result = ToolResultMessage::new(
        "toolu_1",
        "inspect",
        vec![
            ContentBlock::Text(pi_protocol::TextBlock::new("red circle")),
            ContentBlock::Image(ImageBlock::new("aW1hZ2U=", "image/png")),
        ],
        false,
        3,
    );
    ModelRequest {
        model: model(),
        system_prompt: "Use tools precisely.".to_owned(),
        messages: vec![
            Message::User(UserMessage::new(
                MessageContent::Text("read the file".to_owned()),
                1,
            )),
            Message::Assistant(assistant),
            Message::ToolResult(tool_result),
        ],
        tools: vec![ToolDefinition {
            name: "read".to_owned(),
            description: "Read a file".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
            extensions: Extensions::new(),
        }],
        options: ModelRequestOptions {
            temperature: Some(0.2),
            max_tokens: None,
            reasoning: Some(ThinkingLevel::High),
            thinking_budgets: None,
            cache_retention: CacheRetention::Long,
            session_id: None,
            tool_choice: Some(ToolChoice::Required),
        },
    }
}

fn request_json(request: &[u8]) -> Result<Value, Box<dyn Error>> {
    let body_start = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("request has no body separator")?
        + 4;
    Ok(serde_json::from_slice(&request[body_start..])?)
}

fn event_name(event: &AssistantMessageEvent) -> &'static str {
    match event {
        AssistantMessageEvent::Start { .. } => "start",
        AssistantMessageEvent::TextStart { .. } => "text_start",
        AssistantMessageEvent::TextDelta { .. } => "text_delta",
        AssistantMessageEvent::TextEnd { .. } => "text_end",
        AssistantMessageEvent::ThinkingStart { .. } => "thinking_start",
        AssistantMessageEvent::ThinkingDelta { .. } => "thinking_delta",
        AssistantMessageEvent::ThinkingEnd { .. } => "thinking_end",
        AssistantMessageEvent::ToolCallStart { .. } => "toolcall_start",
        AssistantMessageEvent::ToolCallDelta { .. } => "toolcall_delta",
        AssistantMessageEvent::ToolCallEnd { .. } => "toolcall_end",
        AssistantMessageEvent::Done { .. } => "done",
        AssistantMessageEvent::Error { .. } => "error",
    }
}

#[tokio::test]
async fn maps_signed_history_images_cache_and_streams_thinking_usage() -> Result<(), Box<dyn Error>>
{
    let body = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"routed-claude\",\"usage\":{\"input_tokens\":12,\"output_tokens\":0,\"cache_read_input_tokens\":2,\"cache_creation_input_tokens\":3,\"cache_creation\":{\"ephemeral_1h_input_tokens\":1}}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"think\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"signature-1\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":12,\"output_tokens\":5,\"cache_read_input_tokens\":2,\"cache_creation_input_tokens\":3,\"output_tokens_details\":{\"thinking_tokens\":4}}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
    )
    .to_owned();
    let server =
        AsyncLocalHttpServer::start("200 OK", &[("Content-Type", "text/event-stream")], body)
            .await?;
    let base_url = server.base_url().to_owned();
    let adapter = AnthropicMessagesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?
        .with_clock(Arc::new(FixedClock(91)));
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item?);
    }
    assert_eq!(
        events.iter().map(event_name).collect::<Vec<_>>(),
        vec![
            "start",
            "thinking_start",
            "thinking_delta",
            "thinking_end",
            "done"
        ]
    );
    let AssistantMessageEvent::Done { reason, message } = events.last().expect("done event") else {
        panic!("last event must be done");
    };
    assert_eq!(*reason, CompletionReason::Stop);
    assert_eq!(message.timestamp, 91);
    assert_eq!(message.response_id.as_deref(), Some("msg_1"));
    assert_eq!(message.response_model.as_deref(), Some("routed-claude"));
    assert_eq!(message.usage.input, 12);
    assert_eq!(message.usage.output, 5);
    assert_eq!(message.usage.cache_read, 2);
    assert_eq!(message.usage.cache_write, 3);
    assert_eq!(message.usage.cache_write_1h, Some(1));
    assert_eq!(message.usage.reasoning, Some(4));
    assert_eq!(message.usage.total_tokens, 22);
    assert!(matches!(
        message.content.first(),
        Some(ContentBlock::Thinking(block))
            if block.thinking == "think"
                && block.thinking_signature.as_deref() == Some("signature-1")
    ));

    let captured = server.finish().await?;
    let captured_text = String::from_utf8_lossy(&captured).to_ascii_lowercase();
    assert!(captured_text.starts_with("post /v1/messages http/1.1"));
    assert!(captured_text.contains("anthropic-version: 2023-06-01"));
    let payload = request_json(&captured)?;
    assert_eq!(payload["model"], "claude-model");
    assert_eq!(payload["max_tokens"], 4_096);
    assert_eq!(
        payload["thinking"],
        json!({ "type": "adaptive", "display": "summarized" })
    );
    assert_eq!(payload["output_config"], json!({ "effort": "high" }));
    assert_eq!(payload["tool_choice"], json!({ "type": "any" }));
    assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
    assert_eq!(payload["tools"][0]["eager_input_streaming"], true);
    assert_eq!(
        payload["tools"][0]["input_schema"]["required"],
        json!(["path"])
    );
    assert_eq!(payload["tools"][0]["cache_control"]["ttl"], "1h");
    assert_eq!(payload["messages"][1]["content"][0]["type"], "thinking");
    assert_eq!(
        payload["messages"][1]["content"][0]["signature"],
        "signed-reasoning"
    );
    assert_eq!(payload["messages"][1]["content"][1]["type"], "tool_use");
    assert_eq!(payload["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(
        payload["messages"][2]["content"][0]["content"][1]["source"]["media_type"],
        "image/png"
    );
    Ok(())
}

#[tokio::test]
async fn streams_eager_partial_tool_input() -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_tool\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_new\",\"name\":\"read\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"README.md\\\"}\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
    )
    .to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = AnthropicMessagesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut tool_call = None;
    let mut done_reason = None;
    while let Some(item) = stream.next().await {
        match item? {
            AssistantMessageEvent::ToolCallEnd {
                tool_call: call, ..
            } => tool_call = Some(call),
            AssistantMessageEvent::Done { reason, .. } => done_reason = Some(reason),
            _ => {}
        }
    }
    let tool_call = tool_call.expect("tool call end");
    assert_eq!(tool_call.id, "toolu_new");
    assert_eq!(tool_call.name, "read");
    assert_eq!(tool_call.arguments, json!({ "path": "README.md" }));
    assert_eq!(done_reason, Some(CompletionReason::ToolUse));
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_stream_truncated_before_message_stop() -> Result<(), Box<dyn Error>> {
    let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_truncated\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n".to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = AnthropicMessagesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut terminal_error = None;
    while let Some(item) = stream.next().await {
        if let Err(error) = item {
            terminal_error = Some(error);
        }
    }
    let error = terminal_error.expect("truncated stream must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::Protocol);
    assert!(error.retryable);
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn normalizes_anthropic_http_errors() -> Result<(), Box<dyn Error>> {
    let body = json!({
        "type": "error",
        "error": { "type": "overloaded_error", "message": "overloaded" }
    })
    .to_string();
    let server =
        AsyncLocalHttpServer::start("529 Overloaded", &[("Retry-After", "1")], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = AnthropicMessagesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let error = match adapter.stream(request(), &cancellation).await {
        Ok(_) => panic!("HTTP 529 must fail before streaming"),
        Err(error) => error,
    };
    assert_eq!(error.category, ModelServiceErrorCategory::Server);
    assert!(error.retryable);
    assert_eq!(error.http_status, Some(529));
    assert_eq!(error.provider_code.as_deref(), Some("overloaded_error"));
    assert_eq!(error.retry_after_ms, Some(1_000));
    server.finish().await?;
    Ok(())
}

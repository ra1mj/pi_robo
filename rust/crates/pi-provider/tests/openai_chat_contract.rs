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
    OpenAiChatAdapter, ProviderAdapterConfig, ProviderClock, ProviderTimeouts, SecretString,
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
        id: "chat-model".to_owned(),
        name: "Chat Model".to_owned(),
        api: "openai-completions".to_owned(),
        provider: "openai".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        reasoning: true,
        input: vec![ModelInput::Text, ModelInput::Image],
        cost: ModelCost {
            input: 1.0,
            output: 2.0,
            cache_read: 0.5,
            cache_write: 1.5,
        },
        context_window: 128_000,
        max_tokens: 4_096,
        headers: None,
        compat: None,
        thinking_level_map: None,
        extensions: Extensions::new(),
    }
}

fn request() -> ModelRequest {
    let mut thinking = ThinkingBlock::new("prior thought");
    thinking.thinking_signature = Some("reasoning_content".to_owned());
    let prior_tool = ToolCallBlock::new("prior-call", "read", json!({ "path": "old.txt" }));
    let assistant = AssistantMessage::new(
        vec![
            ContentBlock::Thinking(thinking),
            ContentBlock::ToolCall(prior_tool),
        ],
        "openai-completions",
        "openai",
        "chat-model",
        Usage::default(),
        StopReason::ToolUse,
        2,
    );
    let empty_result = ToolResultMessage::new("prior-call", "read", Vec::new(), false, 3);
    let image_result = ToolResultMessage::new(
        "image-call",
        "screenshot",
        vec![ContentBlock::Image(ImageBlock::new(
            "aW1hZ2U=",
            "image/png",
        ))],
        false,
        4,
    );
    ModelRequest {
        model: model(),
        system_prompt: "Be concise.".to_owned(),
        messages: vec![
            Message::User(UserMessage::new(
                MessageContent::Blocks(vec![
                    ContentBlock::Text(pi_protocol::TextBlock::new("hello")),
                    ContentBlock::Image(ImageBlock::new("aW1hZ2U=", "image/png")),
                ]),
                1,
            )),
            Message::Assistant(assistant),
            Message::ToolResult(empty_result),
            Message::ToolResult(image_result),
        ],
        tools: vec![ToolDefinition {
            name: "read".to_owned(),
            description: "Read a file".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }),
            extensions: Extensions::new(),
        }],
        options: ModelRequestOptions {
            temperature: Some(0.25),
            max_tokens: Some(128),
            reasoning: Some(ThinkingLevel::High),
            thinking_budgets: None,
            cache_retention: CacheRetention::Long,
            session_id: Some("session-chat-1".to_owned()),
            tool_choice: Some(ToolChoice::Named("read".to_owned())),
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
async fn maps_request_and_streams_mixed_content_usage_and_tool_call() -> Result<(), Box<dyn Error>>
{
    let body = concat!(
        "data: {\"id\":\"chatcmpl-1\",\"model\":\"routed-model\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think \"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-1\",\"model\":\"routed-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"answer\",\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":8,\"prompt_tokens_details\":{\"cached_tokens\":5,\"cache_write_tokens\":3},\"completion_tokens_details\":{\"reasoning_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_owned();
    let server =
        AsyncLocalHttpServer::start("200 OK", &[("Content-Type", "text/event-stream")], body)
            .await?;
    let base_url = server.base_url().to_owned();
    let config = ProviderAdapterConfig::new(base_url, timeouts())
        .with_authorization(SecretString::new("Bearer synthetic-token"));
    let adapter = OpenAiChatAdapter::new(&config)?.with_clock(Arc::new(FixedClock(42)));
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event?);
    }

    assert_eq!(
        events.iter().map(event_name).collect::<Vec<_>>(),
        vec![
            "start",
            "thinking_start",
            "thinking_delta",
            "text_start",
            "text_delta",
            "toolcall_start",
            "toolcall_delta",
            "thinking_end",
            "text_end",
            "toolcall_end",
            "done",
        ]
    );
    let AssistantMessageEvent::Done { reason, message } = events.last().expect("done event") else {
        panic!("last event must be done");
    };
    assert_eq!(*reason, CompletionReason::ToolUse);
    assert_eq!(message.timestamp, 42);
    assert_eq!(message.response_id.as_deref(), Some("chatcmpl-1"));
    assert_eq!(message.response_model.as_deref(), Some("routed-model"));
    assert_eq!(message.usage.input, 12);
    assert_eq!(message.usage.output, 8);
    assert_eq!(message.usage.cache_read, 5);
    assert_eq!(message.usage.cache_write, 3);
    assert_eq!(message.usage.reasoning, Some(2));
    assert_eq!(message.usage.total_tokens, 28);
    assert!(matches!(
        message.content.get(2),
        Some(ContentBlock::ToolCall(block))
            if block.id == "call_1"
                && block.name == "read"
                && block.arguments == json!({ "path": "README.md" })
    ));

    let captured = server.finish().await?;
    let captured_text = String::from_utf8_lossy(&captured);
    assert!(captured_text.starts_with("POST /v1/chat/completions HTTP/1.1"));
    let payload = request_json(&captured)?;
    assert_eq!(payload["model"], "chat-model");
    assert_eq!(payload["max_completion_tokens"], 128);
    assert_eq!(payload["temperature"], 0.25);
    assert_eq!(payload["reasoning_effort"], "high");
    assert_eq!(payload["prompt_cache_key"], "session-chat-1");
    assert_eq!(payload["prompt_cache_retention"], "24h");
    assert_eq!(
        payload["tool_choice"],
        json!({
            "type": "function",
            "function": { "name": "read" }
        })
    );
    assert_eq!(payload["messages"][0]["role"], "developer");
    assert_eq!(
        payload["messages"][1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,aW1hZ2U="
    );
    assert_eq!(payload["messages"][3]["content"], "(no tool output)");
    assert_eq!(payload["messages"][4]["content"], "(see attached image)");
    assert_eq!(
        payload["messages"][5]["content"][1]["image_url"]["url"],
        "data:image/png;base64,aW1hZ2U="
    );
    Ok(())
}

#[tokio::test]
async fn blocks_truncated_tool_arguments_even_with_length_finish() -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_bad\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":\"length\"}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiChatAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut names = Vec::new();
    let mut terminal_error = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => names.push(event_name(&event)),
            Err(error) => terminal_error = Some(error),
        }
    }
    let error = terminal_error.expect("truncated arguments must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::Protocol);
    assert!(!error.retryable);
    assert!(!names.contains(&"toolcall_end"));
    assert!(!names.contains(&"done"));
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn coalesces_parallel_partial_calls_by_stable_index() -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"read-initial\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"README\"}},{\"index\":1,\"id\":\"grep-initial\",\"function\":{\"name\":\"grep\",\"arguments\":\"{\\\"pattern\\\":\\\"TODO\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"grep-mutated\",\"function\":{\"arguments\":\"\\\",\\\"path\\\":\\\"src\\\"}\"}},{\"index\":0,\"id\":\"read-mutated\",\"function\":{\"arguments\":\".md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiChatAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut ended = Vec::new();
    let mut done = None;
    while let Some(item) = stream.next().await {
        match item? {
            AssistantMessageEvent::ToolCallEnd {
                content_index,
                tool_call,
                ..
            } => ended.push((content_index, tool_call)),
            AssistantMessageEvent::Done { message, .. } => done = Some(message),
            _ => {}
        }
    }

    assert_eq!(ended.len(), 2);
    assert_eq!(ended[0].0, 0);
    assert_eq!(ended[0].1.id, "read-initial");
    assert_eq!(ended[0].1.arguments, json!({ "path": "README.md" }));
    assert_eq!(ended[1].0, 1);
    assert_eq!(ended[1].1.id, "grep-initial");
    assert_eq!(
        ended[1].1.arguments,
        json!({ "pattern": "TODO", "path": "src" })
    );
    assert_eq!(done.expect("done message").content.len(), 2);
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_eof_without_finish_reason_as_retryable_protocol_error()
-> Result<(), Box<dyn Error>> {
    let body = "data: {\"id\":\"chatcmpl-truncated\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n".to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiChatAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut terminal_error = None;
    while let Some(item) = stream.next().await {
        if let Err(error) = item {
            terminal_error = Some(error);
        }
    }
    let error = terminal_error.expect("missing finish reason must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::Protocol);
    assert!(error.retryable);
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn normalizes_http_errors_before_stream_start() -> Result<(), Box<dyn Error>> {
    let body = json!({
        "error": {
            "message": "slow down",
            "code": "rate_limit"
        }
    })
    .to_string();
    let server =
        AsyncLocalHttpServer::start("429 Too Many Requests", &[("Retry-After", "2")], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiChatAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let error = match adapter.stream(request(), &cancellation).await {
        Ok(_) => panic!("HTTP 429 must fail before streaming"),
        Err(error) => error,
    };
    assert_eq!(error.category, ModelServiceErrorCategory::RateLimit);
    assert!(error.retryable);
    assert_eq!(error.http_status, Some(429));
    assert_eq!(error.provider_code.as_deref(), Some("rate_limit"));
    assert_eq!(error.retry_after_ms, Some(2_000));
    server.finish().await?;
    Ok(())
}

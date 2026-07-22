use futures_util::StreamExt;
use pi_model::{
    CacheRetention, ModelRequest, ModelRequestOptions, ModelService, ModelServiceErrorCategory,
    ThinkingLevel, ToolChoice,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, CompletionReason, ContentBlock, Extensions,
    ImageBlock, Message, MessageContent, Model, ModelCost, ModelInput, StopReason, ToolCallBlock,
    ToolDefinition, ToolResultMessage, Usage, UserMessage,
};
use pi_provider::{
    GoogleGenerativeLanguageAdapter, ProviderAdapterConfig, ProviderClock, ProviderTimeouts,
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
        id: "gemini-3-pro-preview".to_owned(),
        name: "Gemini 3 Pro".to_owned(),
        api: "google-generative-ai".to_owned(),
        provider: "google".to_owned(),
        base_url: "https://generativelanguage.googleapis.com/v1beta".to_owned(),
        reasoning: true,
        input: vec![ModelInput::Text, ModelInput::Image],
        cost: ModelCost {
            input: 1.0,
            output: 2.0,
            cache_read: 0.5,
            cache_write: 0.0,
        },
        context_window: 1_000_000,
        max_tokens: 8_192,
        headers: None,
        compat: None,
        thinking_level_map: None,
        extensions: Extensions::new(),
    }
}

fn request() -> ModelRequest {
    let mut prior_call = ToolCallBlock::new("call_1", "inspect", json!({}));
    prior_call.thought_signature = Some("QUFBQQ==".to_owned());
    let assistant = AssistantMessage::new(
        vec![ContentBlock::ToolCall(prior_call)],
        "google-generative-ai",
        "google",
        "gemini-3-pro-preview",
        Usage::default(),
        StopReason::ToolUse,
        2,
    );
    let result = ToolResultMessage::new(
        "call_1",
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
                MessageContent::Blocks(vec![
                    ContentBlock::Text(pi_protocol::TextBlock::new("inspect")),
                    ContentBlock::Image(ImageBlock::new("aW1hZ2U=", "image/png")),
                ]),
                1,
            )),
            Message::Assistant(assistant),
            Message::ToolResult(result),
        ],
        tools: vec![ToolDefinition {
            name: "inspect".to_owned(),
            description: "Inspect input".to_owned(),
            parameters: json!({ "type": "object", "properties": {} }),
            extensions: Extensions::new(),
        }],
        options: ModelRequestOptions {
            temperature: Some(0.3),
            max_tokens: Some(128),
            reasoning: Some(ThinkingLevel::High),
            thinking_budgets: None,
            cache_retention: CacheRetention::None,
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

#[tokio::test]
async fn maps_multimodal_history_and_streams_thinking_text_tool_usage() -> Result<(), Box<dyn Error>>
{
    let body = concat!(
        "data: {\"responseId\":\"google-resp-1\",\"modelVersion\":\"routed-gemini\",\"candidates\":[{\"content\":{\"parts\":[{\"thought\":true,\"text\":\"think\",\"thoughtSignature\":\"QUFBQQ==\"}]}}]}\n\n",
        "data: {\"responseId\":\"google-resp-1\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"answer\"},{\"functionCall\":{\"id\":\"call_new\",\"name\":\"inspect\",\"args\":{}},\"thoughtSignature\":\"QUFBQQ==\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":20,\"candidatesTokenCount\":5,\"thoughtsTokenCount\":3,\"cachedContentTokenCount\":4,\"totalTokenCount\":28}}\n\n"
    )
    .to_owned();
    let server =
        AsyncLocalHttpServer::start("200 OK", &[("Content-Type", "text/event-stream")], body)
            .await?;
    let base_url = server.base_url().to_owned();
    let adapter =
        GoogleGenerativeLanguageAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?
            .with_clock(Arc::new(FixedClock(123)));
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut done = None;
    while let Some(item) = stream.next().await {
        if let AssistantMessageEvent::Done { reason, message } = item? {
            done = Some((reason, message));
        }
    }
    let (reason, message) = done.expect("done event");
    assert_eq!(reason, CompletionReason::ToolUse);
    assert_eq!(message.timestamp, 123);
    assert_eq!(message.response_id.as_deref(), Some("google-resp-1"));
    assert_eq!(message.response_model.as_deref(), Some("routed-gemini"));
    assert_eq!(message.usage.input, 16);
    assert_eq!(message.usage.output, 8);
    assert_eq!(message.usage.cache_read, 4);
    assert_eq!(message.usage.reasoning, Some(3));
    assert_eq!(message.usage.total_tokens, 28);
    assert!(matches!(
        message.content.first(),
        Some(ContentBlock::Thinking(block))
            if block.thinking == "think"
                && block.thinking_signature.as_deref() == Some("QUFBQQ==")
    ));
    assert!(matches!(
        message.content.get(1),
        Some(ContentBlock::Text(block)) if block.text == "answer"
    ));
    assert!(matches!(
        message.content.get(2),
        Some(ContentBlock::ToolCall(block))
            if block.id == "call_new"
                && block.name == "inspect"
                && block.thought_signature.as_deref() == Some("QUFBQQ==")
    ));

    let captured = server.finish().await?;
    assert!(String::from_utf8_lossy(&captured).starts_with(
        "POST /v1/models/gemini-3-pro-preview:streamGenerateContent?alt=sse HTTP/1.1"
    ));
    let payload = request_json(&captured)?;
    assert_eq!(payload["model"], "gemini-3-pro-preview");
    assert_eq!(
        payload["config"]["systemInstruction"],
        "Use tools precisely."
    );
    assert_eq!(payload["config"]["temperature"], 0.3);
    assert_eq!(payload["config"]["maxOutputTokens"], 128);
    assert_eq!(
        payload["config"]["thinkingConfig"],
        json!({ "includeThoughts": true, "thinkingLevel": "HIGH" })
    );
    assert_eq!(
        payload["config"]["toolConfig"]["functionCallingConfig"],
        json!({ "mode": "ANY" })
    );
    assert_eq!(
        payload["contents"][0]["parts"][1]["inlineData"]["mimeType"],
        "image/png"
    );
    assert_eq!(payload["contents"][1]["role"], "model");
    assert_eq!(
        payload["contents"][1]["parts"][0]["functionCall"]["name"],
        "inspect"
    );
    assert!(
        payload["contents"][1]["parts"][0]["functionCall"]
            .get("id")
            .is_none()
    );
    assert_eq!(
        payload["contents"][1]["parts"][0]["thoughtSignature"],
        "QUFBQQ=="
    );
    assert_eq!(
        payload["contents"][2]["parts"][0]["functionResponse"]["response"]["output"],
        "red circle"
    );
    assert_eq!(
        payload["contents"][2]["parts"][0]["functionResponse"]["parts"][0]["inlineData"]["data"],
        "aW1hZ2U="
    );
    Ok(())
}

#[tokio::test]
async fn generates_deterministic_id_when_google_omits_one() -> Result<(), Box<dyn Error>> {
    let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"inspect\",\"args\":{}},\"thoughtSignature\":\"QUFBQQ==\"}]},\"finishReason\":\"STOP\"}]}\n\n".to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter =
        GoogleGenerativeLanguageAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?
            .with_clock(Arc::new(FixedClock(456)));
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut id = None;
    while let Some(item) = stream.next().await {
        if let AssistantMessageEvent::ToolCallEnd { tool_call, .. } = item? {
            id = Some(tool_call.id);
        }
    }
    assert_eq!(id.as_deref(), Some("inspect_456_1"));
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_safety_block_as_invalid_request() -> Result<(), Box<dyn Error>> {
    let body = "data: {\"candidates\":[{\"finishReason\":\"SAFETY\"}]}\n\n".to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter =
        GoogleGenerativeLanguageAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut terminal_error = None;
    while let Some(item) = stream.next().await {
        if let Err(error) = item {
            terminal_error = Some(error);
        }
    }
    let error = terminal_error.expect("safety block must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::InvalidRequest);
    assert!(!error.retryable);
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_eof_without_finish_reason() -> Result<(), Box<dyn Error>> {
    let body = "data: {\"responseId\":\"google-truncated\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial\"}]}}]}\n\n".to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter =
        GoogleGenerativeLanguageAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
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
async fn normalizes_google_http_errors() -> Result<(), Box<dyn Error>> {
    let body = json!({
        "error": {
            "code": 503,
            "message": "service unavailable",
            "status": "UNAVAILABLE"
        }
    })
    .to_string();
    let server =
        AsyncLocalHttpServer::start("503 Service Unavailable", &[("Retry-After", "2")], body)
            .await?;
    let base_url = server.base_url().to_owned();
    let adapter =
        GoogleGenerativeLanguageAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let error = match adapter.stream(request(), &cancellation).await {
        Ok(_) => panic!("HTTP 503 must fail before streaming"),
        Err(error) => error,
    };
    assert_eq!(error.category, ModelServiceErrorCategory::Unavailable);
    assert!(error.retryable);
    assert_eq!(error.provider_code.as_deref(), Some("UNAVAILABLE"));
    assert_eq!(error.retry_after_ms, Some(2_000));
    server.finish().await?;
    Ok(())
}

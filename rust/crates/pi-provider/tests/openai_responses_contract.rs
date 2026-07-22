use futures_util::StreamExt;
use pi_model::{
    CacheRetention, ModelRequest, ModelRequestOptions, ModelService, ModelServiceErrorCategory,
    ThinkingLevel, ToolChoice,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, CompletionReason, ContentBlock, Extensions,
    ImageBlock, Message, MessageContent, Model, ModelCost, ModelInput, StopReason, TextBlock,
    ThinkingBlock, ToolCallBlock, ToolDefinition, ToolResultMessage, Usage, UserMessage,
};
use pi_provider::{OpenAiResponsesAdapter, ProviderAdapterConfig, ProviderClock, ProviderTimeouts};
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
        id: "responses-model".to_owned(),
        name: "Responses Model".to_owned(),
        api: "openai-responses".to_owned(),
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
    thinking.thinking_signature = Some(
        json!({
            "type": "reasoning",
            "id": "rs_prior",
            "summary": [{ "type": "summary_text", "text": "prior thought" }],
            "encrypted_content": "prior-opaque"
        })
        .to_string(),
    );
    let mut text = TextBlock::new("prior answer");
    text.text_signature =
        Some(json!({ "v": 1, "id": "msg_prior", "phase": "commentary" }).to_string());
    let inspect = ToolCallBlock::new("call_1|fc_1", "inspect", json!({}));
    let empty = ToolCallBlock::new("call_empty|fc_empty", "empty", json!({}));
    let assistant = AssistantMessage::new(
        vec![
            ContentBlock::Thinking(thinking),
            ContentBlock::Text(text),
            ContentBlock::ToolCall(inspect),
            ContentBlock::ToolCall(empty),
        ],
        "openai-responses",
        "openai",
        "responses-model",
        Usage::default(),
        StopReason::ToolUse,
        2,
    );
    let image_result = ToolResultMessage::new(
        "call_1|fc_1",
        "inspect",
        vec![
            ContentBlock::Text(TextBlock::new("red circle")),
            ContentBlock::Image(ImageBlock::new("aW1hZ2U=", "image/png")),
        ],
        false,
        3,
    );
    let empty_result = ToolResultMessage::new("call_empty|fc_empty", "empty", Vec::new(), false, 4);
    ModelRequest {
        model: model(),
        system_prompt: "Be concise.".to_owned(),
        messages: vec![
            Message::User(UserMessage::new(
                MessageContent::Text("hello".to_owned()),
                1,
            )),
            Message::Assistant(assistant),
            Message::ToolResult(image_result),
            Message::ToolResult(empty_result),
        ],
        tools: vec![ToolDefinition {
            name: "inspect".to_owned(),
            description: "Inspect an image".to_owned(),
            parameters: json!({ "type": "object", "properties": {} }),
            extensions: Extensions::new(),
        }],
        options: ModelRequestOptions {
            temperature: Some(0.2),
            max_tokens: Some(8),
            reasoning: Some(ThinkingLevel::High),
            thinking_budgets: None,
            cache_retention: CacheRetention::Long,
            session_id: Some("session-responses-1".to_owned()),
            tool_choice: Some(ToolChoice::Named("inspect".to_owned())),
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
async fn maps_replay_input_and_streams_reasoning_text_tool_and_usage() -> Result<(), Box<dyn Error>>
{
    let body = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"routed-responses-model\"}}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[]}}\n\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"output_index\":0,\"delta\":\"think\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"think\"}]}}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"delta\":\"answer\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"answer\",\"annotations\":[]}]}}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_new\",\"call_id\":\"call_new\",\"name\":\"inspect\",\"arguments\":\"\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\"{\\\"path\\\":\"}\n\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":2,\"arguments\":\"{\\\"path\\\":\\\"image.png\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_new\",\"call_id\":\"call_new\",\"name\":\"inspect\",\"arguments\":\"{\\\"path\\\":\\\"image.png\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"routed-responses-model\",\"status\":\"completed\",\"output\":[{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"think\"}],\"encrypted_content\":\"opaque\"}],\"usage\":{\"input_tokens\":20,\"output_tokens\":7,\"total_tokens\":27,\"input_tokens_details\":{\"cached_tokens\":2,\"cache_write_tokens\":3},\"output_tokens_details\":{\"reasoning_tokens\":4}}}}\n\n"
    )
    .to_owned();
    let server =
        AsyncLocalHttpServer::start("200 OK", &[("Content-Type", "text/event-stream")], body)
            .await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiResponsesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?
        .with_clock(Arc::new(FixedClock(77)));
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut done = None;
    let mut tool_deltas = Vec::new();
    while let Some(item) = stream.next().await {
        match item? {
            AssistantMessageEvent::ToolCallDelta { delta, .. } => tool_deltas.push(delta),
            AssistantMessageEvent::Done { reason, message } => done = Some((reason, message)),
            _ => {}
        }
    }
    let (reason, message) = done.expect("done event");
    assert_eq!(reason, CompletionReason::ToolUse);
    assert_eq!(message.timestamp, 77);
    assert_eq!(message.response_id.as_deref(), Some("resp_1"));
    assert_eq!(
        message.response_model.as_deref(),
        Some("routed-responses-model")
    );
    assert_eq!(message.usage.input, 15);
    assert_eq!(message.usage.output, 7);
    assert_eq!(message.usage.cache_read, 2);
    assert_eq!(message.usage.cache_write, 3);
    assert_eq!(message.usage.reasoning, Some(4));
    assert_eq!(message.usage.total_tokens, 27);
    assert_eq!(tool_deltas, vec!["{\"path\":", "\"image.png\"}"]);
    let ContentBlock::Thinking(thinking) = &message.content[0] else {
        panic!("first block must be reasoning");
    };
    let signature: Value = serde_json::from_str(
        thinking
            .thinking_signature
            .as_deref()
            .expect("reasoning signature"),
    )?;
    assert_eq!(signature["encrypted_content"], "opaque");
    let ContentBlock::Text(text) = &message.content[1] else {
        panic!("second block must be text");
    };
    assert_eq!(
        serde_json::from_str::<Value>(text.text_signature.as_deref().expect("text signature"))?["id"],
        "msg_1"
    );
    assert!(matches!(
        message.content.get(2),
        Some(ContentBlock::ToolCall(block))
            if block.id == "call_new|fc_new"
                && block.arguments == json!({ "path": "image.png" })
    ));

    let captured = server.finish().await?;
    assert!(String::from_utf8_lossy(&captured).starts_with("POST /v1/responses HTTP/1.1"));
    let payload = request_json(&captured)?;
    assert_eq!(payload["model"], "responses-model");
    assert_eq!(payload["max_output_tokens"], 16);
    assert_eq!(payload["prompt_cache_key"], "session-responses-1");
    assert_eq!(payload["prompt_cache_retention"], "24h");
    assert_eq!(
        payload["reasoning"],
        json!({ "effort": "high", "summary": "auto" })
    );
    assert_eq!(payload["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(
        payload["tool_choice"],
        json!({ "type": "function", "name": "inspect" })
    );
    assert_eq!(
        payload["input"][0],
        json!({ "role": "developer", "content": "Be concise." })
    );
    assert_eq!(payload["input"][1]["content"][0]["type"], "input_text");
    assert_eq!(payload["input"][2]["type"], "reasoning");
    assert_eq!(payload["input"][3]["id"], "msg_prior");
    assert_eq!(payload["input"][4]["id"], "fc_1");
    assert_eq!(payload["input"][4]["call_id"], "call_1");
    assert_eq!(payload["input"][6]["type"], "function_call_output");
    assert_eq!(payload["input"][6]["output"][0]["type"], "input_text");
    assert_eq!(payload["input"][6]["output"][1]["type"], "input_image");
    assert_eq!(payload["input"][7]["output"], "(no tool output)");
    Ok(())
}

#[tokio::test]
async fn maps_incomplete_terminal_to_length() -> Result<(), Box<dyn Error>> {
    let body = "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_2\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n".to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiResponsesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut done = None;
    while let Some(item) = stream.next().await {
        if let AssistantMessageEvent::Done { reason, message } = item? {
            done = Some((reason, message));
        }
    }
    let (reason, message) = done.expect("done event");
    assert_eq!(reason, CompletionReason::Length);
    assert_eq!(message.stop_reason, StopReason::Length);
    assert_eq!(message.response_id.as_deref(), Some("resp_2"));
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_abrupt_eof_as_retryable_protocol_error() -> Result<(), Box<dyn Error>> {
    let body = "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_truncated\"}}\n\n"
        .to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiResponsesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut terminal_error = None;
    while let Some(item) = stream.next().await {
        if let Err(error) = item {
            terminal_error = Some(error);
        }
    }
    let error = terminal_error.expect("abrupt EOF must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::Protocol);
    assert!(error.retryable);
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn normalizes_failed_stream_events() -> Result<(), Box<dyn Error>> {
    let body = "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_failed\",\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"provider unavailable\"}}}\n\n".to_owned();
    let server = AsyncLocalHttpServer::start("200 OK", &[], body).await?;
    let base_url = server.base_url().to_owned();
    let adapter = OpenAiResponsesAdapter::new(&ProviderAdapterConfig::new(base_url, timeouts()))?;
    let cancellation = FakeCancellation::default();
    let mut stream = adapter.stream(request(), &cancellation).await?;
    let mut terminal_error = None;
    while let Some(item) = stream.next().await {
        if let Err(error) = item {
            terminal_error = Some(error);
        }
    }
    let error = terminal_error.expect("failed event must fail");
    assert_eq!(error.category, ModelServiceErrorCategory::Server);
    assert!(error.retryable);
    assert_eq!(error.provider_code.as_deref(), Some("server_error"));
    server.finish().await?;
    Ok(())
}

use futures_util::StreamExt;
use pi_model::{
    ModelRequest, ModelRequestOptions, ModelService, ModelServiceError, ModelServiceErrorCategory,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, ContentBlock, Extensions, Message, MessageContent,
    Model, ModelCost, ModelInput, StopReason, TextBlock, Usage, UserMessage,
};
use pi_provider::{
    AnthropicMessagesAdapter, GoogleGenerativeLanguageAdapter, OpenAiChatAdapter,
    OpenAiResponsesAdapter, ProviderAdapterConfig, ProviderClock, ProviderTimeouts,
};
use pi_test_support::{
    AsyncLocalHttpServer, FakeCancellation, FakeClock, FauxModelService, FauxResponse,
};
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

fn model(api: &str, provider: &str, id: &str, base_url: &str) -> Model {
    Model {
        id: id.to_owned(),
        name: id.to_owned(),
        api: api.to_owned(),
        provider: provider.to_owned(),
        base_url: base_url.to_owned(),
        reasoning: false,
        input: vec![ModelInput::Text],
        cost: ModelCost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
        },
        context_window: 128_000,
        max_tokens: 1_024,
        headers: None,
        compat: None,
        thinking_level_map: None,
        extensions: Extensions::new(),
    }
}

fn request(model: Model) -> ModelRequest {
    ModelRequest {
        model,
        system_prompt: String::new(),
        messages: vec![Message::User(UserMessage::new(
            MessageContent::Text("hello".to_owned()),
            1,
        ))],
        tools: Vec::new(),
        options: ModelRequestOptions::default(),
    }
}

async fn assert_text_semantics(
    service: &dyn ModelService,
    request: ModelRequest,
) -> Result<(), ModelServiceError> {
    let expected_api = request.model.api.clone();
    let expected_provider = request.model.provider.clone();
    let expected_model = request.model.id.clone();
    let cancellation = FakeCancellation::default();
    let mut stream = service.stream(request, &cancellation).await?;
    let mut names = Vec::new();
    let mut done = None;
    while let Some(item) = stream.next().await {
        let event = item?;
        names.push(match &event {
            AssistantMessageEvent::Start { .. } => "start",
            AssistantMessageEvent::TextStart { .. } => "text_start",
            AssistantMessageEvent::TextDelta { .. } => "text_delta",
            AssistantMessageEvent::TextEnd { .. } => "text_end",
            AssistantMessageEvent::Done { message, .. } => {
                done = Some(message.clone());
                "done"
            }
            AssistantMessageEvent::ThinkingStart { .. }
            | AssistantMessageEvent::ThinkingDelta { .. }
            | AssistantMessageEvent::ThinkingEnd { .. }
            | AssistantMessageEvent::ToolCallStart { .. }
            | AssistantMessageEvent::ToolCallDelta { .. }
            | AssistantMessageEvent::ToolCallEnd { .. }
            | AssistantMessageEvent::Error { .. } => "unexpected",
        });
    }
    assert_eq!(
        names,
        vec!["start", "text_start", "text_delta", "text_end", "done"]
    );
    let done = done.expect("done message");
    assert_eq!(done.api, expected_api);
    assert_eq!(done.provider, expected_provider);
    assert_eq!(done.model, expected_model);
    assert_eq!(done.stop_reason, StopReason::Stop);
    assert!(matches!(
        done.content.first(),
        Some(ContentBlock::Text(block)) if block.text == "ok"
    ));
    Ok(())
}

#[tokio::test]
async fn all_adapters_share_the_canonical_text_stream_contract() -> Result<(), Box<dyn Error>> {
    let chat_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let chat_server = AsyncLocalHttpServer::start(
        "200 OK",
        &[("Content-Type", "text/event-stream")],
        chat_body,
    )
    .await?;
    let chat_url = chat_server.base_url().to_owned();
    let chat = OpenAiChatAdapter::new(&ProviderAdapterConfig::new(chat_url.clone(), timeouts()))?
        .with_clock(Arc::new(FixedClock(10)));
    assert_text_semantics(
        &chat,
        request(model("openai-completions", "openai", "chat", &chat_url)),
    )
    .await?;
    chat_server.finish().await?;

    let responses_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"content\":[]}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\"}}\n\n"
    );
    let responses_server = AsyncLocalHttpServer::start(
        "200 OK",
        &[("Content-Type", "text/event-stream")],
        responses_body,
    )
    .await?;
    let responses_url = responses_server.base_url().to_owned();
    let responses = OpenAiResponsesAdapter::new(&ProviderAdapterConfig::new(
        responses_url.clone(),
        timeouts(),
    ))?
    .with_clock(Arc::new(FixedClock(20)));
    assert_text_semantics(
        &responses,
        request(model(
            "openai-responses",
            "openai",
            "responses",
            &responses_url,
        )),
    )
    .await?;
    responses_server.finish().await?;

    let anthropic_body = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
    );
    let anthropic_server = AsyncLocalHttpServer::start(
        "200 OK",
        &[("Content-Type", "text/event-stream")],
        anthropic_body,
    )
    .await?;
    let anthropic_url = anthropic_server.base_url().to_owned();
    let anthropic = AnthropicMessagesAdapter::new(&ProviderAdapterConfig::new(
        anthropic_url.clone(),
        timeouts(),
    ))?
    .with_clock(Arc::new(FixedClock(30)));
    assert_text_semantics(
        &anthropic,
        request(model(
            "anthropic-messages",
            "anthropic",
            "claude",
            &anthropic_url,
        )),
    )
    .await?;
    anthropic_server.finish().await?;

    let google_body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}]}\n\n";
    let google_server = AsyncLocalHttpServer::start(
        "200 OK",
        &[("Content-Type", "text/event-stream")],
        google_body,
    )
    .await?;
    let google_url = google_server.base_url().to_owned();
    let google = GoogleGenerativeLanguageAdapter::new(&ProviderAdapterConfig::new(
        google_url.clone(),
        timeouts(),
    ))?
    .with_clock(Arc::new(FixedClock(40)));
    assert_text_semantics(
        &google,
        request(model(
            "google-generative-ai",
            "google",
            "gemini-3-pro-preview",
            &google_url,
        )),
    )
    .await?;
    google_server.finish().await?;

    let faux = FauxModelService::new(Arc::new(FakeClock::new(50, 1)), 64);
    faux.set_responses(vec![FauxResponse::Message(Box::new(
        AssistantMessage::new(
            vec![ContentBlock::Text(TextBlock::new("ok"))],
            "faux",
            "faux",
            "faux-1",
            Usage::default(),
            StopReason::Stop,
            0,
        ),
    ))])?;
    assert_text_semantics(
        &faux,
        request(model("faux", "faux", "faux-1", "http://localhost:0")),
    )
    .await?;
    Ok(())
}

#[test]
fn provider_crate_has_no_runtime_layer_dependencies() -> Result<(), Box<dyn Error>> {
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))?;
    for forbidden in ["pi-agent", "pi-cli", "pi-resource", "pi-store", "pi-tools"] {
        assert!(
            !manifest
                .lines()
                .any(|line| line.trim_start().starts_with(forbidden)),
            "pi-provider must not depend on {forbidden}"
        );
    }
    assert!(!manifest.contains("reqwest-retry"));
    assert!(!manifest.contains("eventsource"));
    Ok(())
}

#[test]
fn canonical_errors_remain_closed_and_structured() {
    let error = ModelServiceError::new(ModelServiceErrorCategory::Protocol, "synthetic", false)
        .with_provider_code("synthetic_protocol");
    assert_eq!(error.category, ModelServiceErrorCategory::Protocol);
    assert_eq!(error.provider_code.as_deref(), Some("synthetic_protocol"));
}

use futures_core::Stream;
use pi_model::{
    CacheRetention, ModelRequest, ModelRequestOptions, ModelService, ModelServiceError,
    ModelServiceErrorCategory,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, ContentBlock, Extensions, Message, MessageContent,
    Model, ModelCost, ModelInput, StopReason, TextBlock, ThinkingBlock, ToolCallBlock, Usage,
    UserMessage,
};
use pi_test_support::{FakeCancellation, FakeClock, FauxModelService, FauxResponse};
use serde_json::json;
use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;

fn model() -> Model {
    Model {
        id: "faux-1".to_owned(),
        name: "Faux Model".to_owned(),
        api: "faux".to_owned(),
        provider: "faux".to_owned(),
        base_url: "http://localhost:0".to_owned(),
        reasoning: true,
        input: vec![ModelInput::Text, ModelInput::Image],
        cost: ModelCost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
        },
        context_window: 128_000,
        max_tokens: 16_384,
        headers: None,
        compat: None,
        thinking_level_map: None,
        extensions: Extensions::new(),
    }
}

fn request(messages: Vec<Message>) -> ModelRequest {
    ModelRequest {
        model: model(),
        system_prompt: "Be concise.".to_owned(),
        messages,
        tools: Vec::new(),
        options: ModelRequestOptions {
            cache_retention: CacheRetention::Short,
            session_id: Some("faux-session-1".to_owned()),
            ..ModelRequestOptions::default()
        },
    }
}

fn response(text: &str) -> AssistantMessage {
    AssistantMessage::new(
        vec![ContentBlock::Text(TextBlock::new(text))],
        "ignored-api",
        "ignored-provider",
        "ignored-model",
        Usage::default(),
        StopReason::Stop,
        0,
    )
}

async fn next_event(
    stream: &mut pi_model::ModelEventStream<'_>,
) -> Option<Result<AssistantMessageEvent, ModelServiceError>> {
    std::future::poll_fn(|context| Pin::new(&mut *stream).poll_next(context)).await
}

#[tokio::test]
async fn streams_fixed_mixed_content_order_and_estimates_cache() -> Result<(), Box<dyn Error>> {
    let clock = Arc::new(FakeClock::new(100, 10));
    let service = FauxModelService::new(clock, 64);
    let message = AssistantMessage::new(
        vec![
            ContentBlock::Thinking(ThinkingBlock::new("think")),
            ContentBlock::Text(TextBlock::new("answer")),
            ContentBlock::ToolCall(ToolCallBlock::new(
                "tool-1",
                "echo",
                json!({ "text": "hello" }),
            )),
        ],
        "ignored-api",
        "ignored-provider",
        "ignored-model",
        Usage::default(),
        StopReason::ToolUse,
        0,
    );
    service.set_responses(vec![
        FauxResponse::Message(Box::new(message)),
        FauxResponse::Message(Box::new(response("continued"))),
    ])?;
    let cancellation = FakeCancellation::default();
    let initial_request = request(vec![Message::User(UserMessage::new(
        MessageContent::Text("hello".to_owned()),
        1,
    ))]);
    let mut stream = service.stream(initial_request, &cancellation).await?;
    let mut names = Vec::new();
    let mut first_message = None;
    while let Some(item) = next_event(&mut stream).await {
        let event = item?;
        names.push(match &event {
            AssistantMessageEvent::Start { .. } => "start",
            AssistantMessageEvent::ThinkingStart { .. } => "thinking_start",
            AssistantMessageEvent::ThinkingDelta { .. } => "thinking_delta",
            AssistantMessageEvent::ThinkingEnd { .. } => "thinking_end",
            AssistantMessageEvent::TextStart { .. } => "text_start",
            AssistantMessageEvent::TextDelta { .. } => "text_delta",
            AssistantMessageEvent::TextEnd { .. } => "text_end",
            AssistantMessageEvent::ToolCallStart { .. } => "toolcall_start",
            AssistantMessageEvent::ToolCallDelta { .. } => "toolcall_delta",
            AssistantMessageEvent::ToolCallEnd { .. } => "toolcall_end",
            AssistantMessageEvent::Done { message, .. } => {
                first_message = Some(message.clone());
                "done"
            }
            AssistantMessageEvent::Error { .. } => "error",
        });
    }
    assert_eq!(
        names,
        vec![
            "start",
            "thinking_start",
            "thinking_delta",
            "thinking_end",
            "text_start",
            "text_delta",
            "text_end",
            "toolcall_start",
            "toolcall_delta",
            "toolcall_end",
            "done",
        ]
    );
    let first_message = first_message.expect("first done message");
    assert_eq!(first_message.api, "faux");
    assert_eq!(first_message.provider, "faux");
    assert_eq!(first_message.model, "faux-1");
    assert_eq!(first_message.timestamp, 100);
    assert_eq!(first_message.usage.cache_read, 0);
    assert!(first_message.usage.cache_write > 0);

    let follow_up = request(vec![
        Message::User(UserMessage::new(
            MessageContent::Text("hello".to_owned()),
            1,
        )),
        Message::Assistant(first_message),
        Message::User(UserMessage::new(
            MessageContent::Text("continue".to_owned()),
            2,
        )),
    ]);
    let mut stream = service.stream(follow_up, &cancellation).await?;
    let mut second_message = None;
    while let Some(item) = next_event(&mut stream).await {
        if let AssistantMessageEvent::Done { message, .. } = item? {
            second_message = Some(message);
        }
    }
    let second_message = second_message.expect("second done message");
    assert!(second_message.usage.cache_read > 0);
    assert!(second_message.usage.cache_write > 0);
    assert_eq!(service.call_count(), 2);
    assert_eq!(service.pending_response_count()?, 0);
    Ok(())
}

#[tokio::test]
async fn cancellation_interrupts_between_deltas() -> Result<(), Box<dyn Error>> {
    let service = FauxModelService::new(Arc::new(FakeClock::new(1, 1)), 1);
    service.set_responses(vec![FauxResponse::Message(Box::new(response("abc")))])?;
    let cancellation = FakeCancellation::default();
    let mut stream = service
        .stream(
            request(vec![Message::User(UserMessage::new(
                MessageContent::Text("hello".to_owned()),
                1,
            ))]),
            &cancellation,
        )
        .await?;
    assert!(matches!(
        next_event(&mut stream).await,
        Some(Ok(AssistantMessageEvent::Start { .. }))
    ));
    assert!(matches!(
        next_event(&mut stream).await,
        Some(Ok(AssistantMessageEvent::TextStart { .. }))
    ));
    assert!(matches!(
        next_event(&mut stream).await,
        Some(Ok(AssistantMessageEvent::TextDelta { delta, .. })) if delta == "a"
    ));
    cancellation.cancel();
    let error = next_event(&mut stream)
        .await
        .expect("cancelled item")
        .expect_err("cancellation must fail the stream");
    assert_eq!(error.category, ModelServiceErrorCategory::Cancelled);
    assert!(!error.retryable);
    assert!(next_event(&mut stream).await.is_none());
    Ok(())
}

#[tokio::test]
async fn reports_queue_and_script_errors_deterministically() -> Result<(), Box<dyn Error>> {
    let service = FauxModelService::new(Arc::new(FakeClock::new(1, 1)), 4);
    let cancellation = FakeCancellation::default();
    let model_request = request(vec![Message::User(UserMessage::new(
        MessageContent::Text("hello".to_owned()),
        1,
    ))]);
    let exhausted = match service.stream(model_request.clone(), &cancellation).await {
        Ok(_) => panic!("empty queue must fail"),
        Err(error) => error,
    };
    assert_eq!(exhausted.category, ModelServiceErrorCategory::Configuration);
    assert_eq!(
        exhausted.provider_code.as_deref(),
        Some("no_scripted_response")
    );

    service.set_responses(vec![FauxResponse::Error(
        ModelServiceError::new(
            ModelServiceErrorCategory::Unknown,
            "script factory failed",
            false,
        )
        .with_provider_code("script_factory_error"),
    )])?;
    let factory_error = match service.stream(model_request, &cancellation).await {
        Ok(_) => panic!("script error must fail"),
        Err(error) => error,
    };
    assert_eq!(factory_error.category, ModelServiceErrorCategory::Unknown);
    assert_eq!(
        factory_error.provider_code.as_deref(),
        Some("script_factory_error")
    );
    assert!(!factory_error.retryable);
    Ok(())
}

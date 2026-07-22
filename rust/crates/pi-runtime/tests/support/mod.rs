#![allow(dead_code)]

use pi_agent::Clock;
use pi_model::Cancellation;
use pi_protocol::{
    AssistantMessage, ContentBlock, Extensions, Message, MessageContent, Model, ModelCost,
    ModelInput, StopReason, TextBlock, Usage, UserMessage,
};
use pi_runtime::{
    CompactionFuture, CompactionOutput, CompactionRequest, Compactor, RuntimeBoundaryError,
    RuntimeEvent, RuntimeEventFuture, RuntimeEventSink, SleepFuture, SleepOutcome, Sleeper,
};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Default)]
pub struct RecordingEvents {
    events: Mutex<Vec<RuntimeEvent>>,
}

impl RecordingEvents {
    pub fn snapshot(&self) -> Vec<RuntimeEvent> {
        self.events.lock().expect("event lock").clone()
    }
}

impl RuntimeEventSink for RecordingEvents {
    fn emit<'a>(&'a self, event: RuntimeEvent) -> RuntimeEventFuture<'a> {
        Box::pin(async move {
            self.events
                .lock()
                .map_err(|_| RuntimeBoundaryError::new("event lock poisoned"))?
                .push(event);
            Ok(())
        })
    }
}

#[derive(Debug, Default)]
pub struct RecordingSleeper {
    durations: Mutex<Vec<Duration>>,
    cancel: bool,
}

impl RecordingSleeper {
    pub fn cancelled() -> Self {
        Self {
            durations: Mutex::new(Vec::new()),
            cancel: true,
        }
    }

    pub fn snapshot(&self) -> Vec<Duration> {
        self.durations.lock().expect("sleeper lock").clone()
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep<'a>(
        &'a self,
        duration: Duration,
        _cancellation: &'a dyn Cancellation,
    ) -> SleepFuture<'a> {
        Box::pin(async move {
            self.durations.lock().expect("sleeper lock").push(duration);
            if self.cancel {
                SleepOutcome::Cancelled
            } else {
                SleepOutcome::Completed
            }
        })
    }
}

#[derive(Debug)]
pub struct FixedCompactor {
    summary: String,
    usage: Usage,
    requests: Mutex<Vec<CompactionRequest>>,
    failure: Option<String>,
}

impl FixedCompactor {
    pub fn new(summary: &str, usage: Usage) -> Self {
        Self {
            summary: summary.to_owned(),
            usage,
            requests: Mutex::new(Vec::new()),
            failure: None,
        }
    }

    pub fn failing(message: &str) -> Self {
        Self {
            summary: String::new(),
            usage: Usage::default(),
            requests: Mutex::new(Vec::new()),
            failure: Some(message.to_owned()),
        }
    }

    pub fn requests(&self) -> Vec<CompactionRequest> {
        self.requests.lock().expect("compactor lock").clone()
    }
}

impl Compactor for FixedCompactor {
    fn compact<'a>(
        &'a self,
        request: CompactionRequest,
        _cancellation: &'a dyn Cancellation,
    ) -> CompactionFuture<'a> {
        Box::pin(async move {
            self.requests
                .lock()
                .map_err(|_| RuntimeBoundaryError::new("compactor lock poisoned"))?
                .push(request);
            if let Some(message) = &self.failure {
                return Err(RuntimeBoundaryError::new(message.clone()));
            }
            Ok(CompactionOutput {
                summary: self.summary.clone(),
                usage: self.usage,
            })
        })
    }
}

#[derive(Debug)]
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

pub fn model(context_window: u64) -> Model {
    Model {
        id: "faux-1".to_owned(),
        name: "Faux".to_owned(),
        api: "faux".to_owned(),
        provider: "faux".to_owned(),
        base_url: "http://localhost.invalid".to_owned(),
        reasoning: false,
        input: vec![ModelInput::Text, ModelInput::Image],
        cost: ModelCost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
        },
        context_window,
        max_tokens: 8_192,
        headers: None,
        compat: None,
        thinking_level_map: None,
        extensions: BTreeMap::new(),
    }
}

pub fn assistant(text: &str) -> AssistantMessage {
    AssistantMessage::new(
        vec![ContentBlock::Text(TextBlock::new(text))],
        "faux",
        "faux",
        "faux-1",
        Usage::default(),
        StopReason::Stop,
        0,
    )
}

pub fn prompt(text: &str, timestamp: u64) -> UserMessage {
    UserMessage::new(MessageContent::Text(text.to_owned()), timestamp)
}

pub fn message_text(message: &Message) -> Option<&str> {
    match message {
        Message::User(message) => match &message.content {
            MessageContent::Text(text) => Some(text),
            MessageContent::Blocks(blocks) => blocks.iter().find_map(|block| match block {
                ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            }),
        },
        Message::Assistant(message) => message.content.iter().find_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        }),
        Message::ToolResult(message) => message.content.iter().find_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        }),
    }
}

pub fn empty_extensions() -> Extensions {
    Extensions::new()
}

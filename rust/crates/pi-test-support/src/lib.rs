//! Deterministic fixtures and fakes shared by Rust contract tests.

mod compatibility;

pub use compatibility::{
    CompatibilityCatalog, CompatibilityEntry, CompatibilityState, validate_compatibility_catalog,
};

use futures_core::Stream;
use pi_model::{
    CacheRetention, Cancellation, ModelEventStream, ModelRequest, ModelService, ModelServiceError,
    ModelServiceErrorCategory,
};
use pi_protocol::{
    AssistantMessage, AssistantMessageEvent, CompletionReason, ContentBlock, Message,
    MessageContent, PersistedSessionRecord, StopReason, TextBlock, ThinkingBlock, Usage,
};
use pi_store::{SessionStore, StoreError, StoreFuture};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener as TokioTcpListener, TcpStream as TokioTcpStream},
    task::JoinHandle as TokioJoinHandle,
};

/// Deterministic monotonic clock.
#[derive(Debug)]
pub struct FakeClock {
    next: AtomicU64,
    step: u64,
}

impl FakeClock {
    #[must_use]
    pub const fn new(first: u64, step: u64) -> Self {
        Self {
            next: AtomicU64::new(first),
            step,
        }
    }

    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.next.fetch_add(self.step, Ordering::Relaxed)
    }
}

/// Reproducible string identifier generator.
#[derive(Debug)]
pub struct DeterministicIds {
    prefix: String,
    next: AtomicU64,
}

/// Records requested sleeps without delaying a test.
#[derive(Debug, Default)]
pub struct FakeSleeper {
    durations: Mutex<Vec<Duration>>,
}

impl FakeSleeper {
    pub fn sleep(&self, duration: Duration) -> Result<(), String> {
        self.durations
            .lock()
            .map_err(|_| "fake sleeper lock poisoned".to_owned())?
            .push(duration);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<Duration>, String> {
        self.durations
            .lock()
            .map(|durations| durations.clone())
            .map_err(|_| "fake sleeper lock poisoned".to_owned())
    }
}

impl DeterministicIds {
    #[must_use]
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            next: AtomicU64::new(1),
        }
    }

    #[must_use]
    pub fn next(&self) -> String {
        let sequence = self.next.fetch_add(1, Ordering::Relaxed);
        format!("{}-{sequence:08}", self.prefix)
    }
}

/// Cancellation token controlled directly by a test.
#[derive(Debug)]
pub struct FakeCancellation {
    cancelled: AtomicBool,
    signal: watch::Sender<bool>,
}

impl Default for FakeCancellation {
    fn default() -> Self {
        let (signal, _) = watch::channel(false);
        Self {
            cancelled: AtomicBool::new(false),
            signal,
        }
    }
}

impl FakeCancellation {
    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            self.signal.send_replace(true);
        }
    }
}

impl Cancellation for FakeCancellation {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn cancelled(&self) -> pi_model::CancellationFuture<'_> {
        let mut receiver = self.signal.subscribe();
        Box::pin(async move {
            if *receiver.borrow() {
                return;
            }
            while receiver.changed().await.is_ok() {
                if *receiver.borrow_and_update() {
                    return;
                }
            }
        })
    }
}

struct ScriptedStream {
    events: VecDeque<Result<AssistantMessageEvent, ModelServiceError>>,
}

impl Stream for ScriptedStream {
    type Item = Result<AssistantMessageEvent, ModelServiceError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.events.pop_front())
    }
}

/// Faux provider that returns exactly one preloaded stream per call.
#[derive(Debug)]
pub struct ScriptedModelService {
    events: Mutex<Option<Vec<Result<AssistantMessageEvent, ModelServiceError>>>>,
}

impl ScriptedModelService {
    #[must_use]
    pub fn new(events: Vec<Result<AssistantMessageEvent, ModelServiceError>>) -> Self {
        Self {
            events: Mutex::new(Some(events)),
        }
    }
}

impl ModelService for ScriptedModelService {
    fn stream<'a>(
        &'a self,
        _request: ModelRequest,
        cancellation: &'a dyn Cancellation,
    ) -> pi_model::ModelFuture<'a> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(ModelServiceError::cancelled());
            }
            let events = self
                .events
                .lock()
                .map_err(|_| {
                    ModelServiceError::new(
                        ModelServiceErrorCategory::Unknown,
                        "scripted model lock poisoned",
                        false,
                    )
                })?
                .take()
                .ok_or_else(|| {
                    ModelServiceError::new(
                        ModelServiceErrorCategory::Unknown,
                        "scripted model response already consumed",
                        false,
                    )
                })?;
            Ok(ModelEventStream::new(ScriptedStream {
                events: events.into(),
            }))
        })
    }
}

/// One queued Faux result, including deterministic factory-style failures.
#[derive(Clone, Debug)]
pub enum FauxResponse {
    Message(Box<AssistantMessage>),
    Error(ModelServiceError),
}

/// In-memory provider used to exercise the complete model-service contract without networking.
#[derive(Debug)]
pub struct FauxModelService {
    responses: Mutex<VecDeque<FauxResponse>>,
    prompt_cache: Mutex<BTreeMap<String, String>>,
    clock: Arc<FakeClock>,
    chunk_chars: usize,
    call_count: AtomicU64,
}

impl FauxModelService {
    #[must_use]
    pub fn new(clock: Arc<FakeClock>, chunk_chars: usize) -> Self {
        Self {
            responses: Mutex::new(VecDeque::new()),
            prompt_cache: Mutex::new(BTreeMap::new()),
            clock,
            chunk_chars: chunk_chars.max(1),
            call_count: AtomicU64::new(0),
        }
    }

    pub fn set_responses(&self, responses: Vec<FauxResponse>) -> Result<(), ModelServiceError> {
        let mut queued = self.responses.lock().map_err(|_| faux_lock_error())?;
        *queued = responses.into();
        Ok(())
    }

    pub fn append_responses(&self, responses: Vec<FauxResponse>) -> Result<(), ModelServiceError> {
        self.responses
            .lock()
            .map_err(|_| faux_lock_error())?
            .extend(responses);
        Ok(())
    }

    pub fn pending_response_count(&self) -> Result<usize, ModelServiceError> {
        self.responses
            .lock()
            .map(|responses| responses.len())
            .map_err(|_| faux_lock_error())
    }

    #[must_use]
    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Acquire)
    }

    fn with_usage(
        &self,
        mut message: AssistantMessage,
        request: &ModelRequest,
    ) -> Result<AssistantMessage, ModelServiceError> {
        message.api.clone_from(&request.model.api);
        message.provider.clone_from(&request.model.provider);
        message.model.clone_from(&request.model.id);
        if message.timestamp == 0 {
            message.timestamp = self.clock.now_ms();
        }

        let prompt = serialize_model_request(request)?;
        let prompt_tokens = estimate_tokens(&prompt);
        let output_tokens = estimate_tokens(&serialize_assistant_content(&message.content));
        let mut input = prompt_tokens;
        let mut cache_read = 0;
        let mut cache_write = 0;
        if request.options.cache_retention != CacheRetention::None
            && let Some(session_id) = &request.options.session_id
        {
            let mut cache = self.prompt_cache.lock().map_err(|_| faux_lock_error())?;
            if let Some(previous) = cache.get(session_id) {
                let common = common_prefix(previous, &prompt);
                cache_read = estimate_tokens(common);
                cache_write = estimate_tokens(&prompt[common.len()..]);
                input = prompt_tokens.saturating_sub(cache_read);
            } else {
                cache_write = prompt_tokens;
            }
            cache.insert(session_id.clone(), prompt);
        }
        message.usage = Usage {
            input,
            output: output_tokens,
            cache_read,
            cache_write,
            cache_write_1h: None,
            reasoning: None,
            total_tokens: input + output_tokens + cache_read + cache_write,
            cost: pi_protocol::UsageCost::default(),
        };
        Ok(message)
    }
}

impl ModelService for FauxModelService {
    fn stream<'a>(
        &'a self,
        request: ModelRequest,
        cancellation: &'a dyn Cancellation,
    ) -> pi_model::ModelFuture<'a> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(ModelServiceError::cancelled());
            }
            self.call_count.fetch_add(1, Ordering::AcqRel);
            let response = self
                .responses
                .lock()
                .map_err(|_| faux_lock_error())?
                .pop_front()
                .ok_or_else(|| {
                    ModelServiceError::new(
                        ModelServiceErrorCategory::Configuration,
                        "no Faux response is queued",
                        false,
                    )
                    .with_provider_code("no_scripted_response")
                })?;
            let message = match response {
                FauxResponse::Message(message) => self.with_usage(*message, &request)?,
                FauxResponse::Error(error) => return Err(error),
            };
            let events = build_faux_events(message, self.chunk_chars)?;
            Ok(ModelEventStream::new(FauxEventStream {
                events,
                cancellation,
                terminated: false,
            }))
        })
    }
}

struct FauxEventStream<'a> {
    events: VecDeque<Result<AssistantMessageEvent, ModelServiceError>>,
    cancellation: &'a dyn Cancellation,
    terminated: bool,
}

impl Stream for FauxEventStream<'_> {
    type Item = Result<AssistantMessageEvent, ModelServiceError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }
        if self.cancellation.is_cancelled() {
            self.terminated = true;
            return Poll::Ready(Some(Err(ModelServiceError::cancelled())));
        }
        let item = self.events.pop_front();
        if item.is_none() {
            self.terminated = true;
        }
        Poll::Ready(item)
    }
}

fn build_faux_events(
    message: AssistantMessage,
    chunk_chars: usize,
) -> Result<VecDeque<Result<AssistantMessageEvent, ModelServiceError>>, ModelServiceError> {
    let mut events = VecDeque::new();
    let mut partial = message.clone();
    partial.content.clear();
    events.push_back(Ok(AssistantMessageEvent::Start {
        partial: partial.clone(),
        extensions: pi_protocol::Extensions::new(),
    }));
    for block in &message.content {
        let content_index = partial.content.len();
        match block {
            ContentBlock::Thinking(block) => {
                let mut partial_block = ThinkingBlock::new("");
                partial_block.thinking_signature = block.thinking_signature.clone();
                partial_block.redacted = block.redacted;
                partial.content.push(ContentBlock::Thinking(partial_block));
                events.push_back(Ok(AssistantMessageEvent::ThinkingStart {
                    content_index,
                    partial: partial.clone(),
                }));
                for delta in split_chunks(&block.thinking, chunk_chars) {
                    if let Some(ContentBlock::Thinking(partial_block)) =
                        partial.content.get_mut(content_index)
                    {
                        partial_block.thinking.push_str(&delta);
                    }
                    events.push_back(Ok(AssistantMessageEvent::ThinkingDelta {
                        content_index,
                        delta,
                        partial: partial.clone(),
                    }));
                }
                events.push_back(Ok(AssistantMessageEvent::ThinkingEnd {
                    content_index,
                    content: block.thinking.clone(),
                    partial: partial.clone(),
                }));
            }
            ContentBlock::Text(block) => {
                let mut partial_block = TextBlock::new("");
                partial_block.text_signature = block.text_signature.clone();
                partial.content.push(ContentBlock::Text(partial_block));
                events.push_back(Ok(AssistantMessageEvent::TextStart {
                    content_index,
                    partial: partial.clone(),
                }));
                for delta in split_chunks(&block.text, chunk_chars) {
                    if let Some(ContentBlock::Text(partial_block)) =
                        partial.content.get_mut(content_index)
                    {
                        partial_block.text.push_str(&delta);
                    }
                    events.push_back(Ok(AssistantMessageEvent::TextDelta {
                        content_index,
                        delta,
                        partial: partial.clone(),
                    }));
                }
                events.push_back(Ok(AssistantMessageEvent::TextEnd {
                    content_index,
                    content: block.text.clone(),
                    partial: partial.clone(),
                }));
            }
            ContentBlock::ToolCall(block) => {
                let mut partial_block = block.clone();
                partial_block.arguments = serde_json::json!({});
                partial.content.push(ContentBlock::ToolCall(partial_block));
                events.push_back(Ok(AssistantMessageEvent::ToolCallStart {
                    content_index,
                    partial: partial.clone(),
                }));
                for delta in split_chunks(&block.arguments.to_string(), chunk_chars) {
                    events.push_back(Ok(AssistantMessageEvent::ToolCallDelta {
                        content_index,
                        delta,
                        partial: partial.clone(),
                    }));
                }
                partial.content[content_index] = ContentBlock::ToolCall(block.clone());
                events.push_back(Ok(AssistantMessageEvent::ToolCallEnd {
                    content_index,
                    tool_call: block.clone(),
                    partial: partial.clone(),
                }));
            }
            ContentBlock::Image(_) => {
                return Err(ModelServiceError::new(
                    ModelServiceErrorCategory::Configuration,
                    "Faux assistant responses cannot contain image blocks",
                    false,
                )
                .with_provider_code("invalid_scripted_response"));
            }
        }
    }
    match message.stop_reason {
        StopReason::Stop | StopReason::Length | StopReason::ToolUse => {
            events.push_back(Ok(AssistantMessageEvent::Done {
                reason: completion_reason(message.stop_reason),
                message,
            }));
        }
        StopReason::Error => events.push_back(Err(ModelServiceError::new(
            ModelServiceErrorCategory::Unknown,
            message
                .error_message
                .clone()
                .unwrap_or_else(|| "scripted Faux response failed".to_owned()),
            false,
        )
        .with_provider_code("scripted_response_error"))),
        StopReason::Aborted => events.push_back(Err(ModelServiceError::cancelled())),
    }
    Ok(events)
}

fn serialize_model_request(request: &ModelRequest) -> Result<String, ModelServiceError> {
    let mut parts = Vec::new();
    if !request.system_prompt.is_empty() {
        parts.push(format!("system:{}", request.system_prompt));
    }
    for message in &request.messages {
        parts.push(match message {
            Message::User(message) => format!("user:{}", serialize_user_content(&message.content)),
            Message::Assistant(message) => {
                format!(
                    "assistant:{}",
                    serialize_assistant_content(&message.content)
                )
            }
            Message::ToolResult(message) => format!(
                "toolResult:{}\n{}",
                message.tool_name,
                serialize_assistant_content(&message.content)
            ),
        });
    }
    if !request.tools.is_empty() {
        let tools = serde_json::to_string(&request.tools).map_err(|_| {
            ModelServiceError::new(
                ModelServiceErrorCategory::Configuration,
                "Faux tools could not be serialized",
                false,
            )
        })?;
        parts.push(format!("tools:{tools}"));
    }
    Ok(parts.join("\n\n"))
}

fn serialize_user_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => serialize_assistant_content(blocks),
    }
}

fn serialize_assistant_content(content: &[ContentBlock]) -> String {
    content
        .iter()
        .map(|block| match block {
            ContentBlock::Text(block) => block.text.clone(),
            ContentBlock::Thinking(block) => block.thinking.clone(),
            ContentBlock::Image(block) => {
                format!("[image:{}:{}]", block.mime_type, block.data.len())
            }
            ContentBlock::ToolCall(block) => {
                format!("{}:{}", block.name, block.arguments)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn common_prefix<'a>(left: &'a str, right: &str) -> &'a str {
    let mut end = 0;
    for ((left_index, left_character), right_character) in left.char_indices().zip(right.chars()) {
        if left_character != right_character {
            break;
        }
        end = left_index + left_character.len_utf8();
    }
    &left[..end]
}

fn estimate_tokens(value: &str) -> u64 {
    u64::try_from(value.chars().count().div_ceil(4)).unwrap_or(u64::MAX)
}

fn split_chunks(value: &str, chunk_chars: usize) -> Vec<String> {
    let characters = value.chars().collect::<Vec<_>>();
    if characters.is_empty() {
        return vec![String::new()];
    }
    characters
        .chunks(chunk_chars)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

const fn completion_reason(reason: StopReason) -> CompletionReason {
    match reason {
        StopReason::Stop => CompletionReason::Stop,
        StopReason::Length => CompletionReason::Length,
        StopReason::ToolUse => CompletionReason::ToolUse,
        StopReason::Error | StopReason::Aborted => unreachable!(),
    }
}

fn faux_lock_error() -> ModelServiceError {
    ModelServiceError::new(
        ModelServiceErrorCategory::Unknown,
        "Faux provider state lock poisoned",
        false,
    )
    .with_provider_code("faux_state_error")
}

/// Bounded in-memory event collector.
#[derive(Debug)]
pub struct InMemoryEventSink {
    capacity: usize,
    events: Mutex<Vec<Value>>,
}

impl InMemoryEventSink {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            events: Mutex::new(Vec::new()),
        }
    }

    pub fn emit(&self, event: Value) -> Result<(), String> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| "event sink lock poisoned".to_owned())?;
        if events.len() >= self.capacity {
            return Err(format!("event sink capacity {} exceeded", self.capacity));
        }
        events.push(event);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<Value>, String> {
        self.events
            .lock()
            .map(|events| events.clone())
            .map_err(|_| "event sink lock poisoned".to_owned())
    }
}

/// In-memory append-only session store.
#[derive(Debug, Default)]
pub struct InMemorySessionStore {
    sessions: Mutex<BTreeMap<String, Vec<PersistedSessionRecord>>>,
}

impl SessionStore for InMemorySessionStore {
    fn load<'a>(&'a self, session_id: &'a str) -> StoreFuture<'a, Vec<PersistedSessionRecord>> {
        Box::pin(async move {
            self.sessions
                .lock()
                .map(|sessions| sessions.get(session_id).cloned().unwrap_or_default())
                .map_err(|_| StoreError {
                    message: "session store lock poisoned".to_owned(),
                })
        })
    }

    fn append<'a>(
        &'a self,
        session_id: &'a str,
        record: &'a PersistedSessionRecord,
    ) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let mut sessions = self.sessions.lock().map_err(|_| StoreError {
                message: "session store lock poisoned".to_owned(),
            })?;
            sessions
                .entry(session_id.to_owned())
                .or_default()
                .push(record.clone());
            Ok(())
        })
    }
}

/// Return a validated path beneath the repository root.
pub fn fixture_path(relative: impl AsRef<Path>) -> Result<PathBuf, String> {
    let relative = relative.as_ref();
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("fixture path must be repository-relative and cannot contain '..'".to_owned());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    Ok(root.join(relative))
}

/// Scan fixtures for credential-like values and machine-specific home paths.
pub fn scan_fixture_tree(root: &Path) -> Result<Vec<String>, String> {
    fn visit(root: &Path, path: &Path, findings: &mut Vec<String>) -> Result<(), String> {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                visit(root, &entry.path(), findings)?;
            }
            return Ok(());
        }
        if !path.is_file() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        let patterns = [
            "-----BEGIN PRIVATE KEY-----",
            "AKIA",
            "sk-",
            "/home/",
            "C:\\Users\\",
        ];
        for pattern in patterns {
            if content.contains(pattern) {
                let relative = path.strip_prefix(root).unwrap_or(path);
                findings.push(format!("{} contains {pattern}", relative.display()));
            }
        }
        Ok(())
    }

    let mut findings = Vec::new();
    visit(root, root, &mut findings)?;
    Ok(findings)
}

/// One explicit JSON pointer replacement used for nondeterministic fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizationRule {
    pub pointer: String,
    pub replacement: Value,
}

/// Normalize only allowlisted leaf paths. Whole-event normalization is rejected.
pub fn normalize_json(value: &mut Value, rules: &[NormalizationRule]) -> Result<(), String> {
    for rule in rules {
        if rule.pointer.is_empty() || rule.pointer == "/" {
            return Err("normalization must target a field, not the whole event".to_owned());
        }
        let target = value
            .pointer_mut(&rule.pointer)
            .ok_or_else(|| format!("normalization path not found: {}", rule.pointer))?;
        *target = rule.replacement.clone();
    }
    Ok(())
}

/// One-shot loopback HTTP server for offline provider contract tests.
#[derive(Debug)]
pub struct LocalHttpServer {
    address: SocketAddr,
    request: Receiver<String>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LocalHttpServer {
    pub fn start(status: u16, body: impl Into<String>) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let body = body.into();
        let (sender, request) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !worker_cancelled.load(Ordering::Acquire) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut bytes = Vec::new();
                        let mut buffer = [0_u8; 4096];
                        loop {
                            match stream.read(&mut buffer) {
                                Ok(0) => break,
                                Ok(count) => {
                                    bytes.extend_from_slice(&buffer[..count]);
                                    if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) =>
                                {
                                    break;
                                }
                                Err(_) => break,
                            }
                        }
                        let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
                        let reason = if status == 200 { "OK" } else { "Test Response" };
                        let response = format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            request,
            cancelled,
            worker: Some(worker),
        })
    }

    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn receive_request(&self, timeout: Duration) -> Result<String, String> {
        self.request
            .recv_timeout(timeout)
            .map_err(|error| error.to_string())
    }
}

impl Drop for LocalHttpServer {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// One-shot async HTTP server that captures a complete request and returns a scripted response.
#[derive(Debug)]
pub struct AsyncLocalHttpServer {
    base_url: String,
    worker: TokioJoinHandle<Result<Vec<u8>, String>>,
}

impl AsyncLocalHttpServer {
    pub async fn start(
        status: impl Into<String>,
        headers: &[(&str, &str)],
        body: impl Into<String>,
    ) -> Result<Self, String> {
        let listener = TokioTcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let status = status.into();
        let headers = headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>();
        let body = body.into();
        let worker = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.map_err(|error| error.to_string())?;
            let request = read_async_http_request(&mut socket).await?;
            let mut response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (name, value) in headers {
                response.push_str(&format!("{name}: {value}\r\n"));
            }
            response.push_str("\r\n");
            response.push_str(&body);
            socket
                .write_all(response.as_bytes())
                .await
                .map_err(|error| error.to_string())?;
            Ok(request)
        });
        Ok(Self {
            base_url: format!("http://{address}/v1"),
            worker,
        })
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn finish(self) -> Result<Vec<u8>, String> {
        self.worker.await.map_err(|error| error.to_string())?
    }
}

async fn read_async_http_request(socket: &mut TokioTcpStream) -> Result<Vec<u8>, String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1_024];
    loop {
        let count = socket
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(request);
        }
        request.extend_from_slice(&buffer[..count]);
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if request.len() >= header_end + content_length {
            return Ok(request);
        }
    }
}

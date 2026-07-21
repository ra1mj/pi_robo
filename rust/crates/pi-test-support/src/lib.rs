//! Deterministic fixtures and fakes shared by Rust contract tests.

mod compatibility;

pub use compatibility::{
    CompatibilityCatalog, CompatibilityEntry, CompatibilityState, validate_compatibility_catalog,
};

use futures_core::Stream;
use pi_model::{Cancellation, ModelEventStream, ModelRequest, ModelService, ModelServiceError};
use pi_protocol::{AssistantMessageEvent, PersistedSessionRecord};
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
#[derive(Debug, Default)]
pub struct FakeCancellation {
    cancelled: AtomicBool,
}

impl FakeCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl Cancellation for FakeCancellation {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
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
                return Err(ModelServiceError {
                    category: "cancelled".to_owned(),
                    message: "request cancelled".to_owned(),
                    retryable: false,
                });
            }
            let events = self
                .events
                .lock()
                .map_err(|_| ModelServiceError {
                    category: "test_support".to_owned(),
                    message: "scripted model lock poisoned".to_owned(),
                    retryable: false,
                })?
                .take()
                .ok_or_else(|| ModelServiceError {
                    category: "test_support".to_owned(),
                    message: "scripted model response already consumed".to_owned(),
                    retryable: false,
                })?;
            Ok(Box::pin(ScriptedStream {
                events: events.into(),
            }) as ModelEventStream)
        })
    }
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

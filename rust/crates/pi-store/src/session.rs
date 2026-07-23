use crate::{StoreDiagnostic, StoreError, StoreErrorCategory};
use pi_protocol::{
    CURRENT_SESSION_VERSION, CompactionEntry, KnownSessionRecord, MessageEntry, ModelChangeEntry,
    PersistedSessionRecord, SessionEntryBase, SessionHeader, ThinkingLevelChangeEntry,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::time::SystemTime;

pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;

/// Append-only persistence boundary retained for runtime/test-support consumers.
pub trait SessionStore: Send + Sync {
    fn load<'a>(&'a self, session_id: &'a str) -> StoreFuture<'a, Vec<PersistedSessionRecord>>;
    fn append<'a>(
        &'a self,
        session_id: &'a str,
        record: &'a PersistedSessionRecord,
    ) -> StoreFuture<'a, ()>;
}

pub trait SessionIdentitySource: Send + Sync {
    fn next_id(&self) -> String;
    fn timestamp(&self) -> String;
}

pub struct SessionRecordFactory<'a> {
    source: &'a dyn SessionIdentitySource,
}

impl<'a> SessionRecordFactory<'a> {
    #[must_use]
    pub const fn new(source: &'a dyn SessionIdentitySource) -> Self {
        Self { source }
    }

    pub fn message(
        &self,
        parent_id: Option<String>,
        message: Value,
    ) -> Result<PersistedSessionRecord, StoreError> {
        self.record(KnownSessionRecord::Message(MessageEntry::new(
            self.base(parent_id),
            message,
        )))
    }

    pub fn thinking_level(
        &self,
        parent_id: Option<String>,
        thinking_level: impl Into<String>,
    ) -> Result<PersistedSessionRecord, StoreError> {
        self.record(KnownSessionRecord::ThinkingLevelChange(
            ThinkingLevelChangeEntry::new(self.base(parent_id), thinking_level),
        ))
    }

    pub fn model_change(
        &self,
        parent_id: Option<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Result<PersistedSessionRecord, StoreError> {
        self.record(KnownSessionRecord::ModelChange(ModelChangeEntry::new(
            self.base(parent_id),
            provider,
            model_id,
        )))
    }

    pub fn compaction(
        &self,
        parent_id: Option<String>,
        summary: impl Into<String>,
        first_kept_entry_id: impl Into<String>,
        tokens_before: u64,
    ) -> Result<PersistedSessionRecord, StoreError> {
        self.record(KnownSessionRecord::Compaction(CompactionEntry::new(
            self.base(parent_id),
            summary,
            first_kept_entry_id,
            tokens_before,
        )))
    }

    fn base(&self, parent_id: Option<String>) -> SessionEntryBase {
        SessionEntryBase {
            id: self.source.next_id(),
            parent_id,
            timestamp: self.source.timestamp(),
        }
    }

    fn record(&self, known: KnownSessionRecord) -> Result<PersistedSessionRecord, StoreError> {
        PersistedSessionRecord::from_known(known).map_err(|error| {
            StoreError::new(
                StoreErrorCategory::InvalidShape,
                format!("could not create session record: {error}"),
            )
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionFile {
    pub raw_line: String,
    pub record: Option<PersistedSessionRecord>,
    pub diagnostic: Option<StoreDiagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionFileSnapshot {
    pub path: PathBuf,
    pub header: SessionHeader,
    pub lines: Vec<SessionFile>,
    pub diagnostics: Vec<StoreDiagnostic>,
    pub legacy_version: Option<u32>,
}

impl SessionFileSnapshot {
    pub fn read(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|error| StoreError::io(error, path))?;
        let mut lines = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, raw_line) in split_lines_preserving_endings(&content)
            .into_iter()
            .enumerate()
        {
            let parse_line = raw_line.trim_end_matches(['\r', '\n']);
            if parse_line.trim().is_empty() {
                lines.push(SessionFile {
                    raw_line,
                    record: None,
                    diagnostic: None,
                });
                continue;
            }
            match PersistedSessionRecord::parse(parse_line) {
                Ok(record) => lines.push(SessionFile {
                    raw_line,
                    record: Some(record),
                    diagnostic: None,
                }),
                Err(error) => {
                    let store_error = StoreError::new(
                        StoreErrorCategory::InvalidJson,
                        format!("invalid session JSONL record: {error}"),
                    )
                    .with_path(path)
                    .with_line(index + 1);
                    let diagnostic = StoreDiagnostic::error(&store_error);
                    diagnostics.push(diagnostic.clone());
                    lines.push(SessionFile {
                        raw_line,
                        record: None,
                        diagnostic: Some(diagnostic),
                    });
                }
            }
        }

        let first_nonempty = lines.iter().find(|line| !line.raw_line.trim().is_empty());
        let header = match first_nonempty
            .and_then(|line| line.record.as_ref())
            .and_then(PersistedSessionRecord::known)
        {
            Some(KnownSessionRecord::Header(header)) => header.clone(),
            _ => {
                return Err(StoreError::new(
                    StoreErrorCategory::InvalidShape,
                    "session file does not begin with a valid session header",
                )
                .with_path(path));
            }
        };
        let legacy_version = match header.version {
            Some(CURRENT_SESSION_VERSION) => None,
            Some(version) => Some(version),
            None => Some(1),
        };
        if let Some(version) = legacy_version {
            diagnostics.push(StoreDiagnostic {
                level: crate::DiagnosticLevel::Warning,
                message: format!(
                    "session version {version} is read-only; Rust milestone 1 does not migrate legacy sessions"
                ),
                path: Some(path.display().to_string()),
                line: Some(1),
            });
        }

        Ok(Self {
            path: path.to_path_buf(),
            header,
            lines,
            diagnostics,
            legacy_version,
        })
    }

    #[must_use]
    pub fn records(&self) -> Vec<&PersistedSessionRecord> {
        self.lines
            .iter()
            .filter_map(|line| line.record.as_ref())
            .collect()
    }

    #[must_use]
    pub fn context(&self, leaf_id: Option<&str>) -> SessionContext {
        build_context(&self.lines, leaf_id)
    }

    pub fn find_most_recent(
        directory: impl AsRef<Path>,
        cwd: Option<&Path>,
    ) -> Result<Option<PathBuf>, StoreError> {
        let directory = directory.as_ref();
        let entries = match std::fs::read_dir(directory) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StoreError::io(error, directory)),
        };
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| StoreError::io(error, directory))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(snapshot) = Self::read(&path) else {
                continue;
            };
            if cwd.is_some_and(|cwd| snapshot.header.cwd != cwd.to_string_lossy()) {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            candidates.push((modified, path));
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        Ok(candidates.into_iter().next().map(|candidate| candidate.1))
    }
}

fn split_lines_preserving_endings(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    content.split_inclusive('\n').map(str::to_owned).collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIdentity {
    length: u64,
    modified: Option<SystemTime>,
    platform_id: Option<(u64, u64)>,
}

impl FileIdentity {
    fn read(path: &Path) -> Result<Self, StoreError> {
        let metadata = std::fs::metadata(path).map_err(|error| StoreError::io(error, path))?;
        #[cfg(unix)]
        let platform_id = {
            use std::os::unix::fs::MetadataExt;
            Some((metadata.dev(), metadata.ino()))
        };
        #[cfg(not(unix))]
        let platform_id = None;
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            platform_id,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionWriterState {
    pub snapshot: SessionFileSnapshot,
    identity: FileIdentity,
}

/// One Rust instance serializes appends and rejects detectable external changes.
///
/// This is not a cross-process lock. TypeScript and Rust writers must not append
/// to the same session concurrently.
#[derive(Debug)]
pub struct SessionWriter {
    state: Mutex<SessionWriterState>,
}

impl SessionWriter {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let snapshot = SessionFileSnapshot::read(path)?;
        if let Some(version) = snapshot.legacy_version {
            return Err(StoreError::new(
                StoreErrorCategory::UnsupportedVersion,
                format!(
                    "session version {version} cannot be appended; migrate it with TypeScript first"
                ),
            )
            .with_path(path));
        }
        Ok(Self {
            state: Mutex::new(SessionWriterState {
                snapshot,
                identity: FileIdentity::read(path)?,
            }),
        })
    }

    pub fn create(path: impl AsRef<Path>, header: SessionHeader) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if header.version != Some(CURRENT_SESSION_VERSION) {
            return Err(StoreError::new(
                StoreErrorCategory::UnsupportedVersion,
                "new sessions must use session version 3",
            )
            .with_path(path));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| StoreError::io(error, parent))?;
        }
        let record = PersistedSessionRecord::from_known(KnownSessionRecord::Header(header))
            .map_err(|error| {
                StoreError::new(
                    StoreErrorCategory::InvalidShape,
                    format!("could not serialize session header: {error}"),
                )
                .with_path(path)
            })?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| StoreError::io(error, path))?;
        let mut bytes = serde_json::to_vec(record.raw()).map_err(|error| {
            StoreError::new(
                StoreErrorCategory::InvalidJson,
                format!("could not serialize session header: {error}"),
            )
            .with_path(path)
        })?;
        bytes.push(b'\n');
        file.write_all(&bytes)
            .map_err(|error| StoreError::io(error, path))?;
        drop(file);
        Self::open(path)
    }

    pub fn append(&self, record: &PersistedSessionRecord) -> Result<(), StoreError> {
        if matches!(record.known(), Some(KnownSessionRecord::Header(_)) | None)
            && record.raw().get("type").and_then(Value::as_str) == Some("session")
        {
            return Err(StoreError::new(
                StoreErrorCategory::InvalidShape,
                "a second session header cannot be appended",
            ));
        }
        let mut state = self.state.lock().map_err(|_| {
            StoreError::new(
                StoreErrorCategory::StaleSession,
                "session writer state was poisoned",
            )
        })?;
        let path = state.snapshot.path.clone();
        let current_identity = FileIdentity::read(&path)?;
        if current_identity != state.identity {
            return Err(StoreError::new(
                StoreErrorCategory::StaleSession,
                "session file changed outside this Rust writer; reload before appending",
            )
            .with_path(&path));
        }
        let mut bytes = serde_json::to_vec(record.raw()).map_err(|error| {
            StoreError::new(
                StoreErrorCategory::InvalidJson,
                format!("could not serialize session record: {error}"),
            )
            .with_path(&path)
        })?;
        bytes.push(b'\n');
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|error| StoreError::io(error, &path))?;
        file.write_all(&bytes)
            .map_err(|error| StoreError::io(error, &path))?;
        drop(file);

        let raw_line = String::from_utf8(bytes).map_err(|_| {
            StoreError::new(
                StoreErrorCategory::InvalidJson,
                "serialized session record was not UTF-8",
            )
        })?;
        state.snapshot.lines.push(SessionFile {
            raw_line,
            record: Some(record.clone()),
            diagnostic: None,
        });
        state.identity = FileIdentity::read(&path)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<SessionFileSnapshot, StoreError> {
        self.state
            .lock()
            .map(|state| state.snapshot.clone())
            .map_err(|_| {
                StoreError::new(
                    StoreErrorCategory::StaleSession,
                    "session writer state was poisoned",
                )
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionModel {
    pub provider: String,
    pub model_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionContext {
    pub messages: Vec<Value>,
    pub thinking_level: String,
    pub model: Option<SessionModel>,
}

#[derive(Clone, Debug)]
struct ContextEntry {
    id: String,
    parent_id: Option<String>,
    raw: Value,
    known: Option<KnownSessionRecord>,
}

fn build_context(lines: &[SessionFile], leaf_id: Option<&str>) -> SessionContext {
    let entries: Vec<ContextEntry> = lines
        .iter()
        .filter_map(|line| {
            let record = line.record.as_ref()?;
            if matches!(record.known(), Some(KnownSessionRecord::Header(_))) {
                return None;
            }
            let id = record.raw().get("id")?.as_str()?.to_owned();
            let parent_id = record
                .raw()
                .get("parentId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Some(ContextEntry {
                id,
                parent_id,
                raw: record.raw().clone(),
                known: record.known().cloned(),
            })
        })
        .collect();
    let by_id: BTreeMap<&str, &ContextEntry> = entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let leaf = leaf_id
        .and_then(|id| by_id.get(id).copied())
        .or_else(|| entries.last());
    let mut path = Vec::new();
    let mut current = leaf;
    while let Some(entry) = current {
        path.push(entry);
        current = entry
            .parent_id
            .as_deref()
            .and_then(|parent| by_id.get(parent).copied());
    }
    path.reverse();

    let mut thinking_level = "off".to_owned();
    let mut model = None;
    for entry in &path {
        match &entry.known {
            Some(KnownSessionRecord::ThinkingLevelChange(value)) => {
                thinking_level.clone_from(&value.thinking_level);
            }
            Some(KnownSessionRecord::ModelChange(value)) => {
                model = Some(SessionModel {
                    provider: value.provider.clone(),
                    model_id: value.model_id.clone(),
                });
            }
            Some(KnownSessionRecord::Message(_))
                if entry.raw.pointer("/message/role").and_then(Value::as_str)
                    == Some("assistant") =>
            {
                if let (Some(provider), Some(model_id)) = (
                    entry
                        .raw
                        .pointer("/message/provider")
                        .and_then(Value::as_str),
                    entry.raw.pointer("/message/model").and_then(Value::as_str),
                ) {
                    model = Some(SessionModel {
                        provider: provider.to_owned(),
                        model_id: model_id.to_owned(),
                    });
                }
            }
            _ => {}
        }
    }

    let projected = compacted_path(&path);
    let messages = projected
        .into_iter()
        .filter_map(session_entry_to_message)
        .collect();
    SessionContext {
        messages,
        thinking_level,
        model,
    }
}

fn compacted_path<'a>(path: &[&'a ContextEntry]) -> Vec<&'a ContextEntry> {
    let Some((compaction_index, compaction)) = path
        .iter()
        .enumerate()
        .rev()
        .find(|(_, entry)| matches!(entry.known, Some(KnownSessionRecord::Compaction(_))))
    else {
        return path.to_vec();
    };
    let Some(first_kept_id) = compaction
        .raw
        .get("firstKeptEntryId")
        .and_then(Value::as_str)
    else {
        return path.to_vec();
    };
    let Some(first_kept_index) = path.iter().position(|entry| entry.id == first_kept_id) else {
        return path.to_vec();
    };
    let mut result = vec![*compaction];
    result.extend_from_slice(&path[first_kept_index..compaction_index]);
    result.extend_from_slice(&path[compaction_index + 1..]);
    result
}

fn session_entry_to_message(entry: &ContextEntry) -> Option<Value> {
    match &entry.known {
        Some(KnownSessionRecord::Message(_)) => entry.raw.get("message").cloned(),
        Some(KnownSessionRecord::Compaction(value)) => Some(json!({
            "role": "compactionSummary",
            "summary": value.summary,
            "timestamp": value.base.timestamp
        })),
        Some(KnownSessionRecord::BranchSummary(value)) => Some(json!({
            "role": "branchSummary",
            "summary": value.summary,
            "timestamp": value.base.timestamp
        })),
        Some(KnownSessionRecord::CustomMessage(value)) => Some(json!({
            "role": "custom",
            "customType": value.custom_type,
            "content": value.content,
            "display": value.display,
            "timestamp": value.base.timestamp
        })),
        _ => None,
    }
}

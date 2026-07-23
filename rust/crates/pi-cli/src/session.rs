use crate::CliArgs;
use pi_agent::AgentRunResult;
use pi_protocol::{
    ContentBlock, KnownSessionRecord, Message, MessageContent, PersistedSessionRecord,
    SessionEntryBase, SessionHeader, SessionInfoEntry, TextBlock, UserMessage,
};
use pi_runtime::{CompactionRecord, RuntimeBoundaryError, SessionFuture, SessionSink};
use pi_store::{
    SessionFileSnapshot, SessionIdentitySource, SessionRecordFactory, SessionWriter, StoreError,
    StoreErrorCategory, StorePaths,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemIdentity;

impl SessionIdentitySource for SystemIdentity {
    fn next_id(&self) -> String {
        Uuid::new_v4().simple().to_string()[..8].to_owned()
    }

    fn timestamp(&self) -> String {
        now_rfc3339()
    }
}

#[derive(Clone, Debug)]
pub struct CliSession {
    pub header: SessionHeader,
    pub path: Option<PathBuf>,
    pub cwd: PathBuf,
    pub messages: Vec<Message>,
    pub saved_provider: Option<String>,
    pub saved_model: Option<String>,
    pub saved_thinking: Option<String>,
    pub existing: bool,
    writer: Option<Arc<SessionWriter>>,
    parent_id: Option<String>,
}

impl CliSession {
    pub fn sink(&self) -> CliSessionSink {
        CliSessionSink {
            writer: self.writer.clone(),
            parent_id: Mutex::new(self.parent_id.clone()),
            first_recent_id: Mutex::new(None),
            identity: SystemIdentity,
        }
    }

    pub fn append_name(&mut self, name: &str) -> Result<(), StoreError> {
        let Some(writer) = &self.writer else {
            return Ok(());
        };
        let identity = SystemIdentity;
        let mut entry = SessionInfoEntry::new(SessionEntryBase {
            id: identity.next_id(),
            parent_id: self.parent_id.clone(),
            timestamp: identity.timestamp(),
        });
        entry.name = Some(name.replace(['\r', '\n'], " ").trim().to_owned());
        let record = PersistedSessionRecord::from_known(KnownSessionRecord::SessionInfo(entry))
            .map_err(|error| {
                StoreError::new(
                    StoreErrorCategory::InvalidShape,
                    format!("could not create session name entry: {error}"),
                )
            })?;
        writer.append(&record)?;
        self.parent_id = record_id(&record);
        Ok(())
    }

    pub fn append_model_and_thinking(
        &mut self,
        provider: &str,
        model_id: &str,
        thinking: Option<&str>,
    ) -> Result<(), StoreError> {
        let Some(writer) = &self.writer else {
            return Ok(());
        };
        let identity = SystemIdentity;
        let factory = SessionRecordFactory::new(&identity);
        if self.saved_provider.as_deref() != Some(provider)
            || self.saved_model.as_deref() != Some(model_id)
        {
            let record = factory.model_change(
                self.parent_id.clone(),
                provider.to_owned(),
                model_id.to_owned(),
            )?;
            writer.append(&record)?;
            self.parent_id = record_id(&record);
            self.saved_provider = Some(provider.to_owned());
            self.saved_model = Some(model_id.to_owned());
        }
        if let Some(thinking) = thinking
            && self.saved_thinking.as_deref() != Some(thinking)
        {
            let record = factory.thinking_level(self.parent_id.clone(), thinking.to_owned())?;
            writer.append(&record)?;
            self.parent_id = record_id(&record);
            self.saved_thinking = Some(thinking.to_owned());
        }
        Ok(())
    }
}

pub fn open_session(
    args: &CliArgs,
    startup_paths: &StorePaths,
    session_directory: &Path,
) -> Result<CliSession, StoreError> {
    if args.no_session {
        return Ok(ephemeral_session(
            args.session_id.as_deref(),
            &startup_paths.cwd,
        ));
    }

    if let Some(value) = &args.session
        && (value.contains('/') || value.contains('\\') || value.ends_with(".jsonl"))
    {
        let path = startup_paths.resolve_user_path(value)?;
        return if path.is_file() {
            load_existing(path)
        } else {
            create_session_at(&path, &startup_paths.cwd, None)
        };
    }

    let selected = if let Some(value) = &args.session {
        Some(resolve_session(value, startup_paths, session_directory)?)
    } else if let Some(id) = &args.session_id {
        find_session_by_id(session_directory, id, true, Some(&startup_paths.cwd))?
    } else if args.continue_session {
        SessionFileSnapshot::find_most_recent(session_directory, Some(&startup_paths.cwd))?
    } else {
        None
    };

    match selected {
        Some(path) => load_existing(path),
        None => create_session(
            session_directory,
            &startup_paths.cwd,
            args.session_id.as_deref(),
        ),
    }
}

fn ephemeral_session(session_id: Option<&str>, cwd: &Path) -> CliSession {
    let header = SessionHeader::new(
        session_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::now_v7().to_string()),
        now_rfc3339(),
        cwd.display().to_string(),
    );
    CliSession {
        header,
        path: None,
        cwd: cwd.to_path_buf(),
        messages: Vec::new(),
        saved_provider: None,
        saved_model: None,
        saved_thinking: None,
        existing: false,
        writer: None,
        parent_id: None,
    }
}

fn create_session(
    directory: &Path,
    cwd: &Path,
    session_id: Option<&str>,
) -> Result<CliSession, StoreError> {
    let id = session_id
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let timestamp = now_rfc3339();
    let file_timestamp = timestamp.replace([':', '.'], "-");
    let path = directory.join(format!("{file_timestamp}_{id}.jsonl"));
    create_session_at(&path, cwd, Some((id, timestamp)))
}

fn create_session_at(
    path: &Path,
    cwd: &Path,
    identity: Option<(String, String)>,
) -> Result<CliSession, StoreError> {
    let (id, timestamp) = identity.unwrap_or_else(|| (Uuid::now_v7().to_string(), now_rfc3339()));
    let header = SessionHeader::new(id, timestamp, cwd.display().to_string());
    let writer = Arc::new(SessionWriter::create(path, header.clone())?);
    Ok(CliSession {
        header,
        path: Some(path.to_path_buf()),
        cwd: cwd.to_path_buf(),
        messages: Vec::new(),
        saved_provider: None,
        saved_model: None,
        saved_thinking: None,
        existing: false,
        writer: Some(writer),
        parent_id: None,
    })
}

fn load_existing(path: PathBuf) -> Result<CliSession, StoreError> {
    let writer = Arc::new(SessionWriter::open(&path)?);
    let snapshot = writer.snapshot()?;
    let cwd = PathBuf::from(&snapshot.header.cwd);
    if !cwd.is_dir() {
        return Err(StoreError::new(
            StoreErrorCategory::InvalidPath,
            format!(
                "stored session working directory does not exist: {}\nsession file: {}",
                cwd.display(),
                path.display()
            ),
        )
        .with_path(&path));
    }
    let context = snapshot.context(None);
    let messages = context
        .messages
        .iter()
        .filter_map(project_session_message)
        .collect();
    let parent_id = snapshot
        .records()
        .into_iter()
        .filter(|record| !matches!(record.known(), Some(KnownSessionRecord::Header(_))))
        .filter_map(record_id)
        .next_back();
    Ok(CliSession {
        header: snapshot.header,
        path: Some(path),
        cwd,
        messages,
        saved_provider: context.model.as_ref().map(|model| model.provider.clone()),
        saved_model: context.model.as_ref().map(|model| model.model_id.clone()),
        saved_thinking: Some(context.thinking_level),
        existing: true,
        writer: Some(writer),
        parent_id,
    })
}

fn resolve_session(
    value: &str,
    paths: &StorePaths,
    session_directory: &Path,
) -> Result<PathBuf, StoreError> {
    if let Some(path) = find_session_by_id(session_directory, value, false, None)? {
        return Ok(path);
    }
    let global_root = paths.agent_home.join("sessions");
    if global_root != session_directory
        && let Some(path) = find_session_recursively(&global_root, value)?
    {
        return Ok(path);
    }
    Err(StoreError::new(
        StoreErrorCategory::InvalidPath,
        format!("no session found matching {value:?}"),
    ))
}

fn find_session_by_id(
    directory: &Path,
    id: &str,
    exact: bool,
    cwd: Option<&Path>,
) -> Result<Option<PathBuf>, StoreError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::io(error, directory)),
    };
    let mut partial = None;
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io(error, directory))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(snapshot) = SessionFileSnapshot::read(&path) else {
            continue;
        };
        if cwd.is_some_and(|cwd| snapshot.header.cwd != cwd.to_string_lossy()) {
            continue;
        }
        if snapshot.header.id == id {
            return Ok(Some(path));
        }
        if !exact && snapshot.header.id.starts_with(id) && partial.is_none() {
            partial = Some(path);
        }
    }
    Ok(partial)
}

fn find_session_recursively(root: &Path, id: &str) -> Result<Option<PathBuf>, StoreError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::io(error, root)),
    };
    let mut partial = None;
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io(error, root))?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_session_recursively(&path, id)? {
                return Ok(Some(found));
            }
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(snapshot) = SessionFileSnapshot::read(&path) else {
            continue;
        };
        if snapshot.header.id == id {
            return Ok(Some(path));
        }
        if snapshot.header.id.starts_with(id) && partial.is_none() {
            partial = Some(path);
        }
    }
    Ok(partial)
}

fn project_session_message(value: &Value) -> Option<Message> {
    match value.get("role").and_then(Value::as_str) {
        Some("user" | "assistant" | "toolResult") => serde_json::from_value(value.clone()).ok(),
        Some("compactionSummary" | "branchSummary") => {
            let summary = value.get("summary")?.as_str()?;
            Some(Message::User(UserMessage::new(
                MessageContent::Blocks(vec![ContentBlock::Text(TextBlock::new(format!(
                    "Context summary:\n{summary}"
                )))]),
                timestamp_from_value(value),
            )))
        }
        Some("custom") if value.get("display").and_then(Value::as_bool) != Some(false) => {
            let content = value.get("content")?.as_str()?;
            Some(Message::User(UserMessage::new(
                MessageContent::Text(content.to_owned()),
                timestamp_from_value(value),
            )))
        }
        _ => None,
    }
}

fn timestamp_from_value(value: &Value) -> u64 {
    value
        .get("timestamp")
        .and_then(Value::as_u64)
        .unwrap_or_else(now_ms)
}

fn record_id(record: &PersistedSessionRecord) -> Option<String> {
    record
        .raw()
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

pub struct CliSessionSink {
    writer: Option<Arc<SessionWriter>>,
    parent_id: Mutex<Option<String>>,
    first_recent_id: Mutex<Option<String>>,
    identity: SystemIdentity,
}

impl CliSessionSink {
    fn append_message(&self, message: &Message) -> Result<Option<String>, RuntimeBoundaryError> {
        let Some(writer) = &self.writer else {
            return Ok(None);
        };
        let mut parent_id = self
            .parent_id
            .lock()
            .map_err(|_| RuntimeBoundaryError::new("session parent state was poisoned"))?;
        let message = serde_json::to_value(message).map_err(|error| {
            RuntimeBoundaryError::new(format!("could not serialize session message: {error}"))
        })?;
        let record = SessionRecordFactory::new(&self.identity)
            .message(parent_id.clone(), message)
            .map_err(store_boundary)?;
        writer.append(&record).map_err(store_boundary)?;
        let id = record_id(&record);
        parent_id.clone_from(&id);
        Ok(id)
    }
}

impl SessionSink for CliSessionSink {
    fn record_run<'a>(&'a self, run: AgentRunResult) -> SessionFuture<'a> {
        Box::pin(async move {
            let mut first = None;
            for message in &run.new_messages {
                let id = self.append_message(message)?;
                if first.is_none() {
                    first = id;
                }
            }
            if first.is_some() {
                *self.first_recent_id.lock().map_err(|_| {
                    RuntimeBoundaryError::new("session compaction state was poisoned")
                })? = first;
            }
            Ok(())
        })
    }

    fn record_compaction<'a>(&'a self, record: CompactionRecord) -> SessionFuture<'a> {
        Box::pin(async move {
            let Some(writer) = &self.writer else {
                return Ok(());
            };
            let mut parent_id = self
                .parent_id
                .lock()
                .map_err(|_| RuntimeBoundaryError::new("session parent state was poisoned"))?;
            let first_kept = self
                .first_recent_id
                .lock()
                .map_err(|_| RuntimeBoundaryError::new("session compaction state was poisoned"))?
                .clone()
                .or_else(|| parent_id.clone())
                .unwrap_or_else(|| "root".to_owned());
            let persisted = SessionRecordFactory::new(&self.identity)
                .compaction(
                    parent_id.clone(),
                    record.summary,
                    first_kept,
                    record.tokens_before,
                )
                .map_err(store_boundary)?;
            writer.append(&persisted).map_err(store_boundary)?;
            *parent_id = record_id(&persisted);
            Ok(())
        })
    }
}

fn store_boundary(error: StoreError) -> RuntimeBoundaryError {
    RuntimeBoundaryError::new(error.to_string())
}

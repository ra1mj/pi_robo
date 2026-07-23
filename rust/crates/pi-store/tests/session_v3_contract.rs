mod support;

use pi_protocol::{KnownSessionRecord, PersistedSessionRecord, SessionHeader};
use pi_store::{
    SessionFileSnapshot, SessionIdentitySource, SessionRecordFactory, SessionWriter,
    StoreErrorCategory,
};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use support::{TempDir, repository_path};

#[test]
fn session_read_preserves_unknown_and_malformed_lines_with_diagnostics() {
    let temp = TempDir::new("session-read");
    let path = temp.path().join("session.jsonl");
    let mut fixture = std::fs::read(repository_path("rust/fixtures/sessions/session-v3.jsonl"))
        .expect("fixture must exist");
    let malformed = b"not valid json\n";
    fixture.extend_from_slice(malformed);
    std::fs::write(&path, &fixture).expect("fixture must be copied");

    let snapshot = SessionFileSnapshot::read(&path).expect("session must load");

    assert_eq!(snapshot.header.id, "session-1");
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert!(snapshot.lines.iter().any(|line| {
        line.raw_line.contains("\"future_entry\"")
            && line
                .record
                .as_ref()
                .is_some_and(|record| record.known().is_none())
    }));
    assert_eq!(
        std::fs::read(&path).expect("session bytes must remain"),
        fixture
    );
}

#[test]
fn malformed_content_before_the_header_is_never_accepted_as_a_session() {
    let temp = TempDir::new("session-preheader");
    let path = temp.path().join("session.jsonl");
    let bytes = b"not json\n{\"type\":\"session\",\"version\":3,\"id\":\"late\",\"timestamp\":\"2026-07-23T00:00:00.000Z\",\"cwd\":\"/tmp\"}\n";
    std::fs::write(&path, bytes).expect("fixture must be written");

    let error = SessionFileSnapshot::read(&path).expect_err("header must be first");

    assert_eq!(error.category, StoreErrorCategory::InvalidShape);
    assert_eq!(std::fs::read(&path).expect("fixture remains"), bytes);
}

#[test]
fn context_follows_unknown_structural_entries_and_latest_compaction() {
    let snapshot =
        SessionFileSnapshot::read(repository_path("rust/fixtures/sessions/session-v3.jsonl"))
            .expect("session fixture must load");

    let context = snapshot.context(None);

    assert_eq!(context.thinking_level, "high");
    assert_eq!(
        context.model.as_ref().map(|model| model.provider.as_str()),
        Some("openai")
    );
    assert_eq!(context.messages[0]["role"], "compactionSummary");
    assert!(
        context
            .messages
            .iter()
            .any(|message| message["role"] == "branchSummary")
    );
    assert!(
        context
            .messages
            .iter()
            .any(|message| message["customType"] == "notice")
    );
}

#[test]
fn append_is_compact_prefix_preserving_and_rejects_stale_writer() {
    let temp = TempDir::new("session-append");
    let path = temp.path().join("session.jsonl");
    let original = std::fs::read(repository_path("rust/fixtures/sessions/session-v3.jsonl"))
        .expect("fixture must exist");
    std::fs::write(&path, &original).expect("fixture must be copied");
    let writer = SessionWriter::open(&path).expect("writer must open");
    let record = PersistedSessionRecord::parse(
        r#"{"type":"custom","id":"rust-1","parentId":"entry-10","timestamp":"2026-07-23T00:00:00.000Z","customType":"rust_append","data":{"ok":true}}"#,
    )
    .expect("record must parse");

    writer.append(&record).expect("append must succeed");

    let appended = std::fs::read(&path).expect("session must be readable");
    assert!(appended.starts_with(&original));
    assert_eq!(
        &appended[original.len()..],
        b"{\"customType\":\"rust_append\",\"data\":{\"ok\":true},\"id\":\"rust-1\",\"parentId\":\"entry-10\",\"timestamp\":\"2026-07-23T00:00:00.000Z\",\"type\":\"custom\"}\n"
    );

    let mut external = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("external writer must open");
    external
        .write_all(
            b"{\"type\":\"future_entry\",\"id\":\"external\",\"parentId\":\"rust-1\",\"timestamp\":\"2026-07-23T00:00:01.000Z\"}\n",
        )
        .expect("external append must succeed");
    drop(external);
    let error = writer
        .append(&record)
        .expect_err("stale writer must be rejected");
    assert_eq!(error.category, StoreErrorCategory::StaleSession);
}

#[test]
fn legacy_sessions_are_readable_but_never_mutated_or_migrated() {
    let temp = TempDir::new("session-legacy");
    let path = temp.path().join("legacy.jsonl");
    let bytes =
        b"{\"type\":\"session\",\"version\":2,\"id\":\"legacy\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n";
    std::fs::write(&path, bytes).expect("legacy fixture must be written");

    let snapshot = SessionFileSnapshot::read(&path).expect("legacy session can be inspected");
    assert_eq!(snapshot.legacy_version, Some(2));
    let error = SessionWriter::open(&path).expect_err("legacy append must be refused");
    assert_eq!(error.category, StoreErrorCategory::UnsupportedVersion);
    assert_eq!(
        std::fs::read(&path).expect("legacy file must remain"),
        bytes
    );
}

#[test]
fn creating_a_v3_session_never_rewrites_an_existing_path() {
    let temp = TempDir::new("session-create");
    let path = temp.path().join("sessions").join("new.jsonl");
    let header = SessionHeader::new(
        "new-session",
        "2026-07-23T00:00:00.000Z",
        "/synthetic/project",
    );

    let writer = SessionWriter::create(&path, header).expect("new session must be created");

    assert_eq!(
        writer.snapshot().expect("snapshot is available").header.id,
        "new-session"
    );
    let error = SessionWriter::create(
        &path,
        match PersistedSessionRecord::parse(
            r#"{"type":"session","version":3,"id":"other","timestamp":"2026-07-23T00:00:00.000Z","cwd":"/tmp"}"#,
        )
        .expect("header parses")
        .known()
        .expect("header is known")
        {
            KnownSessionRecord::Header(header) => header.clone(),
            _ => panic!("record must be a header"),
        },
    )
    .expect_err("existing session path must not be replaced");
    assert_eq!(error.category, StoreErrorCategory::Io);
}

#[test]
fn supported_records_use_the_injected_identity_and_timestamp_source() {
    let source = FixedIdentitySource {
        next: AtomicUsize::new(1),
    };
    let factory = SessionRecordFactory::new(&source);

    let message = factory
        .message(
            None,
            serde_json::json!({"role":"user","content":"hello","timestamp":1}),
        )
        .expect("message record builds");
    let model = factory
        .model_change(Some("entry-1".to_owned()), "openai", "fixture-model")
        .expect("model record builds");

    assert_eq!(message.raw()["id"], "entry-1");
    assert_eq!(message.raw()["timestamp"], "2026-07-23T00:00:00.000Z");
    assert_eq!(model.raw()["id"], "entry-2");
    assert_eq!(model.raw()["parentId"], "entry-1");
}

struct FixedIdentitySource {
    next: AtomicUsize,
}

impl SessionIdentitySource for FixedIdentitySource {
    fn next_id(&self) -> String {
        format!("entry-{}", self.next.fetch_add(1, Ordering::Relaxed))
    }

    fn timestamp(&self) -> String {
        "2026-07-23T00:00:00.000Z".to_owned()
    }
}

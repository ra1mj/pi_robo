mod support;

use pi_protocol::{PersistedSessionRecord, SessionHeader};
use pi_store::{SessionFileSnapshot, SessionWriter};
use serde_json::Value;
use std::process::Command;
use support::{TempDir, repository_path};

fn run_typescript(command: &str, path: &std::path::Path) -> Value {
    let output = Command::new(repository_path("node_modules/.bin/tsx"))
        .arg(repository_path("rust/fixtures/runners/session-interop.ts"))
        .arg(command)
        .arg(path)
        .current_dir(repository_path(""))
        .output()
        .expect("Node must run the TypeScript interoperability fixture");
    assert!(
        output.status.success(),
        "TypeScript fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("TypeScript fixture must emit JSON")
}

#[test]
fn typescript_reads_rust_append_and_rust_reads_typescript_append() {
    let temp = TempDir::new("typescript-interop");
    let typescript_created = temp.path().join("typescript-created.jsonl");
    let original = std::fs::read(repository_path("rust/fixtures/sessions/session-v3.jsonl"))
        .expect("fixture must exist");
    std::fs::write(&typescript_created, &original).expect("fixture must be copied");
    let before = run_typescript("read", &typescript_created);
    assert_eq!(before["sessionId"], "session-1");

    let writer = SessionWriter::open(&typescript_created).expect("Rust writer must open");
    let rust_record = PersistedSessionRecord::parse(
        r#"{"type":"custom","id":"rust-interop","parentId":"entry-10","timestamp":"2026-07-23T00:00:00.000Z","customType":"rust_append","data":{"source":"rust"}}"#,
    )
    .expect("Rust record must parse");
    writer
        .append(&rust_record)
        .expect("Rust append must succeed");
    let after_rust = run_typescript("read", &typescript_created);
    assert!(
        after_rust["entries"]
            .as_array()
            .expect("entries must be an array")
            .iter()
            .any(|entry| entry["customType"] == "rust_append")
    );
    assert!(
        std::fs::read(&typescript_created)
            .expect("session remains readable")
            .starts_with(&original)
    );

    let rust_created = temp.path().join("rust-created.jsonl");
    SessionWriter::create(
        &rust_created,
        SessionHeader::new(
            "rust-created",
            "2026-07-23T00:00:00.000Z",
            "/synthetic/project",
        ),
    )
    .expect("Rust session must be created");
    run_typescript("append", &rust_created);
    let snapshot = SessionFileSnapshot::read(&rust_created).expect("Rust must reload TS append");
    assert!(snapshot.records().iter().any(|record| {
        record.raw().get("customType").and_then(Value::as_str) == Some("typescript_append")
    }));
}

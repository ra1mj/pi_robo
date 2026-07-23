mod support;

use pi_cli::RootCancellation;
use pi_protocol::SessionHeader;
use pi_store::SessionWriter;
use serde_json::json;
use support::{InjectedFactory, TempDir, response, run, tool_response};

#[tokio::test]
async fn resumed_session_cwd_is_authoritative_for_runtime_tools() {
    let root = TempDir::new("authoritative-cwd");
    let startup_cwd = root.path().join("startup");
    let session_cwd = root.path().join("session-project");
    let session_dir = root.path().join("sessions");
    std::fs::create_dir_all(&startup_cwd).expect("startup cwd");
    std::fs::create_dir_all(&session_cwd).expect("session cwd");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    let session_path = session_dir.join("existing.jsonl");
    let writer = SessionWriter::create(
        &session_path,
        SessionHeader::new(
            "existing-session",
            "2026-07-23T00:00:00Z",
            session_cwd.display().to_string(),
        ),
    )
    .expect("session fixture");
    drop(writer);

    let factory = InjectedFactory::faux(vec![
        tool_response(
            "write",
            json!({ "path": "cwd-proof.txt", "content": "session cwd" }),
        ),
        response("done"),
    ]);
    let result = run(
        &root,
        &startup_cwd,
        &[
            "--session",
            session_path.to_str().expect("UTF-8 path"),
            "--no-context-files",
            "--no-skills",
            "--provider",
            "openai",
            "--model",
            "gpt-4.1",
            "--tools",
            "write",
            "-p",
            "write proof",
        ],
        None,
        true,
        &factory,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(session_cwd.join("cwd-proof.txt").is_file());
    assert!(!startup_cwd.join("cwd-proof.txt").exists());
}

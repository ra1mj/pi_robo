mod support;

use pi_cli::RootCancellation;
use pi_store::SessionFileSnapshot;
use support::{InjectedFactory, TempDir, response, run};

#[tokio::test]
async fn exact_session_id_creates_then_continue_appends_v3_records() {
    let root = TempDir::new("session");
    let cwd = root.path().join("project");
    let session_dir = root.path().join("sessions");
    let first = InjectedFactory::faux(vec![response("one")]);
    let result = run(
        &root,
        &cwd,
        &[
            "--session-dir",
            session_dir.to_str().expect("UTF-8 path"),
            "--session-id",
            "contract-session",
            "--no-context-files",
            "--no-skills",
            "--provider",
            "openai",
            "--model",
            "gpt-4.1",
            "-p",
            "first",
        ],
        None,
        true,
        &first,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    let session_path = std::fs::read_dir(&session_dir)
        .expect("session directory")
        .next()
        .expect("session file")
        .expect("directory entry")
        .path();

    let second = InjectedFactory::faux(vec![response("two")]);
    let result = run(
        &root,
        &cwd,
        &[
            "--session-dir",
            session_dir.to_str().expect("UTF-8 path"),
            "--continue",
            "--no-context-files",
            "--no-skills",
            "--provider",
            "openai",
            "--model",
            "gpt-4.1",
            "-p",
            "second",
        ],
        None,
        true,
        &second,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    let snapshot = SessionFileSnapshot::read(session_path).expect("valid v3 session");
    assert_eq!(snapshot.header.id, "contract-session");
    assert!(snapshot.context(None).messages.len() >= 4);
}

#[tokio::test]
async fn explicit_missing_session_path_is_created_and_reopenable() {
    let root = TempDir::new("session-path");
    let cwd = root.path().join("project");
    let session_path = root.path().join("custom").join("session.jsonl");
    let first = InjectedFactory::faux(vec![response("one")]);
    let result = run(
        &root,
        &cwd,
        &[
            "--session",
            session_path.to_str().expect("UTF-8 path"),
            "--no-context-files",
            "--no-skills",
            "--provider",
            "openai",
            "--model",
            "gpt-4.1",
            "-p",
            "first",
        ],
        None,
        true,
        &first,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(session_path.is_file());

    let second = InjectedFactory::faux(vec![response("two")]);
    let result = run(
        &root,
        &cwd,
        &[
            "--session",
            session_path.to_str().expect("UTF-8 path"),
            "--no-context-files",
            "--no-skills",
            "--provider",
            "openai",
            "--model",
            "gpt-4.1",
            "-p",
            "second",
        ],
        None,
        true,
        &second,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    let snapshot = SessionFileSnapshot::read(session_path).expect("valid v3 session");
    assert!(snapshot.context(None).messages.len() >= 4);
}

mod support;

use pi_cli::RootCancellation;
use serde_json::Value;
use support::{InjectedFactory, TempDir, response, run};

#[tokio::test]
async fn text_mode_writes_only_the_final_assistant_text() {
    let root = TempDir::new("text-output");
    let cwd = root.path().join("project");
    let factory = InjectedFactory::faux(vec![response("first"), response("second")]);
    let result = run(
        &root,
        &cwd,
        &[
            "--no-session",
            "--no-context-files",
            "--no-skills",
            "--provider",
            "openai",
            "--model",
            "gpt-4.1",
            "-p",
            "one",
            "two",
        ],
        None,
        true,
        &factory,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0);
    assert_eq!(result.stdout, "second\n");
    assert_eq!(result.stderr, "");
}

#[tokio::test]
async fn json_mode_starts_with_v3_header_and_emits_compatible_events() {
    let root = TempDir::new("json-output");
    let cwd = root.path().join("project");
    let factory = InjectedFactory::faux(vec![response("answer")]);
    let result = run(
        &root,
        &cwd,
        &[
            "--no-session",
            "--no-context-files",
            "--no-skills",
            "--provider",
            "openai",
            "--model",
            "gpt-4.1",
            "--mode",
            "json",
            "question",
        ],
        None,
        true,
        &factory,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0);
    assert_eq!(result.stderr, "");
    let records: Vec<Value> = result
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSONL record"))
        .collect();
    assert_eq!(records[0]["type"], "session");
    assert_eq!(records[0]["version"], 3);
    let event_types: Vec<&str> = records
        .iter()
        .skip(1)
        .filter_map(|record| record["type"].as_str())
        .collect();
    assert_eq!(event_types.first(), Some(&"agent_start"));
    assert!(event_types.contains(&"message_update"));
    assert_eq!(event_types.last(), Some(&"agent_end"));
}

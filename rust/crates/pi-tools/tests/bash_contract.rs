mod support;

use pi_agent::{Tool, ToolErrorCategory};
use pi_test_support::FakeCancellation;
use pi_tools::{BashTool, BashToolConfig};
use serde_json::json;
use support::{RecordingUpdates, TempRoot, call, output_text};

fn tool(root: &TempRoot) -> BashTool {
    let mut config = BashToolConfig::new(root.path());
    config.command_prefix = Some("export PI_PREFIX_MARKER=ready".to_owned());
    config.temp_dir = root.path().join("output");
    BashTool::new(config)
}

#[tokio::test]
async fn streams_combined_output_and_applies_prefix_once() {
    let root = TempRoot::new("bash-success");
    let updates = RecordingUpdates::default();
    let output = tool(&root)
        .execute(
            &call(
                "bash",
                json!({ "command": "printf '%s\\n' \"$PI_PREFIX_MARKER\"; printf 'stderr\\n' >&2" }),
            ),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect("bash output");
    let text = output_text(&output);
    assert!(text.contains("ready"));
    assert!(text.contains("stderr"));
    assert!(updates.snapshot().len() >= 2);
}

#[tokio::test]
async fn reports_nonzero_exit_invalid_timeout_and_missing_cwd() {
    let root = TempRoot::new("bash-errors");
    let tool = tool(&root);
    let updates = RecordingUpdates::default();
    let error = tool
        .execute(
            &call("bash", json!({ "command": "printf boom; exit 7" })),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect_err("nonzero exit");
    assert!(error.message.contains("boom"));
    assert!(error.message.contains("code 7"));

    let timeout = tool
        .execute(
            &call("bash", json!({ "command": "true", "timeout": 0 })),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect_err("invalid timeout");
    assert_eq!(timeout.category, ToolErrorCategory::InvalidArguments);

    let missing = BashTool::new(BashToolConfig::new(root.path().join("missing")))
        .execute(
            &call("bash", json!({ "command": "true" })),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect_err("missing cwd");
    assert!(missing.message.contains("Working directory does not exist"));
}

#[tokio::test]
async fn truncates_to_tail_and_persists_full_output() {
    let root = TempRoot::new("bash-truncate");
    let output = tool(&root)
        .execute(
            &call(
                "bash",
                json!({ "command": "i=1; while [ $i -le 2100 ]; do printf 'line-%s\\n' \"$i\"; i=$((i+1)); done" }),
            ),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("truncated output");
    let text = output_text(&output);
    assert!(!text.contains("line-1\n"));
    assert!(text.contains("line-2100"));
    let details = output.details.expect("bash details");
    assert_eq!(details["truncation"]["truncated"], true);
    let full_path = details["fullOutputPath"]
        .as_str()
        .expect("full output path");
    let full = std::fs::read_to_string(full_path).expect("full output");
    assert!(full.starts_with("line-1\n"));
    assert!(full.ends_with("line-2100\n"));
}

#[tokio::test]
async fn drains_output_from_a_descendant_after_the_shell_exits() {
    let root = TempRoot::new("bash-late-output");
    let output = tool(&root)
        .execute(
            &call(
                "bash",
                json!({ "command": "(sleep 0.03; printf late) & printf early" }),
            ),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("late output");
    assert!(output_text(&output).contains("early"));
    assert!(output_text(&output).contains("late"));
}

mod support;

use pi_agent::{Tool, ToolErrorCategory};
use pi_test_support::FakeCancellation;
use pi_tools::{BashTool, BashToolConfig};
use serde_json::json;
use std::time::Duration;
use support::{RecordingUpdates, TempRoot, call};

#[tokio::test]
async fn cancellation_terminates_the_shell_process_group() {
    let root = TempRoot::new("bash-process-tree");
    let pid_file = root.path().join("child.pid");
    let cancellation = FakeCancellation::default();
    let updates = RecordingUpdates::default();
    let tool = BashTool::new(BashToolConfig::new(root.path()));
    let tool_call = call(
        "bash",
        json!({ "command": "sleep 30 & child=$!; printf '%s' \"$child\" > child.pid; wait \"$child\"" }),
    );
    let execution = tool.execute(&tool_call, &cancellation, &updates);
    let cancel_when_started = async {
        for _ in 0..100 {
            if pid_file.is_file() {
                cancellation.cancel();
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("child pid file was not created");
    };
    let (result, ()) = tokio::join!(execution, cancel_when_started);
    let error = result.expect_err("cancelled bash");
    assert_eq!(error.category, ToolErrorCategory::Cancelled);
    let child_pid = std::fs::read_to_string(&pid_file).expect("child pid");
    let process_path = std::path::PathBuf::from(format!("/proc/{}", child_pid.trim()));
    for _ in 0..50 {
        if !process_path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "descendant process survived cancellation: {}",
        child_pid.trim()
    );
}

#[tokio::test]
async fn timeout_terminates_the_process_and_preserves_output() {
    let root = TempRoot::new("bash-timeout");
    let error = BashTool::new(BashToolConfig::new(root.path()))
        .execute(
            &call(
                "bash",
                json!({ "command": "printf before; sleep 30", "timeout": 0.02 }),
            ),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect_err("timed out bash");
    assert!(error.message.contains("before"));
    assert!(error.message.contains("timed out after 0.02 seconds"));
}

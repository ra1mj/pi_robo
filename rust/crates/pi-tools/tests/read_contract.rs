mod support;

use pi_agent::{Tool, ToolErrorCategory};
use pi_test_support::FakeCancellation;
use pi_tools::{ImagePolicy, ReadTool};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use support::{RecordingUpdates, TempRoot, call, output_text};

#[tokio::test]
async fn reads_ranges_and_reports_continuation() {
    let root = TempRoot::new("read-range");
    std::fs::write(root.path().join("file.txt"), "one\ntwo\nthree\nfour").expect("seed file");
    let tool = ReadTool::new(root.path(), ImagePolicy::default());
    let output = tool
        .execute(
            &call(
                "read",
                json!({ "path": "file.txt", "offset": 2, "limit": 2 }),
            ),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("read output");
    assert_eq!(
        output_text(&output),
        "two\nthree\n\n[1 more lines in file. Use offset=4 to continue.]"
    );
}

#[tokio::test]
async fn rejects_invalid_missing_unreadable_and_cancelled_reads() {
    let root = TempRoot::new("read-errors");
    let unreadable = root.path().join("private.txt");
    std::fs::write(&unreadable, "secret").expect("seed private file");
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000))
        .expect("remove permissions");
    let tool = ReadTool::new(root.path(), ImagePolicy::default());
    let updates = RecordingUpdates::default();

    let invalid = tool
        .execute(
            &call("read", json!({ "path": "x", "extra": true })),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect_err("invalid arguments");
    assert_eq!(invalid.category, ToolErrorCategory::InvalidArguments);

    let missing = tool
        .execute(
            &call("read", json!({ "path": "missing.txt" })),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect_err("missing path");
    assert_eq!(missing.category, ToolErrorCategory::Execution);

    let permission = tool
        .execute(
            &call("read", json!({ "path": "private.txt" })),
            &FakeCancellation::default(),
            &updates,
        )
        .await;
    assert!(permission.is_err(), "permission failure must be surfaced");

    let cancellation = FakeCancellation::default();
    cancellation.cancel();
    let cancelled = tool
        .execute(
            &call("read", json!({ "path": "private.txt" })),
            &cancellation,
            &updates,
        )
        .await
        .expect_err("cancelled read");
    assert_eq!(cancelled.category, ToolErrorCategory::Cancelled);
}

#[tokio::test]
async fn truncates_at_complete_lines_and_rejects_out_of_range_offset() {
    let root = TempRoot::new("read-truncate");
    let large = (0..2_100)
        .map(|index| format!("line-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(root.path().join("large.txt"), large).expect("seed large file");
    let tool = ReadTool::new(root.path(), ImagePolicy::default());
    let output = tool
        .execute(
            &call("read", json!({ "path": "large.txt" })),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("truncated read");
    assert!(output_text(&output).contains("Use offset=2001 to continue"));
    assert!(output.details.is_some());

    let error = tool
        .execute(
            &call("read", json!({ "path": "large.txt", "offset": 9999 })),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect_err("offset error");
    assert!(error.message.contains("beyond end of file"));
}

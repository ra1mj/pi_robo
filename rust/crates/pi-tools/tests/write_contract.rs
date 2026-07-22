mod support;

use pi_agent::{Tool, ToolErrorCategory};
use pi_test_support::FakeCancellation;
use pi_tools::{MutationCoordinator, WriteTool};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use support::{RecordingUpdates, TempRoot, call, output_text};

#[tokio::test]
async fn creates_parents_and_writes_complete_utf8_content() {
    let root = TempRoot::new("write-success");
    let tool = WriteTool::new(root.path(), MutationCoordinator::default());
    let output = tool
        .execute(
            &call(
                "write",
                json!({ "path": "nested/file.txt", "content": "hello 甲" }),
            ),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("write output");
    assert_eq!(
        std::fs::read_to_string(root.path().join("nested/file.txt")).expect("written file"),
        "hello 甲"
    );
    assert!(output_text(&output).contains("Successfully wrote"));
}

#[tokio::test]
async fn follows_existing_symlinks_without_replacing_them() {
    let root = TempRoot::new("write-symlink");
    let target = root.path().join("target.txt");
    let link = root.path().join("link.txt");
    std::fs::write(&target, "before").expect("seed target");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    WriteTool::new(root.path(), MutationCoordinator::default())
        .execute(
            &call("write", json!({ "path": "link.txt", "content": "after" })),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("write through symlink");
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("link metadata")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(target).expect("target content"),
        "after"
    );
}

#[tokio::test]
async fn rejects_invalid_permission_and_cancelled_writes() {
    let root = TempRoot::new("write-errors");
    let locked = root.path().join("locked");
    std::fs::create_dir(&locked).expect("locked directory");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500))
        .expect("lock directory");
    let tool = WriteTool::new(root.path(), MutationCoordinator::default());
    let updates = RecordingUpdates::default();

    let invalid = tool
        .execute(
            &call("write", json!({ "path": "x" })),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect_err("invalid write");
    assert_eq!(invalid.category, ToolErrorCategory::InvalidArguments);

    let permission = tool
        .execute(
            &call("write", json!({ "path": "locked/x", "content": "x" })),
            &FakeCancellation::default(),
            &updates,
        )
        .await
        .expect_err("permission error");
    assert_eq!(permission.category, ToolErrorCategory::Execution);

    let cancellation = FakeCancellation::default();
    cancellation.cancel();
    let cancelled = tool
        .execute(
            &call("write", json!({ "path": "x", "content": "x" })),
            &cancellation,
            &updates,
        )
        .await
        .expect_err("cancelled write");
    assert_eq!(cancelled.category, ToolErrorCategory::Cancelled);
}

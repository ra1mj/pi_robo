mod support;

use pi_agent::{Tool, ToolErrorCategory};
use pi_test_support::FakeCancellation;
use pi_tools::{EditTool, MutationCoordinator};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use support::{RecordingUpdates, TempRoot, call, output_text};

#[tokio::test]
async fn applies_multiple_edits_and_preserves_bom_and_crlf() {
    let root = TempRoot::new("edit-success");
    let path = root.path().join("file.txt");
    std::fs::write(&path, "\u{feff}alpha\r\nbeta\r\ngamma\r\n").expect("seed file");
    let tool = EditTool::new(root.path(), MutationCoordinator::default());
    let output = tool
        .execute(
            &call(
                "edit",
                json!({
                    "path": "file.txt",
                    "edits": [
                        { "oldText": "alpha", "newText": "first" },
                        { "oldText": "gamma", "newText": "last" }
                    ]
                }),
            ),
            &FakeCancellation::default(),
            &RecordingUpdates::default(),
        )
        .await
        .expect("edit output");
    assert_eq!(
        std::fs::read_to_string(path).expect("edited file"),
        "\u{feff}first\r\nbeta\r\nlast\r\n"
    );
    assert_eq!(
        output_text(&output),
        "Successfully replaced 2 block(s) in file.txt."
    );
    let details = output.details.expect("edit details");
    assert!(details["diff"].as_str().unwrap().contains("-alpha"));
    assert!(
        details["patch"]
            .as_str()
            .unwrap()
            .starts_with("--- a/file.txt")
    );
    assert_eq!(details["firstChangedLine"], 1);
}

#[tokio::test]
async fn supports_stringified_legacy_and_fuzzy_inputs() {
    let root = TempRoot::new("edit-compat");
    let path = root.path().join("file.txt");
    std::fs::write(&path, "title\nHe said “hello”.   \nend\n").expect("seed file");
    let tool = EditTool::new(root.path(), MutationCoordinator::default());
    tool.execute(
        &call(
            "edit",
            json!({
                "path": "file.txt",
                "edits": "[{\"oldText\":\"He said \\\"hello\\\".\",\"newText\":\"greeting\"}]"
            }),
        ),
        &FakeCancellation::default(),
        &RecordingUpdates::default(),
    )
    .await
    .expect("stringified fuzzy edit");
    assert_eq!(
        std::fs::read_to_string(&path).expect("fuzzy result"),
        "title\ngreeting   \nend\n"
    );

    tool.execute(
        &call(
            "edit",
            json!({
                "path": "file.txt",
                "edits": [],
                "oldText": "title",
                "newText": "heading"
            }),
        ),
        &FakeCancellation::default(),
        &RecordingUpdates::default(),
    )
    .await
    .expect("legacy edit");
    assert!(
        std::fs::read_to_string(path)
            .expect("legacy result")
            .starts_with("heading\n")
    );
}

#[tokio::test]
async fn rejects_missing_duplicate_overlapping_noop_and_unwritable_edits() {
    let root = TempRoot::new("edit-errors");
    let path = root.path().join("file.txt");
    std::fs::write(&path, "abc abc").expect("seed file");
    let tool = EditTool::new(root.path(), MutationCoordinator::default());
    let updates = RecordingUpdates::default();
    let cancellation = FakeCancellation::default();

    for edits in [
        json!([{ "oldText": "missing", "newText": "x" }]),
        json!([{ "oldText": "abc", "newText": "x" }]),
        json!([
            { "oldText": "abc abc", "newText": "x" },
            { "oldText": "abc", "newText": "y" }
        ]),
        json!([{ "oldText": "abc", "newText": "abc" }]),
    ] {
        assert!(
            tool.execute(
                &call("edit", json!({ "path": "file.txt", "edits": edits })),
                &cancellation,
                &updates,
            )
            .await
            .is_err()
        );
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("unchanged file"),
        "abc abc"
    );

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).expect("make readonly");
    let permission = tool
        .execute(
            &call(
                "edit",
                json!({ "path": "file.txt", "edits": [{ "oldText": "abc abc", "newText": "x" }] }),
            ),
            &cancellation,
            &updates,
        )
        .await
        .expect_err("permission error");
    assert_eq!(permission.category, ToolErrorCategory::Execution);
}

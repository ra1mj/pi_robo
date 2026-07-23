mod support;

use pi_cli::RootCancellation;
use serde_json::json;
use support::{InjectedFactory, TempDir, response, run, tool_response};

#[tokio::test]
async fn explicit_tool_filters_and_write_tool_run_end_to_end() {
    let root = TempDir::new("tool");
    let cwd = root.path().join("project");
    let factory = InjectedFactory::faux(vec![
        tool_response(
            "write",
            json!({ "path": "result.txt", "content": "written" }),
        ),
        response("done"),
    ]);
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
            "--tools",
            "write",
            "-p",
            "write it",
        ],
        None,
        true,
        &factory,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        std::fs::read_to_string(cwd.join("result.txt")).expect("written file"),
        "written"
    );
    assert_eq!(result.stdout, "done\n");
}

mod support;

use pi_cli::RootCancellation;
use support::{InjectedFactory, TempDir, response, run};

#[tokio::test]
async fn root_cancellation_maps_signal_status_and_settles_output() {
    let root = TempDir::new("signal");
    let cwd = root.path().join("project");
    let cancellation = RootCancellation::default();
    cancellation.cancel(130);
    let factory = InjectedFactory::faux(vec![response("unused")]);
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
            "cancel",
        ],
        None,
        true,
        &factory,
        &cancellation,
    )
    .await;
    assert_eq!(result.code, 130);
    assert!(result.stderr.contains("request cancelled"));
}

mod support;

use pi_cli::RootCancellation;
use pi_model::{ModelServiceError, ModelServiceErrorCategory};
use pi_test_support::FauxResponse;
use support::{InjectedFactory, TempDir, response, run};

#[tokio::test]
async fn injected_faux_completes_text_and_json_headless_runs() {
    for mode in ["text", "json"] {
        let root = TempDir::new(mode);
        let cwd = root.path().join("project");
        let factory = InjectedFactory::faux(vec![response("ok")]);
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
                mode,
                "say ok",
            ],
            None,
            true,
            &factory,
            &RootCancellation::default(),
        )
        .await;
        assert_eq!(result.code, 0, "mode={mode}: {}", result.stderr);
        assert!(result.stdout.contains("ok"));
    }
}

#[tokio::test]
async fn piped_stdin_selects_text_and_precedes_the_first_argument() {
    let root = TempDir::new("stdin");
    let cwd = root.path().join("project");
    let factory = InjectedFactory::faux(vec![response("ok")]);
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
            "summarize",
        ],
        Some("input"),
        false,
        &factory,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0);
    assert_eq!(result.stdout, "ok\n");
}

#[tokio::test]
async fn retry_events_are_emitted_without_a_production_delay_in_fixture_settings() {
    let root = TempDir::new("retry");
    let cwd = root.path().join("project");
    let agent = root.path().join("home").join(".pi").join("agent");
    std::fs::create_dir_all(&agent).expect("agent");
    std::fs::write(
        agent.join("settings.json"),
        r#"{"retry":{"baseDelayMs":0,"maxRetries":3}}"#,
    )
    .expect("settings");
    let factory = InjectedFactory::faux(vec![
        FauxResponse::Error(ModelServiceError::new(
            ModelServiceErrorCategory::RateLimit,
            "retry me",
            true,
        )),
        response("recovered"),
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
            "--mode",
            "json",
            "retry",
        ],
        None,
        true,
        &factory,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stdout.contains("\"type\":\"auto_retry_start\""));
    assert!(result.stdout.contains("\"type\":\"auto_retry_end\""));
}

#[tokio::test]
async fn threshold_compaction_runs_through_the_injected_model_boundary() {
    let root = TempDir::new("compaction");
    let cwd = root.path().join("project");
    let agent = root.path().join("home").join(".pi").join("agent");
    std::fs::create_dir_all(&agent).expect("agent");
    std::fs::write(
        agent.join("models.json"),
        r#"{"providers":{"tiny":{"api":"openai-responses","baseUrl":"http://127.0.0.1:1/v1","models":[{"id":"tiny","name":"Tiny","reasoning":false,"input":["text"],"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"contextWindow":1,"maxTokens":128}]}}}"#,
    )
    .expect("models");
    std::fs::write(
        agent.join("settings.json"),
        r#"{"compaction":{"enabled":true,"reserveTokens":0,"keepRecentTokens":100}}"#,
    )
    .expect("settings");
    let factory = InjectedFactory::faux(vec![response("answer"), response("summary")]);
    let result = run(
        &root,
        &cwd,
        &[
            "--no-session",
            "--no-context-files",
            "--no-skills",
            "--provider",
            "tiny",
            "--model",
            "tiny",
            "--mode",
            "json",
            "compact",
        ],
        None,
        true,
        &factory,
        &RootCancellation::default(),
    )
    .await;
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stdout.contains("\"type\":\"compaction_start\""));
    assert!(result.stdout.contains("\"type\":\"compaction_end\""));
}

mod support;

use pi_store::{StorePaths, load_model_sources, strip_json_comments};
use support::{TempDir, repository_path};

#[test]
fn models_json_comments_overrides_and_custom_defaults_match_contract() {
    let temp = TempDir::new("models");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&agent).expect("agent directory must be created");
    let fixture =
        std::fs::read(repository_path("rust/fixtures/store/models.json")).expect("fixture exists");
    std::fs::write(agent.join("models.json"), &fixture).expect("models fixture must be written");
    let paths = StorePaths::new(&agent, &cwd, &home).expect("paths must resolve");

    let snapshot = load_model_sources(&paths).expect("models must load");

    let custom = &snapshot.catalog.providers["fixture-custom"].models["fixture-model"];
    assert_eq!(custom.name, "Fixture Model");
    assert_eq!(custom.context_window, 128_000);
    assert_eq!(custom.max_tokens, 16_384);
    assert_eq!(
        custom.extensions["futureModelField"]["preserved"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        snapshot.configured_api_key("fixture-custom"),
        Some("$FIXTURE_CUSTOM_KEY")
    );
    assert!(
        snapshot
            .supported_model("fixture-custom", "fixture-model")
            .is_some()
    );
    assert!(
        snapshot
            .supported_model("openrouter", "anthropic/claude-sonnet-4")
            .is_none()
    );
    assert!(!format!("{snapshot:?}").contains("$FIXTURE_CUSTOM_KEY"));
    let openai = &snapshot.catalog.providers["openai"].models["gpt-4.1"];
    assert_eq!(openai.name, "Fixture GPT");
    assert_eq!(openai.base_url, "https://proxy.example.test/v1");
    assert_eq!(
        std::fs::read(agent.join("models.json")).expect("models file remains"),
        fixture
    );
}

#[test]
fn comment_stripping_preserves_urls_and_string_literals() {
    let source = r#"{"url":"https://example.test/a//b","value":"//literal",}// comment
"#;
    let stripped = strip_json_comments(source);
    let parsed: serde_json::Value =
        serde_json::from_str(&stripped).expect("stripped JSON must parse");
    assert_eq!(parsed["url"], "https://example.test/a//b");
    assert_eq!(parsed["value"], "//literal");
}

#[test]
fn unsupported_custom_protocol_is_diagnosed_without_advertising_model() {
    let temp = TempDir::new("models-unsupported");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    std::fs::create_dir_all(&agent).expect("agent directory must be created");
    std::fs::write(
        agent.join("models.json"),
        r#"{"providers":{"custom":{"baseUrl":"https://example.test","api":"unsupported","models":[{"id":"bad"}]}}}"#,
    )
    .expect("models fixture must be written");
    let paths =
        StorePaths::new(&agent, temp.path().join("project"), &home).expect("paths must resolve");

    let snapshot = load_model_sources(&paths).expect("source loads with diagnostics");

    assert!(
        snapshot
            .catalog
            .providers
            .get("custom")
            .is_none_or(|provider| provider.models.is_empty())
    );
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unsupported milestone-1 api"))
    );
}

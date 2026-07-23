mod support;

use pi_store::{
    AuthDocument, CommandCancellation, CommandRequest, CommandResult, CredentialRequest,
    CredentialSource, NeverCancelled, ProcessFuture, ProcessRunner, StoreErrorCategory, StorePaths,
    resolve_credential,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Mutex;
use support::{TempDir, repository_path};

#[derive(Debug)]
struct ScriptedRunner {
    result: CommandResult,
    requests: Mutex<Vec<CommandRequest>>,
}

impl ProcessRunner for ScriptedRunner {
    fn run<'a>(
        &'a self,
        request: CommandRequest,
        _cancellation: &'a dyn CommandCancellation,
    ) -> ProcessFuture<'a> {
        self.requests
            .lock()
            .expect("request lock must work")
            .push(request);
        Box::pin(async { Ok(self.result.clone()) })
    }
}

fn write_auth_fixture(temp: &TempDir) -> (StorePaths, AuthDocument) {
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&agent).expect("agent directory must be created");
    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string(repository_path("rust/fixtures/store/auth-trust.json"))
            .expect("fixture must be readable"),
    )
    .expect("fixture must parse");
    let auth = serde_json::to_vec_pretty(&fixture["auth"]).expect("auth fixture must serialize");
    std::fs::write(agent.join("auth.json"), &auth).expect("auth file must be written");
    let paths = StorePaths::new(&agent, &cwd, &home).expect("paths must resolve");
    let document = AuthDocument::load(&paths).expect("auth must load");
    assert_eq!(
        std::fs::read(agent.join("auth.json")).expect("auth file must remain"),
        auth
    );
    (paths, document)
}

#[tokio::test]
async fn api_key_precedence_is_cli_auth_environment_then_models() {
    let temp = TempDir::new("auth");
    let (paths, auth) = write_auth_fixture(&temp);
    let runner = ScriptedRunner {
        result: CommandResult {
            stdout: "command-key\n".to_owned(),
            success: true,
        },
        requests: Mutex::new(Vec::new()),
    };
    let environment = BTreeMap::from([
        ("STORED_PROVIDER_KEY".to_owned(), "global-secret".to_owned()),
        ("PROVIDER_KEY".to_owned(), "environment-secret".to_owned()),
    ]);
    let provider_keys = vec!["PROVIDER_KEY".to_owned()];

    let cli = resolve_credential(
        CredentialRequest {
            provider: "stored-provider",
            cli_override: Some("cli-secret"),
            provider_environment_keys: &provider_keys,
            environment: &environment,
            models_json_key: Some("models-secret"),
            cwd: &paths.cwd,
        },
        &auth,
        &runner,
        &NeverCancelled,
    )
    .await
    .expect("CLI credential must resolve");
    assert_eq!(cli.source, CredentialSource::CliOverride);
    assert_eq!(cli.secret.expose_secret(), "cli-secret");

    let stored = resolve_credential(
        CredentialRequest {
            provider: "stored-provider",
            cli_override: None,
            provider_environment_keys: &provider_keys,
            environment: &environment,
            models_json_key: Some("models-secret"),
            cwd: &paths.cwd,
        },
        &auth,
        &runner,
        &NeverCancelled,
    )
    .await
    .expect("stored credential must resolve");
    assert_eq!(stored.source, CredentialSource::AuthJson);
    assert_eq!(stored.secret.expose_secret(), "stored-secret");

    let provider_environment = resolve_credential(
        CredentialRequest {
            provider: "environment-provider",
            cli_override: None,
            provider_environment_keys: &provider_keys,
            environment: &environment,
            models_json_key: Some("models-secret"),
            cwd: &paths.cwd,
        },
        &auth,
        &runner,
        &NeverCancelled,
    )
    .await
    .expect("environment credential must resolve");
    assert_eq!(
        provider_environment.source,
        CredentialSource::ProviderEnvironment
    );

    let models = resolve_credential(
        CredentialRequest {
            provider: "models-provider",
            cli_override: None,
            provider_environment_keys: &[],
            environment: &environment,
            models_json_key: Some("models-secret"),
            cwd: &paths.cwd,
        },
        &auth,
        &runner,
        &NeverCancelled,
    )
    .await
    .expect("models credential must resolve");
    assert_eq!(models.source, CredentialSource::ModelsJson);
    assert_eq!(models.secret.expose_secret(), "models-secret");
}

#[tokio::test]
async fn oauth_is_preserved_but_actionably_unsupported() {
    let temp = TempDir::new("auth-oauth");
    let (paths, auth) = write_auth_fixture(&temp);
    let runner = ScriptedRunner {
        result: CommandResult {
            stdout: String::new(),
            success: true,
        },
        requests: Mutex::new(Vec::new()),
    };
    let error = resolve_credential(
        CredentialRequest {
            provider: "oauth-provider",
            cli_override: None,
            provider_environment_keys: &[],
            environment: &BTreeMap::new(),
            models_json_key: None,
            cwd: &paths.cwd,
        },
        &auth,
        &runner,
        &NeverCancelled,
    )
    .await
    .expect_err("OAuth must not execute");

    assert_eq!(error.category, StoreErrorCategory::UnsupportedOauth);
    assert_eq!(
        auth.raw["oauth-provider"]["futureOAuthField"],
        Value::Bool(true)
    );
    assert!(!format!("{auth:?}").contains("preserve-access"));
}

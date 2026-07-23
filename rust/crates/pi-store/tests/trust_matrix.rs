mod support;

use pi_protocol::DefaultProjectTrust;
use pi_store::{
    ProtectedResource, StorePaths, TrustDecisionSource, TrustDocument, TrustRequest, resolve_trust,
};
use serde_json::json;
use support::{TempDir, repository_path};

#[test]
fn closest_saved_ancestor_wins_and_context_remains_independent() {
    let temp = TempDir::new("trust");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("projects").join("denied").join("child");
    std::fs::create_dir_all(&agent).expect("agent directory must be created");
    std::fs::create_dir_all(&project).expect("project directory must be created");
    let allowed_parent = temp.path().join("projects");
    let denied_parent = allowed_parent.join("denied");
    std::fs::write(
        agent.join("trust.json"),
        serde_json::to_vec_pretty(&json!({
            allowed_parent.to_string_lossy(): true,
            denied_parent.to_string_lossy(): false
        }))
        .expect("trust fixture serializes"),
    )
    .expect("trust fixture writes");
    let paths = StorePaths::new(&agent, &project, &home).expect("paths resolve");
    let document = TrustDocument::load(&paths).expect("trust loads");

    let (decision, access) = resolve_trust(
        TrustRequest {
            cwd: &project,
            explicit: None,
            default: DefaultProjectTrust::Always,
            protected_resources_present: true,
            context_disabled: false,
        },
        &document,
    )
    .expect("trust resolves");

    assert!(!decision.trusted);
    assert_eq!(decision.source, TrustDecisionSource::Saved);
    assert_eq!(
        decision.saved_path.as_deref(),
        Some(denied_parent.as_path())
    );
    assert!(
        access
            .iter()
            .find(|item| item.resource == ProtectedResource::Context)
            .is_some_and(|item| item.loaded)
    );
    assert!(
        access
            .iter()
            .find(|item| item.resource == ProtectedResource::ProjectSettings)
            .is_some_and(|item| !item.loaded)
    );
}

#[test]
fn cli_default_and_headless_ask_decisions_have_explicit_provenance() {
    let temp = TempDir::new("trust-sources");
    let document = TrustDocument {
        raw: json!({}),
        entries: Default::default(),
    };
    let cwd = temp.path();
    let cases = [
        (
            Some(true),
            DefaultProjectTrust::Never,
            true,
            TrustDecisionSource::CliOverride,
        ),
        (
            None,
            DefaultProjectTrust::Always,
            true,
            TrustDecisionSource::DefaultAlways,
        ),
        (
            None,
            DefaultProjectTrust::Never,
            false,
            TrustDecisionSource::DefaultNever,
        ),
        (
            None,
            DefaultProjectTrust::Ask,
            false,
            TrustDecisionSource::HeadlessAsk,
        ),
    ];
    for (explicit, default, trusted, source) in cases {
        let (decision, _) = resolve_trust(
            TrustRequest {
                cwd,
                explicit,
                default,
                protected_resources_present: true,
                context_disabled: false,
            },
            &document,
        )
        .expect("trust resolves");
        assert_eq!(decision.trusted, trusted);
        assert_eq!(decision.source, source);
    }
}

#[test]
fn compatibility_fixture_preserves_saved_trust_values() {
    let temp = TempDir::new("trust-fixture");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    std::fs::create_dir_all(&agent).expect("agent directory must exist");
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repository_path("rust/fixtures/store/auth-trust.json"))
            .expect("fixture must be readable"),
    )
    .expect("fixture must parse");
    std::fs::write(
        agent.join("trust.json"),
        serde_json::to_vec_pretty(&fixture["trust"]).expect("trust fixture serializes"),
    )
    .expect("trust fixture writes");
    let paths = StorePaths::new(&agent, "/fixture/project/child", &home).expect("paths resolve");
    let document = TrustDocument::load(&paths).expect("trust fixture loads");

    assert_eq!(
        document
            .nearest(std::path::Path::new("/fixture/project/child"))
            .expect("nearest decision resolves"),
        Some((std::path::PathBuf::from("/fixture/project"), true))
    );
}

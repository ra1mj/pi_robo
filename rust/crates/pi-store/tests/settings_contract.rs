mod support;

use pi_store::{StorePaths, load_settings};
use serde_json::Value;
use support::{TempDir, repository_path};

#[test]
fn trusted_project_settings_use_typescript_one_level_merge() {
    let temp = TempDir::new("settings");
    let home = temp.path().join("home");
    let agent = home.join(".pi").join("agent");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(cwd.join(".pi")).expect("project config directory must be created");
    std::fs::create_dir_all(&agent).expect("agent directory must be created");
    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string(repository_path("rust/fixtures/store/settings.json"))
            .expect("fixture must be readable"),
    )
    .expect("fixture must be valid JSON");
    let global = serde_json::to_vec_pretty(&fixture["global"]).expect("global must serialize");
    let project = serde_json::to_vec_pretty(&fixture["project"]).expect("project must serialize");
    std::fs::write(agent.join("settings.json"), &global).expect("global settings must be written");
    std::fs::write(cwd.join(".pi/settings.json"), &project)
        .expect("project settings must be written");
    let paths = StorePaths::new(&agent, &cwd, &home).expect("paths must resolve");

    let untrusted = load_settings(&paths, false).expect("global settings must load");
    assert!(!untrusted.project_loaded);
    assert_eq!(
        untrusted.merged.default_model.as_deref(),
        Some("global-model")
    );

    let trusted = load_settings(&paths, true).expect("merged settings must load");
    assert!(trusted.project_loaded);
    assert_eq!(trusted.merged_raw, fixture["expectedTrusted"]);
    assert_eq!(
        trusted
            .merged
            .compaction
            .as_ref()
            .and_then(|settings| settings.reserve_tokens),
        Some(1200)
    );
    assert_eq!(
        trusted
            .merged
            .compaction
            .as_ref()
            .and_then(|settings| settings.keep_recent_tokens),
        Some(400)
    );
    assert_eq!(
        std::fs::read(agent.join("settings.json")).expect("global settings must remain"),
        global
    );
    assert_eq!(
        std::fs::read(cwd.join(".pi/settings.json")).expect("project settings must remain"),
        project
    );
    assert!(!agent.join("settings.json.lock").exists());
}

#[test]
fn malformed_settings_are_diagnosed_without_rewriting_or_creating_files() {
    let temp = TempDir::new("settings-invalid");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&agent).expect("agent directory must be created");
    let invalid = b"{ invalid";
    std::fs::write(agent.join("settings.json"), invalid).expect("invalid fixture must be written");
    let paths = StorePaths::new(&agent, &cwd, &home).expect("paths must resolve");

    let snapshot = load_settings(&paths, false).expect("invalid settings produce diagnostics");

    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(
        std::fs::read(agent.join("settings.json")).expect("fixture must remain"),
        invalid
    );
    assert!(!cwd.join(".pi").exists());
}

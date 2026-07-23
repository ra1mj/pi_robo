mod support;

use pi_store::{StorePaths, expand_tilde, normalize_path};
use std::path::Path;
use support::TempDir;

#[test]
fn paths_are_explicit_normalized_and_read_only() {
    let temp = TempDir::new("paths");
    let home = temp.path().join("home");
    let agent = home.join(".pi").join("agent");
    let cwd = temp.path().join("project").join("nested");
    let paths = StorePaths::new("~/.pi/agent", &cwd, &home).expect("paths must resolve");

    assert_eq!(paths.agent_home, agent);
    assert_eq!(paths.cwd, cwd);
    assert_eq!(
        paths.project_settings_file(),
        cwd.join(".pi").join("settings.json")
    );
    assert_eq!(
        paths.default_session_dir(),
        agent.join("sessions").join(format!(
            "--{}--",
            cwd.to_string_lossy()
                .trim_start_matches('/')
                .replace(['/', '\\', ':'], "-")
        ))
    );
    assert!(
        !agent.exists(),
        "path resolution must not create agent home"
    );
}

#[test]
fn parent_components_cannot_escape_the_filesystem_root() {
    assert_eq!(
        normalize_path(Path::new("/../synthetic"), None).expect("absolute path normalizes"),
        Path::new("/synthetic")
    );
}

#[test]
fn tilde_expansion_only_changes_a_leading_path_component() {
    let home = Path::new("/synthetic/home");
    assert_eq!(expand_tilde(Path::new("~"), home), home);
    assert_eq!(
        expand_tilde(Path::new("~/skills"), home),
        home.join("skills")
    );
    assert_eq!(
        expand_tilde(Path::new("project/~"), home),
        Path::new("project/~")
    );
}

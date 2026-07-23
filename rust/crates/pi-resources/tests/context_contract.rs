mod support;

use pi_resources::{CONTEXT_FILENAMES, ContextFileSource, discover_context};
use pi_store::StorePaths;
use support::TempDir;

#[test]
fn global_and_ancestor_context_load_in_stable_precedence_order() {
    let temp = TempDir::new("context");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("project");
    let nested = project.join("src").join("nested");
    std::fs::create_dir_all(&agent).expect("agent directory must exist");
    std::fs::create_dir_all(&nested).expect("project directory must exist");
    std::fs::write(agent.join("AGENTS.md"), "global").expect("global context writes");
    std::fs::write(project.join("AGENTS.md"), "project agents").expect("context writes");
    std::fs::write(project.join("CLAUDE.md"), "lower precedence").expect("context writes");
    std::fs::write(nested.join("CLAUDE.MD"), "nested").expect("context writes");
    let paths = StorePaths::new(&agent, &nested, &home).expect("paths resolve");

    let snapshot = discover_context(&paths, false).expect("context discovery succeeds");

    assert_eq!(
        CONTEXT_FILENAMES,
        ["AGENTS.md", "AGENTS.MD", "CLAUDE.md", "CLAUDE.MD"]
    );
    assert_eq!(snapshot.files[0].source, ContextFileSource::Global);
    assert_eq!(snapshot.files[0].content, "global");
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == project.join("AGENTS.md") && file.content == "project agents")
    );
    assert!(
        !snapshot
            .files
            .iter()
            .any(|file| file.path == project.join("CLAUDE.md"))
    );
    assert_eq!(
        snapshot.files.last().map(|file| file.content.as_str()),
        Some("nested")
    );
}

#[test]
fn explicit_context_disable_skips_all_files_without_touching_them() {
    let temp = TempDir::new("context-disabled");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&agent).expect("agent directory must exist");
    std::fs::create_dir_all(&project).expect("project directory must exist");
    let bytes = b"must remain";
    std::fs::write(project.join("AGENTS.md"), bytes).expect("context writes");
    let paths = StorePaths::new(&agent, &project, &home).expect("paths resolve");

    let snapshot = discover_context(&paths, true).expect("disabled discovery succeeds");

    assert!(snapshot.disabled);
    assert!(snapshot.files.is_empty());
    assert_eq!(
        std::fs::read(project.join("AGENTS.md")).expect("context remains"),
        bytes
    );
}

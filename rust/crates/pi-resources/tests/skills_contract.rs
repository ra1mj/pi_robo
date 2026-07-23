mod support;

use pi_protocol::Settings;
use pi_resources::{SkillDiagnosticLevel, SkillDiscoveryRequest, SkillSource, discover_skills};
use pi_store::StorePaths;
use support::TempDir;

fn write_skill(path: &std::path::Path, name: &str, description: &str, extra: &str) {
    std::fs::create_dir_all(path.parent().expect("skill has parent"))
        .expect("skill directory must exist");
    std::fs::write(
        path,
        format!("---\nname: {name}\ndescription: {description}\n{extra}---\nBody for {name}\n"),
    )
    .expect("skill must be written");
}

#[test]
fn explicit_project_and_global_collision_precedence_is_deterministic() {
    let temp = TempDir::new("skills-order");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("project");
    let explicit = temp
        .path()
        .join("explicit")
        .join("calendar")
        .join("SKILL.md");
    write_skill(
        &agent.join("skills/calendar/SKILL.md"),
        "calendar",
        "global",
        "",
    );
    write_skill(
        &project.join(".pi/skills/calendar/SKILL.md"),
        "calendar",
        "project",
        "",
    );
    write_skill(&explicit, "calendar", "explicit", "");
    let paths = StorePaths::new(&agent, &project, &home).expect("paths resolve");

    let snapshot = discover_skills(SkillDiscoveryRequest {
        paths: &paths,
        settings: &Settings::default(),
        explicit_paths: &[explicit],
        project_trusted: true,
        include_defaults: true,
    })
    .expect("skills discover");

    assert_eq!(snapshot.skills.len(), 1);
    assert_eq!(snapshot.skills[0].description, "explicit");
    assert_eq!(snapshot.skills[0].source, SkillSource::Explicit);
    assert_eq!(
        snapshot
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("name collision"))
            .count(),
        2
    );
}

#[test]
fn root_skill_ignore_rules_frontmatter_and_prompt_visibility_are_preserved() {
    let temp = TempDir::new("skills-contract");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("project");
    let root = agent.join("skills");
    write_skill(
        &root.join("root-skill/SKILL.md"),
        "root-skill",
        "|",
        "  First line\n  Second line\ndisable-model-invocation: true\n",
    );
    write_skill(
        &root.join("root-skill/nested/SKILL.md"),
        "nested-should-not-load",
        "nested",
        "",
    );
    std::fs::create_dir_all(root.join("ignored")).expect("ignored directory exists");
    std::fs::write(root.join(".ignore"), "ignored/\n").expect("ignore file writes");
    write_skill(&root.join("ignored/SKILL.md"), "ignored", "ignored", "");
    write_skill(
        &root.join("invalid/SKILL.md"),
        "Invalid--Name",
        "still loads",
        "",
    );
    let paths = StorePaths::new(&agent, &project, &home).expect("paths resolve");

    let snapshot = discover_skills(SkillDiscoveryRequest {
        paths: &paths,
        settings: &Settings::default(),
        explicit_paths: &[],
        project_trusted: false,
        include_defaults: true,
    })
    .expect("skills discover");

    let root_skill = snapshot
        .skills
        .iter()
        .find(|skill| skill.name == "root-skill")
        .expect("root skill loads");
    assert_eq!(root_skill.description, "First line\nSecond line");
    assert!(root_skill.disable_model_invocation);
    assert!(
        !snapshot
            .skills
            .iter()
            .any(|skill| skill.name == "nested-should-not-load" || skill.name == "ignored")
    );
    assert!(
        snapshot
            .diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic.level == SkillDiagnosticLevel::Warning
                    && diagnostic.message.contains("invalid characters")
            )
    );
}

#[test]
fn untrusted_project_skills_are_skipped_while_global_skills_load() {
    let temp = TempDir::new("skills-trust");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("project");
    write_skill(
        &agent.join("skills/global/SKILL.md"),
        "global",
        "global",
        "",
    );
    write_skill(
        &project.join(".pi/skills/project/SKILL.md"),
        "project",
        "project",
        "",
    );
    let paths = StorePaths::new(&agent, &project, &home).expect("paths resolve");

    let snapshot = discover_skills(SkillDiscoveryRequest {
        paths: &paths,
        settings: &Settings::default(),
        explicit_paths: &[],
        project_trusted: false,
        include_defaults: true,
    })
    .expect("skills discover");

    assert!(snapshot.skills.iter().any(|skill| skill.name == "global"));
    assert!(!snapshot.skills.iter().any(|skill| skill.name == "project"));
}

#[test]
fn negative_settings_paths_exclude_matching_auto_discovered_skills() {
    let temp = TempDir::new("skills-exclude");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("project");
    write_skill(&agent.join("skills/keep/SKILL.md"), "keep", "keep", "");
    write_skill(&agent.join("skills/skip/SKILL.md"), "skip", "skip", "");
    let paths = StorePaths::new(&agent, &project, &home).expect("paths resolve");
    let settings = Settings {
        skills: Some(vec!["-skills/skip".to_owned()]),
        ..Settings::default()
    };

    let snapshot = discover_skills(SkillDiscoveryRequest {
        paths: &paths,
        settings: &settings,
        explicit_paths: &[],
        project_trusted: false,
        include_defaults: true,
    })
    .expect("skills discover");

    assert!(snapshot.skills.iter().any(|skill| skill.name == "keep"));
    assert!(!snapshot.skills.iter().any(|skill| skill.name == "skip"));
}

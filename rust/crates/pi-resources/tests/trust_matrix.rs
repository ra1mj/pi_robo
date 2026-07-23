mod support;

use pi_protocol::{DefaultProjectTrust, Settings};
use pi_resources::{SkillDiscoveryRequest, discover_context, discover_skills};
use pi_store::{StorePaths, TrustDocument, TrustRequest, resolve_trust};
use support::TempDir;

#[test]
fn denied_and_headless_ask_load_context_but_skip_protected_project_resources() {
    let temp = TempDir::new("resource-trust");
    let home = temp.path().join("home");
    let agent = home.join("agent");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&agent).expect("agent directory exists");
    std::fs::create_dir_all(project.join(".pi/skills/project"))
        .expect("project skill directory exists");
    std::fs::write(project.join("AGENTS.md"), "project context").expect("context writes");
    std::fs::write(
        project.join(".pi/skills/project/SKILL.md"),
        "---\nname: project\ndescription: project\n---\nbody\n",
    )
    .expect("project skill writes");
    let paths = StorePaths::new(&agent, &project, &home).expect("paths resolve");
    let trust = TrustDocument::load(&paths).expect("missing trust file is empty");

    for default in [DefaultProjectTrust::Never, DefaultProjectTrust::Ask] {
        let (decision, _) = resolve_trust(
            TrustRequest {
                cwd: &project,
                explicit: None,
                default,
                protected_resources_present: true,
                context_disabled: false,
            },
            &trust,
        )
        .expect("trust resolves");
        let context = discover_context(&paths, false).expect("context discovers");
        let skills = discover_skills(SkillDiscoveryRequest {
            paths: &paths,
            settings: &Settings::default(),
            explicit_paths: &[],
            project_trusted: decision.trusted,
            include_defaults: true,
        })
        .expect("skills discover");

        assert!(
            context
                .files
                .iter()
                .any(|file| file.content == "project context")
        );
        assert!(!skills.skills.iter().any(|skill| skill.name == "project"));
    }
}

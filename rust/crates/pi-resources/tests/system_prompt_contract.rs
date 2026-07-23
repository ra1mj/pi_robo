use pi_resources::{
    ContextFile, ContextFileSource, Skill, SkillSource, SystemPromptInput, assemble_system_prompt,
    format_skills,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn skill(name: &str, hidden: bool) -> Skill {
    Skill {
        name: name.to_owned(),
        description: "A <special> & useful skill.".to_owned(),
        file_path: PathBuf::from(format!("/skills/{name}/SKILL.md")),
        base_dir: PathBuf::from(format!("/skills/{name}")),
        source: SkillSource::Global,
        disable_model_invocation: hidden,
        body: String::new(),
    }
}

#[test]
fn prompt_inputs_render_tools_guidelines_context_and_visible_skills() {
    let prompt = assemble_system_prompt(&SystemPromptInput {
        selected_tools: Some(vec!["read".to_owned(), "dynamic".to_owned()]),
        tool_snippets: BTreeMap::from([
            ("read".to_owned(), "Read files".to_owned()),
            ("dynamic".to_owned(), "Run dynamic behavior".to_owned()),
        ]),
        prompt_guidelines: vec![
            "Keep paths clear.".to_owned(),
            "  Keep paths clear.  ".to_owned(),
            String::new(),
        ],
        context_files: vec![ContextFile {
            path: PathBuf::from("/project/AGENTS.md"),
            content: "Use <strict> rules.".to_owned(),
            source: ContextFileSource::Project,
        }],
        skills: vec![skill("visible", false), skill("hidden", true)],
        cwd: PathBuf::from("/project"),
        ..SystemPromptInput::default()
    });

    assert!(prompt.contains("- read: Read files"));
    assert!(prompt.contains("- dynamic: Run dynamic behavior"));
    assert_eq!(prompt.matches("- Keep paths clear.").count(), 1);
    assert!(prompt.contains("<project_context>"));
    assert!(prompt.contains("Use <strict> rules."));
    assert!(prompt.contains("<name>visible</name>"));
    assert!(!prompt.contains("<name>hidden</name>"));
    assert!(prompt.contains("Current working directory: /project"));
}

#[test]
fn empty_tools_and_all_hidden_skills_have_compatible_empty_forms() {
    let prompt = assemble_system_prompt(&SystemPromptInput {
        cwd: PathBuf::from("/project"),
        selected_tools: Some(Vec::new()),
        skills: vec![skill("hidden", true)],
        ..SystemPromptInput::default()
    });
    assert!(prompt.contains("Available tools:\n(none)"));
    assert_eq!(format_skills(&[skill("hidden", true)]), "");
}

#[test]
fn skills_are_not_injected_when_read_is_not_selected() {
    let prompt = assemble_system_prompt(&SystemPromptInput {
        cwd: PathBuf::from("/project"),
        selected_tools: Some(vec!["bash".to_owned()]),
        tool_snippets: BTreeMap::from([("bash".to_owned(), "Run commands".to_owned())]),
        skills: vec![skill("visible", false)],
        ..SystemPromptInput::default()
    });

    assert!(!prompt.contains("<available_skills>"));
}

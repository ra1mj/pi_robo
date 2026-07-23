use crate::{ContextFile, Skill};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemPromptInput {
    pub selected_tools: Option<Vec<String>>,
    pub tool_snippets: BTreeMap<String, String>,
    pub prompt_guidelines: Vec<String>,
    pub appended_prompt: Option<String>,
    pub context_files: Vec<ContextFile>,
    pub skills: Vec<Skill>,
    pub documentation_path: Option<PathBuf>,
    pub examples_path: Option<PathBuf>,
    pub cwd: PathBuf,
}

#[must_use]
pub fn assemble_system_prompt(input: &SystemPromptInput) -> String {
    let mut sections = Vec::new();
    let tool_names: Vec<&str> = match &input.selected_tools {
        Some(selected) => selected.iter().map(String::as_str).collect(),
        None => input.tool_snippets.keys().map(String::as_str).collect(),
    };
    let tools: Vec<String> = tool_names
        .into_iter()
        .filter_map(|name| {
            input
                .tool_snippets
                .get(name)
                .map(|snippet| format!("- {name}: {snippet}"))
        })
        .collect();
    sections.push(format!(
        "Available tools:\n{}",
        if tools.is_empty() {
            "(none)".to_owned()
        } else {
            tools.join("\n")
        }
    ));

    let mut seen = BTreeSet::new();
    let guidelines: Vec<String> = input
        .prompt_guidelines
        .iter()
        .map(|guideline| guideline.trim())
        .filter(|guideline| !guideline.is_empty())
        .filter(|guideline| seen.insert((*guideline).to_owned()))
        .map(|guideline| format!("- {guideline}"))
        .collect();
    if !guidelines.is_empty() {
        sections.push(format!("Additional guidelines:\n{}", guidelines.join("\n")));
    }
    if let Some(path) = &input.documentation_path {
        sections.push(format!("Additional docs: {}", path.display()));
    }
    if let Some(path) = &input.examples_path {
        sections.push(format!("Examples: {}", path.display()));
    }
    sections.push(format!(
        "Current working directory: {}",
        input.cwd.display()
    ));

    let context = format_context_files(&input.context_files);
    if !context.is_empty() {
        sections.push(context);
    }
    let has_read = input
        .selected_tools
        .as_ref()
        .is_none_or(|selected| selected.iter().any(|tool| tool == "read"));
    if has_read {
        let skills = format_skills(&input.skills);
        if !skills.is_empty() {
            sections.push(skills);
        }
    }
    if let Some(appended) = input
        .appended_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(appended.to_owned());
    }
    sections.join("\n\n")
}

#[must_use]
pub fn format_context_files(files: &[ContextFile]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut result =
        String::from("<project_context>\n\nProject-specific instructions and guidelines:\n\n");
    for file in files {
        result.push_str("<project_instructions path=\"");
        result.push_str(&file.path.display().to_string());
        result.push_str("\">\n");
        result.push_str(&file.content);
        result.push_str("\n</project_instructions>\n\n");
    }
    result.push_str("</project_context>\n");
    result
}

#[must_use]
pub fn format_skills(skills: &[Skill]) -> String {
    let visible: Vec<&Skill> = skills
        .iter()
        .filter(|skill| !skill.disable_model_invocation)
        .collect();
    if visible.is_empty() {
        return String::new();
    }
    let mut result = String::from(
        "The following skills provide specialized instructions for specific tasks.\nUse the read tool to load a skill's file when the task matches its description.\nWhen a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\n<available_skills>\n",
    );
    for skill in visible {
        result.push_str("  <skill>\n    <name>");
        result.push_str(&escape_xml(&skill.name));
        result.push_str("</name>\n    <description>");
        result.push_str(&escape_xml(&skill.description));
        result.push_str("</description>\n    <location>");
        result.push_str(&escape_xml(&skill.file_path.display().to_string()));
        result.push_str("</location>\n  </skill>\n");
    }
    result.push_str("</available_skills>");
    result
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

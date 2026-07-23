//! Project context, skill discovery, and normalized system-prompt inputs.

mod context;
mod prompt;
mod skills;

pub use context::{
    CONTEXT_FILENAMES, ContextFile, ContextFileSource, ContextSnapshot, discover_context,
};
pub use prompt::{SystemPromptInput, assemble_system_prompt, format_context_files, format_skills};
pub use skills::{
    Skill, SkillDiagnostic, SkillDiagnosticLevel, SkillDiscoveryRequest, SkillSnapshot,
    SkillSource, discover_skills,
};

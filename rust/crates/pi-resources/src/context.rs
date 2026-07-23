use pi_store::{StoreDiagnostic, StoreError, StorePaths, canonicalize_for_match};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const CONTEXT_FILENAMES: [&str; 4] = ["AGENTS.md", "AGENTS.MD", "CLAUDE.md", "CLAUDE.MD"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextFileSource {
    Global,
    Project,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextFile {
    pub path: PathBuf,
    pub content: String,
    pub source: ContextFileSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSnapshot {
    pub files: Vec<ContextFile>,
    pub diagnostics: Vec<StoreDiagnostic>,
    pub disabled: bool,
}

pub fn discover_context(paths: &StorePaths, disabled: bool) -> Result<ContextSnapshot, StoreError> {
    if disabled {
        return Ok(ContextSnapshot {
            files: Vec::new(),
            diagnostics: Vec::new(),
            disabled: true,
        });
    }

    let mut directories = vec![(paths.agent_home.clone(), ContextFileSource::Global)];
    let mut ancestors = ancestors_root_first(&paths.cwd);
    directories.extend(
        ancestors
            .drain(..)
            .map(|path| (path, ContextFileSource::Project)),
    );
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    for (directory, source) in directories {
        let Some(path) = first_context_file(&directory) else {
            continue;
        };
        let canonical = canonicalize_for_match(&path)?;
        if !seen.insert(canonical) {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => files.push(ContextFile {
                path,
                content,
                source,
            }),
            Err(error) => diagnostics.push(StoreDiagnostic::error(&StoreError::io(error, &path))),
        }
    }
    Ok(ContextSnapshot {
        files,
        diagnostics,
        disabled: false,
    })
}

fn first_context_file(directory: &Path) -> Option<PathBuf> {
    CONTEXT_FILENAMES
        .iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
}

fn ancestors_root_first(path: &Path) -> Vec<PathBuf> {
    let mut ancestors: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
    ancestors.reverse();
    ancestors
}

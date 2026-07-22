use pi_agent::ToolError;
use std::path::{Component, Path, PathBuf};

pub fn resolve_path(path: &str, cwd: &Path) -> Result<PathBuf, ToolError> {
    if path.is_empty() {
        return Err(ToolError::invalid_arguments("path must not be empty"));
    }
    if !cwd.is_absolute() {
        return Err(ToolError::execution("authoritative cwd must be absolute"));
    }
    let candidate = Path::new(path);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        cwd.join(candidate)
    };
    Ok(normalize_absolute(&absolute))
}

pub(crate) fn normalize_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let absolute = path.is_absolute();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !absolute {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

pub(crate) async fn mutation_key(path: &Path) -> Result<PathBuf, ToolError> {
    match tokio::fs::canonicalize(path).await {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(normalize_absolute(path)),
        Err(error) => Err(ToolError::execution(format!(
            "failed to resolve {}: {error}",
            path.display()
        ))),
    }
}

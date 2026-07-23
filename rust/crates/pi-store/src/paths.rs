use crate::{StoreError, StoreErrorCategory};
use std::path::{Component, Path, PathBuf};

/// Explicit paths used by milestone-1 data stores.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorePaths {
    pub agent_home: PathBuf,
    pub cwd: PathBuf,
    pub home: PathBuf,
}

impl StorePaths {
    pub fn new(
        agent_home: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        home: impl AsRef<Path>,
    ) -> Result<Self, StoreError> {
        let home = normalize_path(home.as_ref(), None)?;
        let agent_home = normalize_path(&expand_tilde(agent_home.as_ref(), &home), Some(&home))?;
        let cwd = normalize_path(&expand_tilde(cwd.as_ref(), &home), Some(&home))?;
        Ok(Self {
            agent_home,
            cwd,
            home,
        })
    }

    #[must_use]
    pub fn settings_file(&self) -> PathBuf {
        self.agent_home.join("settings.json")
    }

    #[must_use]
    pub fn project_settings_file(&self) -> PathBuf {
        self.cwd.join(".pi").join("settings.json")
    }

    #[must_use]
    pub fn models_file(&self) -> PathBuf {
        self.agent_home.join("models.json")
    }

    #[must_use]
    pub fn auth_file(&self) -> PathBuf {
        self.agent_home.join("auth.json")
    }

    #[must_use]
    pub fn trust_file(&self) -> PathBuf {
        self.agent_home.join("trust.json")
    }

    #[must_use]
    pub fn global_skills_dir(&self) -> PathBuf {
        self.agent_home.join("skills")
    }

    #[must_use]
    pub fn project_skills_dir(&self) -> PathBuf {
        self.cwd.join(".pi").join("skills")
    }

    #[must_use]
    pub fn default_session_dir(&self) -> PathBuf {
        let cwd = self.cwd.to_string_lossy();
        let trimmed = cwd.trim_start_matches(['/', '\\']);
        let encoded: String = trimmed
            .chars()
            .map(|character| {
                if matches!(character, '/' | '\\' | ':') {
                    '-'
                } else {
                    character
                }
            })
            .collect();
        self.agent_home
            .join("sessions")
            .join(format!("--{encoded}--"))
    }

    pub fn resolve_user_path(&self, value: impl AsRef<Path>) -> Result<PathBuf, StoreError> {
        normalize_path(&expand_tilde(value.as_ref(), &self.home), Some(&self.cwd))
    }
}

#[must_use]
pub fn expand_tilde(path: &Path, home: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(suffix) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home.join(suffix);
    }
    path.to_path_buf()
}

pub fn normalize_path(path: &Path, base: Option<&Path>) -> Result<PathBuf, StoreError> {
    if path.as_os_str().is_empty() {
        return Err(StoreError::new(
            StoreErrorCategory::InvalidPath,
            "path must not be empty",
        ));
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let base = match base {
            Some(value) => value.to_path_buf(),
            None => std::env::current_dir().map_err(|error| StoreError::io(error, path))?,
        };
        base.join(path)
    };

    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

pub fn canonicalize_for_match(path: &Path) -> Result<PathBuf, StoreError> {
    match std::fs::canonicalize(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => normalize_path(path, None),
        Err(error) => Err(StoreError::io(error, path)),
    }
}

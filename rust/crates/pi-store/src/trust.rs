use crate::{StoreError, StoreErrorCategory, StorePaths, canonicalize_for_match};
use pi_protocol::DefaultProjectTrust;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct TrustDocument {
    pub raw: Value,
    pub entries: BTreeMap<PathBuf, Option<bool>>,
}

impl TrustDocument {
    pub fn load(paths: &StorePaths) -> Result<Self, StoreError> {
        let path = paths.trust_file();
        let content = match std::fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    raw: Value::Object(serde_json::Map::new()),
                    entries: BTreeMap::new(),
                });
            }
            Err(error) => return Err(StoreError::io(error, &path)),
        };
        let raw: Value =
            serde_json::from_str(&content).map_err(|error| StoreError::json(error, &path))?;
        let object = raw.as_object().ok_or_else(|| {
            StoreError::new(
                StoreErrorCategory::InvalidShape,
                "trust.json must be a JSON object",
            )
            .with_path(&path)
        })?;
        let mut entries = BTreeMap::new();
        for (entry_path, decision) in object {
            let decision = match decision {
                Value::Bool(value) => Some(*value),
                Value::Null => None,
                _ => {
                    return Err(StoreError::new(
                        StoreErrorCategory::InvalidShape,
                        format!("trust decision for {entry_path:?} must be true, false, or null"),
                    )
                    .with_path(&path));
                }
            };
            entries.insert(canonicalize_for_match(Path::new(entry_path))?, decision);
        }
        Ok(Self { raw, entries })
    }

    pub fn nearest(&self, cwd: &Path) -> Result<Option<(PathBuf, bool)>, StoreError> {
        let mut current = canonicalize_for_match(cwd)?;
        loop {
            if let Some(Some(decision)) = self.entries.get(&current) {
                return Ok(Some((current, *decision)));
            }
            let Some(parent) = current.parent() else {
                return Ok(None);
            };
            if parent == current {
                return Ok(None);
            }
            current = parent.to_path_buf();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustDecisionSource {
    CliOverride,
    Saved,
    DefaultAlways,
    DefaultNever,
    HeadlessAsk,
    NoProtectedResources,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustDecision {
    pub trusted: bool,
    pub source: TrustDecisionSource,
    pub saved_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
pub struct TrustRequest<'a> {
    pub cwd: &'a Path,
    pub explicit: Option<bool>,
    pub default: DefaultProjectTrust,
    pub protected_resources_present: bool,
    pub context_disabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProtectedResource {
    Context,
    ProjectSettings,
    ProjectSkills,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceAccess {
    pub resource: ProtectedResource,
    pub loaded: bool,
    pub reason: String,
}

pub fn resolve_trust(
    request: TrustRequest<'_>,
    document: &TrustDocument,
) -> Result<(TrustDecision, Vec<ResourceAccess>), StoreError> {
    let decision = if let Some(trusted) = request.explicit {
        TrustDecision {
            trusted,
            source: TrustDecisionSource::CliOverride,
            saved_path: None,
        }
    } else if !request.protected_resources_present {
        TrustDecision {
            trusted: true,
            source: TrustDecisionSource::NoProtectedResources,
            saved_path: None,
        }
    } else if let Some((path, trusted)) = document.nearest(request.cwd)? {
        TrustDecision {
            trusted,
            source: TrustDecisionSource::Saved,
            saved_path: Some(path),
        }
    } else {
        match request.default {
            DefaultProjectTrust::Always => TrustDecision {
                trusted: true,
                source: TrustDecisionSource::DefaultAlways,
                saved_path: None,
            },
            DefaultProjectTrust::Never => TrustDecision {
                trusted: false,
                source: TrustDecisionSource::DefaultNever,
                saved_path: None,
            },
            DefaultProjectTrust::Ask => TrustDecision {
                trusted: false,
                source: TrustDecisionSource::HeadlessAsk,
                saved_path: None,
            },
        }
    };

    let accesses = vec![
        ResourceAccess {
            resource: ProtectedResource::Context,
            loaded: !request.context_disabled,
            reason: if request.context_disabled {
                "context loading was explicitly disabled".to_owned()
            } else {
                "context files are independent of project trust".to_owned()
            },
        },
        ResourceAccess {
            resource: ProtectedResource::ProjectSettings,
            loaded: decision.trusted,
            reason: protected_reason(&decision),
        },
        ResourceAccess {
            resource: ProtectedResource::ProjectSkills,
            loaded: decision.trusted,
            reason: protected_reason(&decision),
        },
    ];
    Ok((decision, accesses))
}

fn protected_reason(decision: &TrustDecision) -> String {
    if decision.trusted {
        format!(
            "protected project resource approved by {:?}",
            decision.source
        )
    } else {
        format!(
            "protected project resource skipped by {:?}",
            decision.source
        )
    }
}

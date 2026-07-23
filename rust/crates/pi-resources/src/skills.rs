use pi_protocol::Settings;
use pi_store::{StoreError, StorePaths, canonicalize_for_match};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MAX_NAME_LENGTH: usize = 64;
const MAX_DESCRIPTION_LENGTH: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillSource {
    Explicit,
    Settings,
    Project,
    Global,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,
    pub source: SkillSource,
    pub disable_model_invocation: bool,
    pub body: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillDiagnosticLevel {
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillDiagnostic {
    pub level: SkillDiagnosticLevel,
    pub message: String,
    pub path: Option<PathBuf>,
    pub line: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct SkillDiscoveryRequest<'a> {
    pub paths: &'a StorePaths,
    pub settings: &'a Settings,
    pub explicit_paths: &'a [PathBuf],
    pub project_trusted: bool,
    pub include_defaults: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillSnapshot {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<SkillDiagnostic>,
}

pub fn discover_skills(request: SkillDiscoveryRequest<'_>) -> Result<SkillSnapshot, StoreError> {
    let mut roots = Vec::new();
    roots.extend(
        request
            .explicit_paths
            .iter()
            .cloned()
            .map(|path| (path, SkillSource::Explicit)),
    );

    let mut excluded = BTreeSet::new();
    if let Some(settings_paths) = &request.settings.skills {
        for configured in settings_paths {
            if let Some(exclusion) = configured.strip_prefix('-') {
                let path = request.paths.agent_home.join(exclusion);
                excluded.insert(canonicalize_for_match(&path)?);
            } else {
                roots.push((
                    request.paths.resolve_user_path(configured)?,
                    SkillSource::Settings,
                ));
            }
        }
    }

    if request.include_defaults {
        if request.project_trusted {
            roots.push((request.paths.project_skills_dir(), SkillSource::Project));
            let global_agents_skills =
                canonicalize_for_match(&request.paths.home.join(".agents").join("skills"))?;
            let mut ancestors: Vec<PathBuf> = request
                .paths
                .cwd
                .ancestors()
                .map(Path::to_path_buf)
                .collect();
            ancestors.reverse();
            for ancestor in ancestors {
                let root = ancestor.join(".agents").join("skills");
                if canonicalize_for_match(&root)? != global_agents_skills {
                    roots.push((root, SkillSource::Project));
                }
            }
        }
        roots.push((request.paths.global_skills_dir(), SkillSource::Global));
        roots.push((
            request.paths.home.join(".agents").join("skills"),
            SkillSource::Global,
        ));
    }

    let mut diagnostics = Vec::new();
    let mut discovered = Vec::new();
    let mut seen_files = BTreeSet::new();
    for (root, source) in roots {
        let canonical_root = canonicalize_for_match(&root)?;
        if excluded
            .iter()
            .any(|excluded| canonical_root.starts_with(excluded))
        {
            continue;
        }
        if !root.exists() {
            if source == SkillSource::Explicit {
                diagnostics.push(SkillDiagnostic {
                    level: SkillDiagnosticLevel::Warning,
                    message: "skill path does not exist".to_owned(),
                    path: Some(root),
                    line: None,
                });
            }
            continue;
        }
        let candidates = collect_skill_files(&root, &mut diagnostics)?;
        for candidate in candidates {
            let canonical = canonicalize_for_match(&candidate)?;
            if excluded
                .iter()
                .any(|excluded| canonical.starts_with(excluded))
            {
                continue;
            }
            if !seen_files.insert(canonical) {
                continue;
            }
            if let Some(skill) = load_skill(&candidate, source, &mut diagnostics) {
                discovered.push(skill);
            }
        }
    }

    let mut by_name = BTreeMap::new();
    let mut skills = Vec::new();
    for skill in discovered {
        if let Some(existing) = by_name.get(&skill.name) {
            diagnostics.push(SkillDiagnostic {
                level: SkillDiagnosticLevel::Warning,
                message: format!(
                    "skill name collision: {:?} already loaded from {}",
                    skill.name, existing
                ),
                path: Some(skill.file_path),
                line: None,
            });
            continue;
        }
        by_name.insert(skill.name.clone(), skill.file_path.display().to_string());
        skills.push(skill);
    }
    Ok(SkillSnapshot {
        skills,
        diagnostics,
    })
}

fn collect_skill_files(
    root: &Path,
    diagnostics: &mut Vec<SkillDiagnostic>,
) -> Result<Vec<PathBuf>, StoreError> {
    if root.is_file() {
        return Ok(
            (root.extension().and_then(|value| value.to_str()) == Some("md"))
                .then(|| root.to_path_buf())
                .into_iter()
                .collect(),
        );
    }
    collect_directory(root, &[], diagnostics)
}

fn collect_directory(
    directory: &Path,
    inherited_rules: &[IgnoreRule],
    diagnostics: &mut Vec<SkillDiagnostic>,
) -> Result<Vec<PathBuf>, StoreError> {
    let root_skill = directory.join("SKILL.md");
    if root_skill.is_file() {
        return Ok(vec![root_skill]);
    }

    let mut rules = inherited_rules.to_vec();
    for name in [".gitignore", ".ignore", ".fdignore"] {
        let path = directory.join(name);
        if !path.is_file() {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => rules.extend(parse_ignore_rules(directory, &content)),
            Err(error) => diagnostics.push(SkillDiagnostic {
                level: SkillDiagnosticLevel::Warning,
                message: format!("could not read ignore file: {error}"),
                path: Some(path),
                line: None,
            }),
        }
    }

    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => return Err(StoreError::io(error, directory)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io(error, directory))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || is_ignored(&path, &rules) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| StoreError::io(error, &path))?;
        if file_type.is_dir() {
            paths.extend(collect_directory(&path, &rules, diagnostics)?);
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("md")
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

#[derive(Clone, Debug)]
struct IgnoreRule {
    base: PathBuf,
    pattern: String,
    negated: bool,
}

fn parse_ignore_rules(base: &Path, content: &str) -> Vec<IgnoreRule> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (negated, pattern) = line
                .strip_prefix('!')
                .map_or((false, line), |pattern| (true, pattern));
            Some(IgnoreRule {
                base: base.to_path_buf(),
                pattern: pattern.trim_start_matches('/').to_owned(),
                negated,
            })
        })
        .collect()
}

fn is_ignored(path: &Path, rules: &[IgnoreRule]) -> bool {
    let mut ignored = false;
    for rule in rules {
        let Ok(relative) = path.strip_prefix(&rule.base) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let pattern = rule.pattern.trim_end_matches('/');
        if glob_matches(pattern, &relative)
            || (!pattern.contains('/')
                && relative.split('/').any(|part| glob_matches(pattern, part)))
        {
            ignored = !rule.negated;
        }
    }
    ignored
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut table = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    table[0][0] = true;
    for pattern_index in 1..=pattern.len() {
        if pattern[pattern_index - 1] == '*' {
            table[pattern_index][0] = table[pattern_index - 1][0];
        }
        for value_index in 1..=value.len() {
            table[pattern_index][value_index] = match pattern[pattern_index - 1] {
                '*' => {
                    table[pattern_index - 1][value_index] || table[pattern_index][value_index - 1]
                }
                '?' => table[pattern_index - 1][value_index - 1],
                character => {
                    character == value[value_index - 1] && table[pattern_index - 1][value_index - 1]
                }
            };
        }
    }
    table[pattern.len()][value.len()]
}

fn load_skill(
    path: &Path,
    source: SkillSource,
    diagnostics: &mut Vec<SkillDiagnostic>,
) -> Option<Skill> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            diagnostics.push(SkillDiagnostic {
                level: SkillDiagnosticLevel::Error,
                message: format!("could not read skill: {error}"),
                path: Some(path.to_path_buf()),
                line: None,
            });
            return None;
        }
    };
    let frontmatter = match parse_frontmatter(&content) {
        Ok(value) => value,
        Err((line, message)) => {
            diagnostics.push(SkillDiagnostic {
                level: SkillDiagnosticLevel::Error,
                message,
                path: Some(path.to_path_buf()),
                line: Some(line),
            });
            return None;
        }
    };
    let base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let default_name = if path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
        base_dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("skill")
    } else {
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("skill")
    };
    let name = frontmatter
        .fields
        .get("name")
        .filter(|name| !name.is_empty())
        .cloned()
        .unwrap_or_else(|| default_name.to_owned());
    let Some(description) = frontmatter
        .fields
        .get("description")
        .filter(|description| !description.trim().is_empty())
        .cloned()
    else {
        diagnostics.push(SkillDiagnostic {
            level: SkillDiagnosticLevel::Warning,
            message: "skill description is required".to_owned(),
            path: Some(path.to_path_buf()),
            line: None,
        });
        return None;
    };

    validate_name(&name, path, diagnostics);
    if description.chars().count() > MAX_DESCRIPTION_LENGTH {
        diagnostics.push(SkillDiagnostic {
            level: SkillDiagnosticLevel::Warning,
            message: format!("skill description exceeds {MAX_DESCRIPTION_LENGTH} characters"),
            path: Some(path.to_path_buf()),
            line: None,
        });
    }
    let disable_model_invocation = frontmatter
        .fields
        .get("disable-model-invocation")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    Some(Skill {
        name,
        description,
        file_path: path.to_path_buf(),
        base_dir,
        source,
        disable_model_invocation,
        body: frontmatter.body,
    })
}

fn validate_name(name: &str, path: &Path, diagnostics: &mut Vec<SkillDiagnostic>) {
    if name.chars().count() > MAX_NAME_LENGTH {
        diagnostics.push(SkillDiagnostic {
            level: SkillDiagnosticLevel::Warning,
            message: format!("skill name exceeds {MAX_NAME_LENGTH} characters"),
            path: Some(path.to_path_buf()),
            line: None,
        });
    }
    if !name.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    }) || name.starts_with('-')
        || name.ends_with('-')
    {
        diagnostics.push(SkillDiagnostic {
            level: SkillDiagnosticLevel::Warning,
            message: "skill name contains invalid characters".to_owned(),
            path: Some(path.to_path_buf()),
            line: None,
        });
    }
    if name.contains("--") {
        diagnostics.push(SkillDiagnostic {
            level: SkillDiagnosticLevel::Warning,
            message: "skill name contains consecutive hyphens".to_owned(),
            path: Some(path.to_path_buf()),
            line: None,
        });
    }
}

#[derive(Debug)]
struct ParsedFrontmatter {
    fields: BTreeMap<String, String>,
    body: String,
}

fn parse_frontmatter(content: &str) -> Result<ParsedFrontmatter, (usize, String)> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.lines();
    if lines.next() != Some("---") {
        return Ok(ParsedFrontmatter {
            fields: BTreeMap::new(),
            body: normalized,
        });
    }
    let all_lines: Vec<&str> = normalized.lines().collect();
    let Some(end) = all_lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (*line == "---").then_some(index))
    else {
        return Ok(ParsedFrontmatter {
            fields: BTreeMap::new(),
            body: normalized,
        });
    };
    let mut fields = BTreeMap::new();
    let mut index = 1;
    while index < end {
        let line = all_lines[index];
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            index += 1;
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            return Err((
                index + 1,
                format!("invalid YAML frontmatter at line {}", index + 1),
            ));
        }
        let Some((key, raw_value)) = line.split_once(':') else {
            return Err((
                index + 1,
                format!("invalid YAML frontmatter at line {}", index + 1),
            ));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err((
                index + 1,
                format!("invalid YAML frontmatter at line {}", index + 1),
            ));
        }
        let raw_value = raw_value.trim();
        if raw_value.is_empty() && !is_skill_frontmatter_key(key) {
            index += 1;
            while index < end && all_lines[index].starts_with(char::is_whitespace) {
                index += 1;
            }
            continue;
        }
        if matches!(raw_value, "|" | ">") {
            let folded = raw_value == ">";
            let mut block = Vec::new();
            index += 1;
            while index < end && all_lines[index].starts_with(char::is_whitespace) {
                block.push(all_lines[index].trim_start());
                index += 1;
            }
            if fields
                .insert(
                    key.to_owned(),
                    if folded {
                        block.join(" ")
                    } else {
                        block.join("\n")
                    },
                )
                .is_some()
            {
                return Err((
                    index + 1,
                    format!("duplicate YAML key at line {}", index + 1),
                ));
            }
            continue;
        }
        if has_unclosed_yaml_delimiter(raw_value) {
            return Err((
                index + 1,
                format!("invalid YAML frontmatter at line {}", index + 1),
            ));
        }
        let parsed_value = if key == "disable-model-invocation"
            && ((raw_value.starts_with('"') && raw_value.ends_with('"'))
                || (raw_value.starts_with('\'') && raw_value.ends_with('\'')))
        {
            raw_value.to_owned()
        } else {
            unquote_yaml_scalar(raw_value)
        };
        if fields.insert(key.to_owned(), parsed_value).is_some() {
            return Err((
                index + 1,
                format!("duplicate YAML key at line {}", index + 1),
            ));
        }
        index += 1;
    }
    Ok(ParsedFrontmatter {
        fields,
        body: all_lines[end + 1..].join("\n").trim().to_owned(),
    })
}

fn is_skill_frontmatter_key(key: &str) -> bool {
    matches!(key, "name" | "description" | "disable-model-invocation")
}

fn has_unclosed_yaml_delimiter(value: &str) -> bool {
    (value.starts_with('[') && !value.ends_with(']'))
        || (value.starts_with('{') && !value.ends_with('}'))
        || (value.starts_with('"') && !value.ends_with('"'))
        || (value.starts_with('\'') && !value.ends_with('\''))
}

fn unquote_yaml_scalar(value: &str) -> String {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return value[1..value.len() - 1].to_owned();
    }
    value.to_owned()
}

use crate::{MutationCoordinator, resolve_path};
use pi_agent::{Cancellation, Tool, ToolError, ToolFuture, ToolOutput, ToolUpdateSink};
use pi_protocol::{Extensions, ToolCallBlock, ToolDefinition};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug)]
pub struct EditTool {
    cwd: PathBuf,
    mutations: MutationCoordinator,
}

impl EditTool {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>, mutations: MutationCoordinator) -> Self {
        Self {
            cwd: cwd.into(),
            mutations,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EditReplacement {
    old_text: String,
    new_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    edits: Vec<EditReplacement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MatchRange {
    start: usize,
    end: usize,
}

impl Tool for EditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit".to_owned(),
            description: "Apply unique, non-overlapping text replacements to one file.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "oldText": { "type": "string" },
                                "newText": { "type": "string" }
                            },
                            "required": ["oldText", "newText"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["path", "edits"],
                "additionalProperties": false
            }),
            extensions: Extensions::new(),
        }
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCallBlock,
        cancellation: &'a dyn Cancellation,
        _updates: &'a dyn ToolUpdateSink,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let args = parse_args(call.arguments.clone())?;
            if args.edits.is_empty() {
                return Err(ToolError::invalid_arguments(
                    "Edit tool input is invalid. edits must contain at least one replacement.",
                ));
            }
            let path = resolve_path(&args.path, &self.cwd)?;
            let _lease = self.mutations.acquire(&path, cancellation).await?;
            check_cancelled(cancellation)?;
            tokio::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .await
                .map_err(|error| {
                    ToolError::execution(format!(
                        "Could not edit file: {}. Error code: {:?}.",
                        args.path,
                        error.raw_os_error()
                    ))
                })?;
            check_cancelled(cancellation)?;
            let bytes = tokio::fs::read(&path).await.map_err(|error| {
                ToolError::execution(format!("failed to read {}: {error}", path.display()))
            })?;
            check_cancelled(cancellation)?;
            let raw = String::from_utf8(bytes).map_err(|error| {
                ToolError::execution(format!("{} is not valid UTF-8: {error}", path.display()))
            })?;
            let (bom, without_bom) = if let Some(content) = raw.strip_prefix('\u{feff}') {
                ("\u{feff}", content)
            } else {
                ("", raw.as_str())
            };
            let ending = if without_bom.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let base = normalize_line_endings(without_bom);
            let edited = apply_edits(&base, &args.edits, &args.path)?;
            if edited == base {
                return Err(ToolError::invalid_arguments("edit produced no changes"));
            }
            check_cancelled(cancellation)?;
            let restored = if ending == "\r\n" {
                edited.replace('\n', "\r\n")
            } else {
                edited.clone()
            };
            let final_content = format!("{bom}{restored}");
            tokio::fs::write(&path, final_content.as_bytes())
                .await
                .map_err(|error| {
                    ToolError::execution(format!("failed to write {}: {error}", path.display()))
                })?;
            check_cancelled(cancellation)?;
            let (diff, patch, first_changed_line) = build_diffs(&args.path, &base, &edited);
            Ok(ToolOutput {
                content: vec![pi_protocol::ContentBlock::Text(
                    pi_protocol::TextBlock::new(format!(
                        "Successfully replaced {} block(s) in {}.",
                        args.edits.len(),
                        args.path
                    )),
                )],
                details: Some(json!({
                    "diff": diff,
                    "patch": patch,
                    "firstChangedLine": first_changed_line
                })),
            })
        })
    }
}

fn parse_args(mut value: Value) -> Result<EditArgs, ToolError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| ToolError::invalid_arguments("edit arguments must be an object"))?;
    if let Some(Value::String(serialized)) = object.get("edits") {
        let parsed = serde_json::from_str::<Value>(serialized).map_err(|error| {
            ToolError::invalid_arguments(format!("invalid stringified edits: {error}"))
        })?;
        if !parsed.is_array() {
            return Err(ToolError::invalid_arguments(
                "stringified edits must decode to an array",
            ));
        }
        object.insert("edits".to_owned(), parsed);
    }
    let legacy_old = object.remove("oldText");
    let legacy_new = object.remove("newText");
    match (legacy_old, legacy_new) {
        (Some(old_text), Some(new_text)) => {
            let edits = object
                .entry("edits".to_owned())
                .or_insert_with(|| Value::Array(Vec::new()));
            let array = edits
                .as_array_mut()
                .ok_or_else(|| ToolError::invalid_arguments("edits must be an array"))?;
            array.push(json!({ "oldText": old_text, "newText": new_text }));
        }
        (None, None) => {}
        _ => {
            return Err(ToolError::invalid_arguments(
                "legacy edit input requires both oldText and newText",
            ));
        }
    }
    serde_json::from_value(value)
        .map_err(|error| ToolError::invalid_arguments(format!("invalid edit arguments: {error}")))
}

fn apply_edits(original: &str, edits: &[EditReplacement], path: &str) -> Result<String, ToolError> {
    let normalized = NormalizedText::new(original);
    let mut ranges = Vec::with_capacity(edits.len());
    for edit in edits {
        if edit.old_text.is_empty() {
            return Err(ToolError::invalid_arguments(
                "edits[].oldText must not be empty",
            ));
        }
        if edit.old_text == edit.new_text {
            return Err(ToolError::invalid_arguments(
                "edits[].oldText and newText must differ",
            ));
        }
        let old = normalize_line_endings(&edit.old_text);
        let exact = occurrences(original, &old);
        let range = if exact.len() == 1 {
            MatchRange {
                start: exact[0].0,
                end: exact[0].1,
            }
        } else if exact.len() > 1 {
            return Err(ToolError::execution(format!(
                "oldText matched multiple locations in {path}; include more context"
            )));
        } else {
            let needle = normalized_value(&old);
            let matches = occurrences(&normalized.text, &needle);
            if matches.is_empty() {
                return Err(ToolError::execution(format!(
                    "oldText was not found in {path}"
                )));
            }
            if matches.len() > 1 {
                return Err(ToolError::execution(format!(
                    "oldText matched multiple normalized locations in {path}; include more context"
                )));
            }
            normalized
                .original_range(matches[0].0, matches[0].1)
                .ok_or_else(|| {
                    ToolError::execution("normalized edit match could not be mapped to source text")
                })?
        };
        ranges.push((range, normalize_line_endings(&edit.new_text)));
    }
    let mut ordered = ranges
        .iter()
        .enumerate()
        .map(|(index, (range, _))| (index, *range))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(_, range)| range.start);
    for pair in ordered.windows(2) {
        if pair[0].1.end > pair[1].1.start {
            return Err(ToolError::invalid_arguments(
                "edits contain overlapping or duplicate matches",
            ));
        }
    }

    let mut result = original.to_owned();
    let mut descending = ranges.into_iter().collect::<Vec<_>>();
    descending.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, replacement) in descending {
        result.replace_range(range.start..range.end, &replacement);
    }
    Ok(result)
}

fn occurrences(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    haystack
        .char_indices()
        .filter_map(|(start, _)| {
            haystack[start..]
                .starts_with(needle)
                .then_some((start, start + needle.len()))
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct NormalizedToken {
    normalized_start: usize,
    normalized_end: usize,
    original_start: usize,
    original_end: usize,
}

#[derive(Debug)]
struct NormalizedText {
    text: String,
    tokens: Vec<NormalizedToken>,
}

impl NormalizedText {
    fn new(value: &str) -> Self {
        let mut characters = Vec::<(char, usize, usize)>::new();
        let mut iterator = value.char_indices().peekable();
        while let Some((start, character)) = iterator.next() {
            let mut end = iterator.peek().map_or(value.len(), |(index, _)| *index);
            if character == '\r' {
                if let Some((next_start, '\n')) = iterator.peek().copied() {
                    iterator.next();
                    end = next_start + 1;
                }
                trim_trailing_horizontal_space(&mut characters);
                characters.push(('\n', start, end));
                continue;
            }
            if character == '\n' {
                trim_trailing_horizontal_space(&mut characters);
                characters.push(('\n', start, end));
                continue;
            }
            for normalized in character.to_string().nfkc() {
                characters.push((normalize_punctuation(normalized), start, end));
            }
        }
        trim_trailing_horizontal_space(&mut characters);
        let mut text = String::new();
        let mut tokens = Vec::with_capacity(characters.len());
        for (character, original_start, original_end) in characters {
            let normalized_start = text.len();
            text.push(character);
            tokens.push(NormalizedToken {
                normalized_start,
                normalized_end: text.len(),
                original_start,
                original_end,
            });
        }
        Self { text, tokens }
    }

    fn original_range(&self, start: usize, end: usize) -> Option<MatchRange> {
        let first = self
            .tokens
            .iter()
            .find(|token| token.normalized_start == start)?;
        let last = self
            .tokens
            .iter()
            .rev()
            .find(|token| token.normalized_end == end)?;
        Some(MatchRange {
            start: first.original_start,
            end: last.original_end,
        })
    }
}

fn normalized_value(value: &str) -> String {
    NormalizedText::new(value).text
}

fn trim_trailing_horizontal_space(characters: &mut Vec<(char, usize, usize)>) {
    while characters
        .last()
        .is_some_and(|(character, _, _)| matches!(character, ' ' | '\t'))
    {
        characters.pop();
    }
}

fn normalize_punctuation(character: char) -> char {
    match character {
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
        '\u{00a0}' | '\u{2007}' | '\u{202f}' => ' ',
        other => other,
    }
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn check_cancelled(cancellation: &dyn Cancellation) -> Result<(), ToolError> {
    if cancellation.is_cancelled() {
        Err(ToolError::cancelled())
    } else {
        Ok(())
    }
}

fn build_diffs(path: &str, old: &str, new: &str) -> (String, String, usize) {
    let old_lines = old.split('\n').collect::<Vec<_>>();
    let new_lines = new.split('\n').collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut old_suffix = old_lines.len();
    let mut new_suffix = new_lines.len();
    while old_suffix > prefix
        && new_suffix > prefix
        && old_lines[old_suffix - 1] == new_lines[new_suffix - 1]
    {
        old_suffix -= 1;
        new_suffix -= 1;
    }
    let context_start = prefix.saturating_sub(3);
    let old_context_end = (old_suffix + 3).min(old_lines.len());
    let new_context_end = (new_suffix + 3).min(new_lines.len());
    let old_count = old_context_end - context_start;
    let new_count = new_context_end - context_start;
    let mut body = Vec::new();
    body.extend(
        old_lines[context_start..prefix]
            .iter()
            .map(|line| format!(" {line}")),
    );
    body.extend(
        old_lines[prefix..old_suffix]
            .iter()
            .map(|line| format!("-{line}")),
    );
    body.extend(
        new_lines[prefix..new_suffix]
            .iter()
            .map(|line| format!("+{line}")),
    );
    body.extend(
        new_lines[new_suffix..new_context_end]
            .iter()
            .map(|line| format!(" {line}")),
    );
    let header = format!(
        "@@ -{},{} +{},{} @@",
        context_start + 1,
        old_count,
        context_start + 1,
        new_count
    );
    let diff = std::iter::once(header.clone())
        .chain(body.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");
    let patch = std::iter::once(format!("--- a/{path}"))
        .chain(std::iter::once(format!("+++ b/{path}")))
        .chain(std::iter::once(header))
        .chain(body)
        .collect::<Vec<_>>()
        .join("\n");
    (diff, patch, prefix + 1)
}

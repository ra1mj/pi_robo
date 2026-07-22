pub const DEFAULT_MAX_LINES: usize = 2_000;
pub const DEFAULT_MAX_BYTES: usize = 50 * 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TruncatedBy {
    Lines,
    Bytes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TruncationResult {
    pub content: String,
    pub truncated: bool,
    pub truncated_by: Option<TruncatedBy>,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub output_lines: usize,
    pub output_bytes: usize,
    pub last_line_partial: bool,
    pub first_line_exceeds_limit: bool,
    pub max_lines: usize,
    pub max_bytes: usize,
}

pub fn truncate_head(content: &str, max_lines: usize, max_bytes: usize) -> TruncationResult {
    let lines = split_lines_for_counting(content);
    let total_lines = lines.len();
    let total_bytes = content.len();
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return unchanged(content, total_lines, total_bytes, max_lines, max_bytes);
    }
    if lines.first().is_some_and(|line| line.len() > max_bytes) {
        return TruncationResult {
            content: String::new(),
            truncated: true,
            truncated_by: Some(TruncatedBy::Bytes),
            total_lines,
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            last_line_partial: false,
            first_line_exceeds_limit: true,
            max_lines,
            max_bytes,
        };
    }

    let mut output = Vec::new();
    let mut bytes = 0;
    let mut truncated_by = TruncatedBy::Lines;
    for (index, line) in lines.iter().take(max_lines).enumerate() {
        let line_bytes = line.len() + usize::from(index > 0);
        if bytes + line_bytes > max_bytes {
            truncated_by = TruncatedBy::Bytes;
            break;
        }
        output.push(*line);
        bytes += line_bytes;
    }
    if output.len() >= max_lines && bytes <= max_bytes {
        truncated_by = TruncatedBy::Lines;
    }
    truncated(TruncatedOutput {
        content: output.join("\n"),
        total_lines,
        total_bytes,
        output_lines: output.len(),
        last_line_partial: false,
        first_line_exceeds_limit: false,
        truncated_by,
        max_lines,
        max_bytes,
    })
}

pub fn truncate_tail(content: &str, max_lines: usize, max_bytes: usize) -> TruncationResult {
    let lines = split_lines_for_counting(content);
    let total_lines = lines.len();
    let total_bytes = content.len();
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return unchanged(content, total_lines, total_bytes, max_lines, max_bytes);
    }

    let mut output = Vec::new();
    let mut bytes = 0;
    let mut truncated_by = TruncatedBy::Lines;
    let mut partial = false;
    for line in lines.iter().rev().take(max_lines) {
        let line_bytes = line.len() + usize::from(!output.is_empty());
        if bytes + line_bytes > max_bytes {
            truncated_by = TruncatedBy::Bytes;
            if output.is_empty() {
                output.push(utf8_suffix(line, max_bytes));
                partial = true;
            }
            break;
        }
        output.push((*line).to_owned());
        bytes += line_bytes;
    }
    output.reverse();
    if output.len() >= max_lines && bytes <= max_bytes {
        truncated_by = TruncatedBy::Lines;
    }
    let content = output.join("\n");
    let output_lines = output.len();
    truncated(TruncatedOutput {
        content,
        total_lines,
        total_bytes,
        output_lines,
        last_line_partial: partial,
        first_line_exceeds_limit: false,
        truncated_by,
        max_lines,
        max_bytes,
    })
}

fn split_lines_for_counting(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines = content.split('\n').collect::<Vec<_>>();
    if content.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn utf8_suffix(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut start = value.len() - max_bytes;
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    value[start..].to_owned()
}

fn unchanged(
    content: &str,
    total_lines: usize,
    total_bytes: usize,
    max_lines: usize,
    max_bytes: usize,
) -> TruncationResult {
    TruncationResult {
        content: content.to_owned(),
        truncated: false,
        truncated_by: None,
        total_lines,
        total_bytes,
        output_lines: total_lines,
        output_bytes: total_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

struct TruncatedOutput {
    content: String,
    total_lines: usize,
    total_bytes: usize,
    output_lines: usize,
    last_line_partial: bool,
    first_line_exceeds_limit: bool,
    truncated_by: TruncatedBy,
    max_lines: usize,
    max_bytes: usize,
}

fn truncated(output: TruncatedOutput) -> TruncationResult {
    let TruncatedOutput {
        content,
        total_lines,
        total_bytes,
        output_lines,
        last_line_partial,
        first_line_exceeds_limit,
        truncated_by,
        max_lines,
        max_bytes,
    } = output;
    let output_bytes = content.len();
    TruncationResult {
        content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines,
        output_bytes,
        last_line_partial,
        first_line_exceeds_limit,
        max_lines,
        max_bytes,
    }
}

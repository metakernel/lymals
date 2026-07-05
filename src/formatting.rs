use crate::{
    parser::ParsedDocument,
    syntax::{FileId, FormatResult, ParserBackend, SourceSpan, TextEdit},
};

pub fn format_document(parsed: &ParsedDocument) -> FormatResult {
    format_text(
        parsed.source.file_id,
        &parsed.source.name,
        parsed.backend,
        parsed.source.as_str(),
    )
}

pub fn format_text(
    _file_id: FileId,
    _name: &str,
    backend: ParserBackend,
    text: &str,
) -> FormatResult {
    match backend {
        ParserBackend::Fallback => fallback_format_result(text),
        #[cfg(feature = "upstream-luma")]
        ParserBackend::UpstreamLuma => {
            if requires_conservative_preservation(text) {
                fallback_format_result(text)
            } else {
                upstream_format_result(_file_id, _name, text)
            }
        }
        #[cfg(not(feature = "upstream-luma"))]
        ParserBackend::UpstreamLuma => fallback_format_result(text),
    }
}

pub fn fallback_format_result(text: &str) -> FormatResult {
    let original = text;
    let line_ending = preferred_line_ending(text);
    let lines = split_lines(text);
    let formatted = format_lines(&lines);
    let text = join_lines(&formatted, line_ending);

    FormatResult {
        changed: text != apply_line_ending(original, line_ending),
        text,
    }
}

pub fn format_range(text: &str, file_id: FileId, range: SourceSpan) -> Option<TextEdit> {
    if range.file_id != file_id || range.start > range.end || range.end > text.len() {
        return None;
    }

    let line_ending = preferred_line_ending(text);
    let original_lines = split_lines(text);
    let formatted_lines = format_lines(&original_lines);
    let (start_line, end_line) = selected_line_bounds(text, range)?;
    let original = join_line_slice(&original_lines, start_line, end_line, line_ending);
    let formatted = join_line_slice(&formatted_lines, start_line, end_line, line_ending);

    if original == formatted {
        return None;
    }

    let start = original_lines[start_line].start;
    let end = original_lines[end_line].end_with_newline;
    Some(TextEdit {
        range: SourceSpan::new(file_id, start, end),
        text: formatted,
    })
}

#[cfg(feature = "upstream-luma")]
fn upstream_format_result(file_id: FileId, name: &str, text: &str) -> FormatResult {
    let formatted = luma::parser::format_str(luma::parser::FileId(file_id.0), name, text);
    let line_ending = preferred_line_ending(text);
    let formatted_text = apply_line_ending(&formatted.formatted.text, line_ending);

    FormatResult {
        changed: formatted_text != text,
        text: formatted_text,
    }
}

fn preferred_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn apply_line_ending(text: &str, line_ending: &str) -> String {
    if line_ending == "\n" {
        text.replace("\r\n", "\n")
    } else {
        text.replace("\r\n", "\n").replace("\n", "\r\n")
    }
}

fn selected_line_bounds(text: &str, range: SourceSpan) -> Option<(usize, usize)> {
    let lines = split_lines(text);
    if lines.is_empty() {
        return None;
    }

    let start_line = lines
        .iter()
        .position(|line| range.start >= line.start && range.start < line.end_with_newline)
        .unwrap_or(lines.len() - 1);

    let target_end = if range.start == range.end {
        range.end
    } else {
        range.end.saturating_sub(1)
    };
    let end_line = lines
        .iter()
        .position(|line| target_end >= line.start && target_end < line.end_with_newline)
        .unwrap_or(lines.len() - 1);

    Some((start_line.min(end_line), start_line.max(end_line)))
}

fn join_line_slice(
    lines: &[Line],
    start_line: usize,
    end_line: usize,
    line_ending: &str,
) -> String {
    join_lines(&lines[start_line..=end_line], line_ending)
}

fn format_lines(lines: &[Line]) -> Vec<Line> {
    let mut formatted = Vec::with_capacity(lines.len());
    let mut indent_stack = vec![(0usize, 0usize)];
    let mut block_mode = BlockMode::None;

    for line in lines {
        let trimmed_end = line.content.trim_end();
        let trimmed = trimmed_end.trim_start();
        let original_indent = line.leading_indent();

        if block_mode.should_preserve(original_indent, trimmed) {
            formatted.push(line.with_content(line.content.clone()));
            if block_mode.ends_after_line(trimmed) {
                block_mode = BlockMode::None;
            }
            continue;
        } else {
            block_mode = BlockMode::None;
        }

        let content = if trimmed.is_empty() {
            String::new()
        } else if is_document_boundary(trimmed) {
            trimmed.to_owned()
        } else {
            while indent_stack
                .last()
                .is_some_and(|(indent, _)| *indent > original_indent)
            {
                indent_stack.pop();
            }

            let level = match indent_stack.last().copied() {
                Some((indent, level)) if indent == original_indent => level,
                Some((indent, level)) if original_indent > indent => {
                    let next = level + 1;
                    indent_stack.push((original_indent, next));
                    next
                }
                _ => 0,
            };

            let indent = "  ".repeat(level);
            let normalized = normalize_line(trimmed);
            format!("{indent}{normalized}")
        };

        if let Some(next_mode) = block_mode_for(trimmed, original_indent) {
            block_mode = next_mode;
        }

        formatted.push(line.with_content(content));
    }

    formatted
}

fn normalize_line(trimmed: &str) -> String {
    if let Some(rest) = trimmed.strip_prefix('#') {
        let rest = rest.trim_start();
        return if rest.is_empty() {
            "#".to_owned()
        } else {
            format!("# {rest}")
        };
    }

    if let Some((name, rest)) = split_head(trimmed, '@') {
        return if rest.is_empty() {
            name.to_owned()
        } else {
            format!("{name} {rest}")
        };
    }

    if let Some(rest) = trimmed.strip_prefix("let ") {
        return match rest.split_once('=') {
            Some((name, value)) => format!("let {} = {}", name.trim(), value.trim()),
            None => format!("let {}", rest.trim()),
        };
    }

    if trimmed.starts_with('-') {
        let rest = trimmed[1..].trim();
        return if rest.is_empty() {
            "-".to_owned()
        } else {
            format!("- {rest}")
        };
    }

    if should_normalize_mapping(trimmed) {
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim_end();
            let value = value.trim();
            return if value.is_empty() {
                format!("{key}:")
            } else {
                format!("{key}: {value}")
            };
        }
    }

    trimmed.to_owned()
}

fn split_head(trimmed: &str, marker: char) -> Option<(&str, &str)> {
    if !trimmed.starts_with(marker) {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let head = parts.next()?;
    let rest = parts.next().unwrap_or_default().trim();
    Some((head, rest))
}

fn should_normalize_mapping(trimmed: &str) -> bool {
    !trimmed.starts_with('?')
        && !trimmed.starts_with('!')
        && !trimmed.starts_with("...")
        && trimmed.contains(':')
}

fn is_document_boundary(trimmed: &str) -> bool {
    matches!(trimmed, "---" | "...")
}

fn block_mode_for(trimmed: &str, original_indent: usize) -> Option<BlockMode> {
    if let Some((_, value)) = trimmed.split_once(':') {
        let value = value.trim();
        if is_block_scalar_header(value) {
            return Some(BlockMode::IndentedBlock(original_indent));
        }
        if value.starts_with("```lua") {
            return Some(BlockMode::Fence);
        }
    }

    if trimmed.starts_with("```lua") {
        return Some(BlockMode::Fence);
    }

    if (trimmed == "lua{" || trimmed.starts_with("lua{")) && !trimmed.ends_with('}') {
        return Some(BlockMode::BraceBlock);
    }

    None
}

fn is_block_scalar_header(value: &str) -> bool {
    matches!(value.chars().next(), Some('|') | Some('>'))
}

#[cfg(feature = "upstream-luma")]
fn requires_conservative_preservation(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```lua") || trimmed.starts_with("lua{") {
            return true;
        }

        trimmed
            .split_once(':')
            .map(|(_, value)| {
                let value = value.trim();
                is_block_scalar_header(value) || value.starts_with("```lua")
            })
            .unwrap_or(false)
    })
}

fn join_lines(lines: &[Line], line_ending: &str) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&line.content);
        if line.has_newline {
            out.push_str(line_ending);
        }
    }
    out
}

fn split_lines(text: &str) -> Vec<Line> {
    if text.is_empty() {
        return vec![Line::new(0, 0, String::new(), false)];
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    for segment in text.split_inclusive('\n') {
        let has_newline = segment.ends_with('\n');
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let end_with_newline = start + segment.len();
        lines.push(Line::new(
            start,
            end_with_newline,
            body.to_owned(),
            has_newline,
        ));
        start = end_with_newline;
    }

    if !text.ends_with('\n')
        && lines
            .last()
            .is_some_and(|line| line.end_with_newline != text.len())
    {
        lines.push(Line::new(
            start,
            text.len(),
            text[start..].to_owned(),
            false,
        ));
    }

    lines
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    start: usize,
    end_with_newline: usize,
    content: String,
    has_newline: bool,
}

impl Line {
    fn new(start: usize, end_with_newline: usize, content: String, has_newline: bool) -> Self {
        Self {
            start,
            end_with_newline,
            content,
            has_newline,
        }
    }

    fn with_content(&self, content: String) -> Self {
        Self {
            start: self.start,
            end_with_newline: self.end_with_newline,
            content,
            has_newline: self.has_newline,
        }
    }

    fn leading_indent(&self) -> usize {
        self.content
            .chars()
            .take_while(|ch| matches!(ch, ' ' | '\t'))
            .count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockMode {
    None,
    IndentedBlock(usize),
    Fence,
    BraceBlock,
}

impl BlockMode {
    fn should_preserve(self, indent: usize, trimmed: &str) -> bool {
        match self {
            Self::None => false,
            Self::IndentedBlock(base_indent) => trimmed.is_empty() || indent > base_indent,
            Self::Fence | Self::BraceBlock => true,
        }
    }

    fn ends_after_line(self, trimmed: &str) -> bool {
        match self {
            Self::None => true,
            Self::IndentedBlock(_) => false,
            Self::Fence => trimmed == "```",
            Self::BraceBlock => trimmed == "}",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::syntax::FileId;

    use super::{fallback_format_result, format_range};

    #[test]
    fn preserves_crlf_and_block_contents() {
        let input = "script: |\r\n    local x = 1  \r\n    print(x)\r\nvalue:  ok \r\n";
        let formatted = fallback_format_result(input);

        assert_eq!(
            formatted.text,
            "script: |\r\n    local x = 1  \r\n    print(x)\r\nvalue: ok\r\n"
        );
    }

    #[test]
    fn range_formatting_is_line_scoped() {
        let input = "root:\n   child:  one  \n   other: two\n";
        let edit = format_range(
            input,
            FileId(7),
            crate::syntax::SourceSpan::new(FileId(7), 6, 23),
        )
        .expect("expected edit");

        assert_eq!(edit.range.start, 6);
        assert_eq!(edit.text, "  child: one\n");
    }
}

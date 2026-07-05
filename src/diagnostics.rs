use std::collections::HashMap;

use tower_lsp::lsp_types::Url;

use crate::{
    ast::{AstFile, DocumentItem, Mapping, Node},
    parser::ParsedDocument,
    syntax::{ParsedFile, SourcePosition, SourceSpan, SourceText},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticTag {
    Deprecated,
    Unnecessary,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelatedDiagnosticSpan {
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub source: String,
    pub message: String,
    pub primary_span: Option<SourceSpan>,
    pub related_spans: Vec<RelatedDiagnosticSpan>,
    pub tags: Vec<DiagnosticTag>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(
        code: impl Into<String>,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            source: "lumals".to_owned(),
            message: message.into(),
            primary_span: None,
            related_spans: Vec::new(),
            tags: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

#[must_use]
pub fn collect(parsed: &ParsedDocument) -> Vec<Diagnostic> {
    let mut diagnostics = parsed.diagnostics.clone();
    diagnostics.extend(validate_text(&parsed.source));

    #[allow(irrefutable_let_patterns)]
    if let ParsedFile::Fallback(file) = &parsed.file {
        diagnostics.extend(validate_ast(&parsed.source, &file.ast));
    }

    diagnostics.sort_by(|left, right| diagnostic_sort_key(left).cmp(&diagnostic_sort_key(right)));
    diagnostics.dedup();
    diagnostics
}

fn diagnostic_sort_key(diagnostic: &Diagnostic) -> (usize, usize, &str, &str) {
    let span = diagnostic
        .primary_span
        .unwrap_or(SourceSpan::new(Default::default(), 0, 0));
    (
        span.start,
        span.end,
        diagnostic.code.as_str(),
        diagnostic.message.as_str(),
    )
}

fn validate_text(source: &SourceText) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let lines = source_lines(source);

    for (offset, byte) in source.as_str().bytes().enumerate() {
        if byte == b'\0' {
            diagnostics.push(validation_error(
                "L001",
                "NUL bytes are not allowed",
                SourceSpan::new(source.file_id, offset, offset + 1),
            ));
        }
    }

    validate_indentation(source, &lines, &mut diagnostics);

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(diagnostic) = validate_directive_line(source, line) {
            diagnostics.push(diagnostic);
        }

        if let Some(diagnostic) = validate_let_alias(source, line) {
            diagnostics.push(diagnostic);
        }

        if let Some(diagnostic) = validate_unterminated_string(source, line) {
            diagnostics.push(diagnostic);
        }

        if let Some(diagnostic) = validate_unterminated_block(source, &lines, index) {
            diagnostics.push(diagnostic);
        }
    }

    diagnostics
}

fn validate_ast(source: &SourceText, file: &AstFile) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for document in &file.documents {
        for item in &document.items {
            if let DocumentItem::Node(node) = item {
                collect_node_diagnostics(source, node, &mut diagnostics);
            }
        }
    }

    diagnostics
}

fn collect_node_diagnostics(source: &SourceText, node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    match node {
        Node::Mapping(mapping) => {
            validate_duplicate_keys(source, mapping, diagnostics);
            for entry in &mapping.entries {
                if let Some(value) = &entry.value {
                    collect_node_diagnostics(source, value, diagnostics);
                }
            }
        }
        Node::Sequence(sequence) => {
            for item in &sequence.items {
                if let Some(value) = &item.value {
                    collect_node_diagnostics(source, value, diagnostics);
                }
            }
        }
        Node::Tag(tag) => {
            if let Some(value) = &tag.value {
                collect_node_diagnostics(source, value, diagnostics);
            }
        }
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => {}
    }
}

fn validate_duplicate_keys(
    source: &SourceText,
    mapping: &Mapping,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen = HashMap::new();

    for entry in &mapping.entries {
        if let Some(first_span) = seen.insert(entry.key.clone(), entry.key_span) {
            let mut diagnostic = Diagnostic::new(
                "L002",
                DiagnosticSeverity::Warning,
                format!("duplicate mapping key '{}'", entry.key),
            )
            .with_source("lumals.validation");
            diagnostic.primary_span = Some(entry.key_span);
            diagnostic.related_spans.push(RelatedDiagnosticSpan {
                span: first_span,
                message: "first definition here".to_owned(),
            });
            diagnostic.tags.push(DiagnosticTag::Unnecessary);
            diagnostic.notes.push(format!(
                "line {} shadows the earlier '{}' entry",
                source.position(entry.key_span.start).line,
                entry.key
            ));
            diagnostics.push(diagnostic);
        }
    }
}

fn validate_indentation(
    source: &SourceText,
    lines: &[SourceLine<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut stack = vec![0usize];
    let mut previous: Option<(usize, bool)> = None;

    for line in lines {
        let trimmed = line.text.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        for (offset, ch) in line.text.char_indices() {
            if ch == ' ' {
                continue;
            }
            if ch == '\t' {
                diagnostics.push(validation_error(
                    "L003",
                    "tabs in indentation are not allowed",
                    SourceSpan::new(source.file_id, line.start + offset, line.start + offset + 1),
                ));
                continue;
            }
            break;
        }

        if line.indent % 2 != 0 {
            diagnostics.push(validation_error(
                "L004",
                "indentation must use multiples of two spaces",
                SourceSpan::new(
                    source.file_id,
                    line.start,
                    line.start + line.indent.min(line.text.len()),
                ),
            ));
        }

        let current = *stack.last().unwrap_or(&0);
        if line.indent > current {
            let allows_child = previous.is_some_and(|(_, allows_child)| allows_child);
            if !allows_child || line.indent != current + 2 {
                diagnostics.push(validation_error(
                    "L005",
                    "invalid indentation increase",
                    SourceSpan::new(
                        source.file_id,
                        line.start,
                        line.start + line.indent.min(line.text.len()),
                    ),
                ));
            }
            stack.push(line.indent);
        } else if line.indent < current {
            while stack.last().is_some_and(|level| *level > line.indent) {
                stack.pop();
            }
            if stack.last().copied().unwrap_or_default() != line.indent {
                diagnostics.push(validation_error(
                    "L006",
                    "indentation does not match any enclosing block",
                    SourceSpan::new(
                        source.file_id,
                        line.start,
                        line.start + line.indent.min(line.text.len()),
                    ),
                ));
                stack.push(line.indent);
            }
        }

        previous = Some((line.indent, line_allows_child_block(trimmed)));
    }
}

fn validate_directive_line(source: &SourceText, line: &SourceLine<'_>) -> Option<Diagnostic> {
    let trimmed = line.text.trim_start();
    if !trimmed.starts_with('@') {
        return None;
    }

    let name = trimmed.split_whitespace().next().unwrap_or(trimmed);
    let name_span = SourceSpan::new(
        source.file_id,
        line.start + line.leading_ws,
        line.start + line.leading_ws + name.len(),
    );
    let value = trimmed[name.len()..].trim();

    if name == "@" || !valid_directive_name(name) {
        return Some(validation_error("L007", "malformed directive", name_span));
    }

    match name {
        "@luma" | "@profile" | "@schema" | "@use" | "@lua" | "@meta" => None,
        "@import" | "@include" => {
            if value.is_empty() {
                Some(validation_error(
                    "L008",
                    "directive requires a path or file: URI",
                    line.span(source.file_id),
                ))
            } else {
                validate_import_like_value(source, line, name, value)
            }
        }
        _ => Some(validation_error("L009", "unknown directive", name_span)),
    }
}

fn validate_import_like_value(
    source: &SourceText,
    line: &SourceLine<'_>,
    directive_name: &str,
    value: &str,
) -> Option<Diagnostic> {
    let value_offset = line.text.find(value)?;
    let value_span = SourceSpan::new(
        source.file_id,
        line.start + value_offset,
        line.start + value_offset + value.len(),
    );
    let candidate = strip_matching_quotes(value).unwrap_or(value);

    if candidate.starts_with("file:") || candidate.contains("://") {
        let uri = Url::parse(candidate).ok()?;
        if uri.scheme() != "file" {
            return Some(validation_error(
                "L010",
                format!("{directive_name} only allows relative paths or file: URIs"),
                value_span,
            ));
        }

        if uri.path().split('/').any(|segment| segment == "..") {
            return Some(validation_error(
                "L011",
                format!("{directive_name} path must not contain parent traversal"),
                value_span,
            ));
        }

        return None;
    }

    if looks_like_absolute_path(candidate) {
        return Some(validation_error(
            "L012",
            format!("{directive_name} absolute paths are not allowed by default"),
            value_span,
        ));
    }

    if candidate.split(['/', '\\']).any(|segment| segment == "..") {
        return Some(validation_error(
            "L013",
            format!("{directive_name} path must stay within the workspace root"),
            value_span,
        ));
    }

    None
}

fn validate_let_alias(source: &SourceText, line: &SourceLine<'_>) -> Option<Diagnostic> {
    let trimmed = line.text.trim_start();
    if !trimmed.starts_with("let ") {
        return None;
    }

    let rest = &trimmed[4..];
    let (binding, _) = rest.split_once('=')?;
    let binding = binding.trim();
    if !binding.contains(" as ") {
        return None;
    }

    let offset = line.text.find(binding)?;
    Some(validation_error(
        "L014",
        "let aliases are not supported in parse-only mode",
        SourceSpan::new(
            source.file_id,
            line.start + offset,
            line.start + offset + binding.len(),
        ),
    ))
}

fn validate_unterminated_string(source: &SourceText, line: &SourceLine<'_>) -> Option<Diagnostic> {
    let (value, offset) = scalar_candidate(line)?;
    let quote = value.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    if is_closed_string(value, quote) {
        return None;
    }

    Some(validation_error(
        "L015",
        "unterminated string",
        SourceSpan::new(
            source.file_id,
            line.start + offset,
            line.start + offset + value.len(),
        ),
    ))
}

fn validate_unterminated_block(
    source: &SourceText,
    lines: &[SourceLine<'_>],
    index: usize,
) -> Option<Diagnostic> {
    let line = &lines[index];
    let trimmed = line.text.trim();

    if let Some(start) = line.text.find("lua{")
        && !line.text[start + 4..].contains('}')
    {
        return Some(validation_error(
            "L016",
            "unterminated lua block",
            SourceSpan::new(source.file_id, line.start + start, line.start + start + 4),
        ));
    }

    if trimmed.starts_with("```lua")
        && !lines[index + 1..]
            .iter()
            .any(|next| next.text.trim() == "```")
    {
        return Some(validation_error(
            "L017",
            "unterminated fenced lua block",
            line.span(source.file_id),
        ));
    }

    None
}

fn validation_error(
    code: impl Into<String>,
    message: impl Into<String>,
    span: SourceSpan,
) -> Diagnostic {
    let mut diagnostic =
        Diagnostic::new(code, DiagnosticSeverity::Error, message).with_source("lumals.validation");
    diagnostic.primary_span = Some(span);
    diagnostic
}

fn scalar_candidate<'a>(line: &'a SourceLine<'a>) -> Option<(&'a str, usize)> {
    let trimmed = line.text.trim_start();

    if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        return Some((trimmed, line.leading_ws));
    }

    if trimmed.starts_with("let ") {
        let offset = line.text.find('=')? + 1;
        let value = line.text[offset..].trim_start();
        let leading = offset + line.text[offset..].len() - value.len();
        return Some((value, leading));
    }

    if trimmed.starts_with('@') {
        let name = trimmed.split_whitespace().next()?;
        let rest = trimmed[name.len()..].trim_start();
        let offset = line.text.find(rest).unwrap_or(line.leading_ws + name.len());
        return Some((rest, offset));
    }

    if let Some(colon) = line.text.find(':') {
        let value = line.text[colon + 1..].trim_start();
        let leading = colon + 1 + line.text[colon + 1..].len() - value.len();
        return Some((value, leading));
    }

    if let Some(rest) = line.text.trim_start().strip_prefix("-") {
        let value = rest.trim_start();
        let offset = line.text.find(value)?;
        return Some((value, offset));
    }

    None
}

fn is_closed_string(value: &str, quote: char) -> bool {
    let mut escaped = false;
    let mut chars = value.chars();
    let _ = chars.next();

    for ch in chars {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == quote {
            return true;
        }
    }

    false
}

fn valid_directive_name(name: &str) -> bool {
    let mut chars = name.chars();
    if chars.next() != Some('@') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn line_allows_child_block(trimmed: &str) -> bool {
    trimmed.ends_with(':')
        || trimmed.ends_with("|")
        || trimmed.ends_with('>')
        || trimmed.starts_with("?if ")
        || trimmed.starts_with("?elif ")
        || trimmed == "?else"
        || trimmed.starts_with("*for ")
        || trimmed.starts_with("```lua")
}

fn looks_like_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}

fn strip_matching_quotes(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        return value.get(1..value.len() - 1);
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct SourceLine<'a> {
    start: usize,
    indent: usize,
    leading_ws: usize,
    text: &'a str,
}

impl SourceLine<'_> {
    fn span(&self, file_id: crate::syntax::FileId) -> SourceSpan {
        SourceSpan::new(file_id, self.start, self.start + self.text.len())
    }
}

fn source_lines(source: &SourceText) -> Vec<SourceLine<'_>> {
    let mut lines = Vec::new();
    let mut start = 0usize;

    for segment in source.as_str().split_inclusive('\n') {
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        let leading_ws = body
            .char_indices()
            .find_map(|(offset, ch)| (ch != ' ' && ch != '\t').then_some(offset))
            .unwrap_or(body.len());
        let indent = body[..leading_ws]
            .chars()
            .take_while(|ch| *ch == ' ')
            .count();
        lines.push(SourceLine {
            start,
            indent,
            leading_ws,
            text: body,
        });
        start += segment.len();
    }

    if source.as_str().is_empty() || source.as_str().ends_with('\n').not() {
        if lines
            .last()
            .is_none_or(|line| line.start + line.text.len() != source.as_str().len())
        {
            let body = &source.as_str()[start..];
            let leading_ws = body
                .char_indices()
                .find_map(|(offset, ch)| (ch != ' ' && ch != '\t').then_some(offset))
                .unwrap_or(body.len());
            let indent = body[..leading_ws]
                .chars()
                .take_while(|ch| *ch == ' ')
                .count();
            lines.push(SourceLine {
                start,
                indent,
                leading_ws,
                text: body,
            });
        }
    }

    lines
}

pub(crate) fn stable_span(source: &SourceText, span: SourceSpan) -> Option<SourceSpan> {
    if span.file_id != source.file_id || span.start > span.end || span.end > source.as_str().len() {
        return None;
    }
    if !span.is_empty() {
        return Some(span);
    }

    if let Some(end) = next_char_boundary(source.as_str(), span.start)
        && end > span.start
    {
        return Some(SourceSpan::new(span.file_id, span.start, end));
    }
    if let Some(start) = prev_char_boundary(source.as_str(), span.start)
        && start < span.start
    {
        return Some(SourceSpan::new(span.file_id, start, span.start));
    }
    Some(span)
}

#[must_use]
pub fn describe_span(source: &SourceText, span: SourceSpan) -> Option<String> {
    let stable = stable_span(source, span)?;
    let SourcePosition { line, column } = source.position(stable.start);
    Some(format!("{line}:{column}"))
}

fn next_char_boundary(text: &str, offset: usize) -> Option<usize> {
    if offset >= text.len() {
        return None;
    }
    let mut chars = text[offset..].chars();
    let ch = chars.next()?;
    Some(offset + ch.len_utf8())
}

fn prev_char_boundary(text: &str, offset: usize) -> Option<usize> {
    if offset == 0 || offset > text.len() {
        return None;
    }
    text[..offset].char_indices().last().map(|(index, _)| index)
}

trait BoolExt {
    fn not(self) -> bool;
}

impl BoolExt for bool {
    fn not(self) -> bool {
        !self
    }
}

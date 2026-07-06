use tower_lsp::lsp_types::{SelectionRange, SelectionRangeParams, Url};

use crate::{
    ast::AstFile,
    parser,
    position::LineIndex,
    syntax::{FileId, ParsedFile, SourceSpan, Token, TokenKind},
};

#[derive(Debug, Clone)]
pub struct SelectionRangesRequest<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub file_id: FileId,
    pub params: &'a SelectionRangeParams,
}

#[derive(Debug, Clone)]
struct SourceLine<'a> {
    number: u32,
    start: usize,
    end: usize,
    indent: usize,
    text: &'a str,
}

impl SourceLine<'_> {
    fn trimmed(&self) -> &str {
        self.text.trim()
    }

    fn span(&self, file_id: FileId) -> SourceSpan {
        SourceSpan::new(file_id, self.start, self.end)
    }
}

pub fn collect(request: SelectionRangesRequest<'_>) -> Option<Vec<SelectionRange>> {
    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-lyma")]
        ParsedFile::Upstream(_) => return Some(Vec::new()),
    };

    let source = &parsed.source;
    let line_index = LineIndex::new(source.as_str());
    let lines = source_lines(source.as_str());
    let tokens = parsed.tokens.as_deref().unwrap_or(&[]);
    let directive_blocks = directive_block_spans(file);
    let mut ranges = Vec::with_capacity(request.params.positions.len());

    for position in &request.params.positions {
        let offset = line_index
            .position_to_offset(source.as_str(), *position)
            .ok()?;
        let line = line_at(&lines, offset)?;
        let mut spans = Vec::new();

        if let Some(token) = token_at(tokens, offset) {
            spans.push(token.span);
        }

        collect_line_spans(request.file_id, &lines, line, offset, &mut spans);

        if let Some(block) = directive_blocks
            .iter()
            .copied()
            .find(|span| contains(*span, offset))
        {
            spans.push(block);
        }

        if let Some(document_span) = file
            .documents
            .iter()
            .map(|document| document.span)
            .find(|span| contains_inclusive(*span, offset))
        {
            spans.push(document_span);
        }

        spans.push(file.span);
        normalize_spans(&mut spans);
        ranges.push(to_selection_range(&line_index, source.as_str(), &spans)?);
    }

    Some(ranges)
}

fn collect_line_spans(
    file_id: FileId,
    lines: &[SourceLine<'_>],
    line: &SourceLine<'_>,
    offset: usize,
    spans: &mut Vec<SourceSpan>,
) {
    if line.trimmed().starts_with('#') {
        spans.push(line.span(file_id));
        return;
    }

    if let Some((header_index, value_span, pair_span)) = owner_block(lines, line, offset, file_id) {
        if let Some(value_span) = value_span.filter(|span| contains(*span, offset)) {
            spans.push(value_span);
        }
        spans.push(pair_span);
        collect_ancestor_blocks(lines, header_index, pair_span.end, file_id, spans);
        return;
    }

    if let Some((key_span, value_span, pair_span)) = mapping_line_spans(line, offset, file_id) {
        if let Some(key_span) = key_span {
            spans.push(key_span);
        }
        if let Some(value_span) = value_span {
            spans.push(value_span);
        }
        spans.push(pair_span);
        collect_ancestor_blocks(lines, line.number as usize, pair_span.end, file_id, spans);
    }
}

fn collect_ancestor_blocks(
    lines: &[SourceLine<'_>],
    mut child_index: usize,
    target_end: usize,
    file_id: FileId,
    spans: &mut Vec<SourceSpan>,
) {
    let mut child_indent = lines[child_index].indent;

    while let Some((parent_index, parent_span)) =
        find_parent_block(lines, child_index, child_indent, target_end, file_id)
    {
        spans.push(parent_span);
        child_index = parent_index;
        child_indent = lines[parent_index].indent;
    }
}

fn owner_block(
    lines: &[SourceLine<'_>],
    line: &SourceLine<'_>,
    offset: usize,
    file_id: FileId,
) -> Option<(usize, Option<SourceSpan>, SourceSpan)> {
    if is_structural_line(line) {
        return None;
    }

    for index in (0..line.number as usize).rev() {
        let header = &lines[index];
        if header.trimmed().is_empty() || is_document_boundary(header.trimmed()) {
            continue;
        }
        if header.indent >= line.indent {
            continue;
        }

        let Some(pair_span) = pair_block_span(lines, index, file_id) else {
            continue;
        };
        if !contains_inclusive(pair_span, offset) {
            continue;
        }

        let value_span = block_value_span(lines, index, file_id);
        return Some((index, value_span, pair_span));
    }

    None
}

fn find_parent_block(
    lines: &[SourceLine<'_>],
    child_index: usize,
    child_indent: usize,
    target_end: usize,
    file_id: FileId,
) -> Option<(usize, SourceSpan)> {
    for index in (0..child_index).rev() {
        let line = &lines[index];
        if line.trimmed().is_empty()
            || is_document_boundary(line.trimmed())
            || line.indent >= child_indent
        {
            continue;
        }

        let Some(span) = pair_block_span(lines, index, file_id) else {
            continue;
        };
        if span.end >= target_end && span.start < lines[child_index].start {
            return Some((index, span));
        }
    }

    None
}

fn mapping_line_spans(
    line: &SourceLine<'_>,
    offset: usize,
    file_id: FileId,
) -> Option<(Option<SourceSpan>, Option<SourceSpan>, SourceSpan)> {
    let trimmed = line.trimmed();
    if trimmed.starts_with('-') || trimmed.starts_with('#') || !trimmed.contains(':') {
        return None;
    }

    let indent_offset = line.text.len().saturating_sub(line.text.trim_start().len());
    let colon_offset = line.text.find(':')?;
    let key_text = line.text[indent_offset..colon_offset].trim_end();
    let key_start = line.start + indent_offset;
    let key_span = SourceSpan::new(file_id, key_start, key_start + key_text.len());

    let value_text = line.text[colon_offset + 1..].trim_start();
    let value_span = if value_text.is_empty() {
        None
    } else {
        let value_start = line.end - value_text.len();
        Some(SourceSpan::new(file_id, value_start, line.end))
    };

    let pair_span = line.span(file_id);
    Some((
        contains(key_span, offset).then_some(key_span),
        value_span.filter(|span| contains(*span, offset)),
        pair_span,
    ))
}

fn pair_block_span(lines: &[SourceLine<'_>], index: usize, file_id: FileId) -> Option<SourceSpan> {
    let line = &lines[index];
    let trimmed = line.trimmed();
    if trimmed.starts_with('#') || trimmed.starts_with('@') || trimmed.starts_with("let ") {
        return None;
    }

    if trimmed.starts_with('-') {
        return Some(item_block_span(lines, index, file_id));
    }

    if !trimmed.contains(':') {
        return None;
    }

    let end_index = block_end_index(lines, index);
    Some(SourceSpan::new(file_id, line.start, lines[end_index].end))
}

fn block_value_span(lines: &[SourceLine<'_>], index: usize, file_id: FileId) -> Option<SourceSpan> {
    let line = &lines[index];
    let colon_offset = line.text.find(':')?;
    let value_text = line.text[colon_offset + 1..].trim_start();
    let block_end = block_end_index(lines, index);

    if value_text.is_empty() {
        return Some(SourceSpan::new(file_id, line.end, lines[block_end].end));
    }

    let key_indent = line.text.len().saturating_sub(line.text.trim_start().len());
    let value_start = if value_text == "|"
        || value_text == ">"
        || value_text.starts_with('|')
        || value_text.starts_with('>')
    {
        line.start + key_indent
    } else {
        line.end - value_text.len()
    };

    Some(SourceSpan::new(file_id, value_start, lines[block_end].end))
}

fn item_block_span(lines: &[SourceLine<'_>], index: usize, file_id: FileId) -> SourceSpan {
    let end_index = block_end_index(lines, index);
    SourceSpan::new(file_id, lines[index].start, lines[end_index].end)
}

fn block_end_index(lines: &[SourceLine<'_>], index: usize) -> usize {
    let base_indent = lines[index].indent;
    let mut last = index;

    for (next, line) in lines.iter().enumerate().skip(index + 1) {
        let trimmed = line.trimmed();
        if is_document_boundary(trimmed) {
            break;
        }
        if trimmed.is_empty() {
            if last > index {
                last = next;
            }
            continue;
        }
        if line.indent <= base_indent {
            break;
        }
        last = next;
    }

    while last > index && lines[last].trimmed().is_empty() {
        last -= 1;
    }

    last
}

fn directive_block_spans(file: &AstFile) -> Vec<SourceSpan> {
    let mut blocks = Vec::new();

    for document in &file.documents {
        let mut start = None;
        let mut end = None;

        for item in &document.items {
            match item {
                crate::ast::DocumentItem::Directive(directive) => {
                    start.get_or_insert(directive.span.start);
                    end = Some(directive.span.end);
                }
                crate::ast::DocumentItem::Comment(comment) => {
                    start.get_or_insert(comment.span.start);
                    end = Some(comment.span.end);
                }
                crate::ast::DocumentItem::Let(binding) => {
                    start.get_or_insert(binding.span.start);
                    end = Some(binding.span.end);
                }
                crate::ast::DocumentItem::Node(_) => break,
            }
        }

        if let (Some(start), Some(end)) = (start, end) {
            blocks.push(SourceSpan::new(document.span.file_id, start, end));
        }
    }

    blocks
}

fn source_lines(text: &str) -> Vec<SourceLine<'_>> {
    let mut lines = Vec::new();
    let mut start = 0usize;

    for (number, segment) in text.split_inclusive('\n').enumerate() {
        let text = segment.strip_suffix('\n').unwrap_or(segment);
        lines.push(SourceLine {
            number: number as u32,
            start,
            end: start + text.len(),
            indent: text
                .chars()
                .take_while(|ch| *ch == ' ' || *ch == '\t')
                .count(),
            text,
        });
        start += segment.len();
    }

    if text.is_empty() {
        lines.push(SourceLine {
            number: 0,
            start: 0,
            end: 0,
            indent: 0,
            text: "",
        });
    } else if !text.ends_with('\n') {
        let number = text.split('\n').count() as u32 - 1;
        if lines.last().is_none_or(|line| line.number != number) {
            let tail = &text[start..];
            lines.push(SourceLine {
                number,
                start,
                end: text.len(),
                indent: tail
                    .chars()
                    .take_while(|ch| *ch == ' ' || *ch == '\t')
                    .count(),
                text: tail,
            });
        }
    }

    lines
}

fn line_at<'a>(lines: &'a [SourceLine<'_>], offset: usize) -> Option<&'a SourceLine<'a>> {
    lines
        .iter()
        .find(|line| offset >= line.start && offset <= line.end)
}

fn token_at(tokens: &[Token], offset: usize) -> Option<&Token> {
    tokens
        .iter()
        .filter(|token| {
            !matches!(token.kind, TokenKind::LineBreak | TokenKind::EndOfFile)
                && contains(token.span, offset)
        })
        .min_by_key(|token| token.span.len())
}

fn to_selection_range(
    line_index: &LineIndex,
    text: &str,
    spans: &[SourceSpan],
) -> Option<SelectionRange> {
    let mut parent = None;

    for span in spans.iter().rev() {
        parent = Some(SelectionRange {
            range: line_index.span_to_range(text, *span).ok()?,
            parent: parent.map(Box::new),
        });
    }

    parent
}

fn normalize_spans(spans: &mut Vec<SourceSpan>) {
    spans.retain(|span| !span.is_empty());

    let mut normalized = Vec::with_capacity(spans.len());
    for span in spans.iter().copied() {
        if normalized.last().copied() == Some(span) || normalized.contains(&span) {
            continue;
        }
        normalized.push(span);
    }
    *spans = normalized;
}

fn is_structural_line(line: &SourceLine<'_>) -> bool {
    let trimmed = line.trimmed();
    trimmed.starts_with('#')
        || trimmed.starts_with('@')
        || trimmed.starts_with("let ")
        || trimmed.starts_with('-')
        || trimmed.contains(':')
}

fn is_document_boundary(trimmed: &str) -> bool {
    matches!(trimmed, "---" | "...")
}

fn contains(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

fn contains_inclusive(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}

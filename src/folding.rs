use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind, Url};

use crate::{
    ast::{AstFile, DocumentItem},
    parser,
    position::LineIndex,
    syntax::{FileId, ParsedFile, SourceSpan, SourceText},
};

#[derive(Debug, Clone)]
pub struct FoldingRangesRequest<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub file_id: FileId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RangeKind {
    Comment,
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateRange {
    start_line: u32,
    end_line: u32,
    kind: RangeKind,
}

#[derive(Debug, Clone)]
struct SourceLine<'a> {
    number: u32,
    indent: usize,
    text: &'a str,
}

impl SourceLine<'_> {
    fn trimmed(&self) -> &str {
        self.text.trim()
    }

    fn is_blank(&self) -> bool {
        self.trimmed().is_empty()
    }
}

pub fn collect(request: FoldingRangesRequest<'_>) -> Option<Vec<FoldingRange>> {
    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return Some(Vec::new()),
    };

    let source = &parsed.source;
    let line_index = LineIndex::new(source.as_str());
    let lines = source_lines(source);
    let mut candidates = Vec::new();

    collect_document_ranges(file, source, &line_index, &mut candidates);
    collect_prelude_ranges(file, source, &line_index, &mut candidates);
    collect_comment_ranges(&lines, &mut candidates);
    collect_structural_ranges(&lines, &mut candidates);

    candidates.sort_by(|left, right| {
        left.start_line
            .cmp(&right.start_line)
            .then_with(|| right.end_line.cmp(&left.end_line))
            .then_with(|| left.kind.cmp(&right.kind))
    });

    let mut accepted = Vec::new();
    for candidate in candidates {
        if candidate.end_line <= candidate.start_line {
            continue;
        }
        if accepted.iter().any(|existing| *existing == candidate) {
            continue;
        }
        if accepted
            .iter()
            .any(|existing| overlaps_invalidly(*existing, candidate))
        {
            continue;
        }
        accepted.push(candidate);
    }

    Some(accepted.into_iter().map(to_folding_range).collect())
}

fn collect_document_ranges(
    file: &AstFile,
    source: &SourceText,
    line_index: &LineIndex,
    candidates: &mut Vec<CandidateRange>,
) {
    if file.documents.len() < 2 {
        return;
    }

    for document in &file.documents {
        let span = document.separator_span.map_or(document.span, |separator| {
            SourceSpan::new(source.file_id, separator.start, document.span.end)
        });
        push_span_range(candidates, source, line_index, span, RangeKind::Region);
    }
}

fn collect_prelude_ranges(
    file: &AstFile,
    source: &SourceText,
    line_index: &LineIndex,
    candidates: &mut Vec<CandidateRange>,
) {
    for document in &file.documents {
        let mut first = None;
        let mut last = None;

        for item in &document.items {
            match item {
                DocumentItem::Directive(directive) => {
                    first.get_or_insert(directive.span);
                    last = Some(directive.span);
                }
                DocumentItem::Comment(comment) => {
                    first.get_or_insert(comment.span);
                    last = Some(comment.span);
                }
                DocumentItem::Let(binding) => {
                    first.get_or_insert(binding.span);
                    last = Some(binding.span);
                }
                DocumentItem::Node(_) => break,
            }
        }

        if let (Some(first), Some(last)) = (first, last) {
            push_span_range(
                candidates,
                source,
                line_index,
                SourceSpan::new(source.file_id, first.start, last.end),
                RangeKind::Region,
            );
        }
    }
}

fn collect_comment_ranges(lines: &[SourceLine<'_>], candidates: &mut Vec<CandidateRange>) {
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        if !line.trimmed().starts_with('#') {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;
        while index < lines.len() && lines[index].trimmed().starts_with('#') {
            index += 1;
        }

        let end = index - 1;
        push_line_range(
            candidates,
            lines[start].number,
            lines[end].number,
            RangeKind::Comment,
        );
    }
}

fn collect_structural_ranges(lines: &[SourceLine<'_>], candidates: &mut Vec<CandidateRange>) {
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.trimmed();

        if trimmed.is_empty() || is_document_boundary(trimmed) {
            index += 1;
            continue;
        }

        if let Some(end) = lua_fence_end(lines, index) {
            push_line_range(
                candidates,
                line.number,
                lines[end].number,
                RangeKind::Region,
            );
            index += 1;
            continue;
        }

        if let Some(end) = lua_brace_end(lines, index) {
            push_line_range(
                candidates,
                line.number,
                lines[end].number,
                RangeKind::Region,
            );
            index += 1;
            continue;
        }

        if is_sequence_start(lines, index) {
            let end = sequence_end(lines, index);
            push_line_range(
                candidates,
                line.number,
                lines[end].number,
                RangeKind::Region,
            );
            index += 1;
            continue;
        }

        if let Some(end) = indented_block_end(lines, index) {
            push_line_range(
                candidates,
                line.number,
                lines[end].number,
                RangeKind::Region,
            );
        }

        index += 1;
    }
}

fn push_span_range(
    candidates: &mut Vec<CandidateRange>,
    source: &SourceText,
    line_index: &LineIndex,
    span: SourceSpan,
    kind: RangeKind,
) {
    let Ok(start) = line_index.offset_to_position(source.as_str(), span.start) else {
        return;
    };
    let Ok(end) = line_index.offset_to_position(source.as_str(), span.end) else {
        return;
    };
    push_line_range(candidates, start.line, end.line, kind);
}

fn push_line_range(
    candidates: &mut Vec<CandidateRange>,
    start_line: u32,
    end_line: u32,
    kind: RangeKind,
) {
    if end_line > start_line {
        candidates.push(CandidateRange {
            start_line,
            end_line,
            kind,
        });
    }
}

fn overlaps_invalidly(left: CandidateRange, right: CandidateRange) -> bool {
    let overlaps = left.start_line <= right.end_line && right.start_line <= left.end_line;
    overlaps && !contains(left, right) && !contains(right, left)
}

fn contains(outer: CandidateRange, inner: CandidateRange) -> bool {
    outer.start_line <= inner.start_line && outer.end_line >= inner.end_line
}

fn to_folding_range(candidate: CandidateRange) -> FoldingRange {
    FoldingRange {
        start_line: candidate.start_line,
        start_character: None,
        end_line: candidate.end_line,
        end_character: None,
        kind: Some(match candidate.kind {
            RangeKind::Comment => FoldingRangeKind::Comment,
            RangeKind::Region => FoldingRangeKind::Region,
        }),
        collapsed_text: None,
    }
}

fn source_lines(source: &SourceText) -> Vec<SourceLine<'_>> {
    let mut lines = Vec::new();
    let mut start = 0usize;

    for (number, segment) in source.as_str().split_inclusive('\n').enumerate() {
        let text = segment.strip_suffix('\n').unwrap_or(segment);
        let indent = text
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .count();
        lines.push(SourceLine {
            number: number as u32,
            indent,
            text,
        });
        start += segment.len();
    }

    if source.as_str().is_empty() {
        lines.push(SourceLine {
            number: 0,
            indent: 0,
            text: "",
        });
    } else if !source.as_str().ends_with('\n') {
        let expected = source.as_str().split('\n').count() as u32 - 1;
        if lines.last().is_none_or(|line| line.number != expected) {
            let text = &source.as_str()[start..];
            lines.push(SourceLine {
                number: expected,
                indent: text
                    .chars()
                    .take_while(|ch| *ch == ' ' || *ch == '\t')
                    .count(),
                text,
            });
        }
    }

    lines
}

fn is_document_boundary(trimmed: &str) -> bool {
    matches!(trimmed, "---" | "...")
}

fn lua_fence_end(lines: &[SourceLine<'_>], index: usize) -> Option<usize> {
    let trimmed = lines[index].trimmed();
    let starts_fence = trimmed.starts_with("```lua")
        || trimmed
            .split_once(':')
            .is_some_and(|(_, value)| value.trim().starts_with("```lua"));
    if !starts_fence {
        return None;
    }

    (index + 1..lines.len()).find(|next| lines[*next].trimmed() == "```")
}

fn lua_brace_end(lines: &[SourceLine<'_>], index: usize) -> Option<usize> {
    let trimmed = lines[index].trimmed();
    let starts_brace = if let Some((_, value)) = trimmed.split_once(':') {
        let value = value.trim();
        (value == "lua{" || value.starts_with("lua{")) && !value.contains('}')
    } else {
        (trimmed == "lua{" || trimmed.starts_with("lua{")) && !trimmed.contains('}')
    };
    if !starts_brace {
        return None;
    }

    (index + 1..lines.len()).find(|next| lines[*next].text.contains('}'))
}

fn is_sequence_start(lines: &[SourceLine<'_>], index: usize) -> bool {
    let trimmed = lines[index].trimmed();
    if !trimmed.starts_with('-') {
        return false;
    }

    if index > 0 {
        let prev = &lines[index - 1];
        if !prev.is_blank() && prev.indent == lines[index].indent && prev.trimmed().starts_with('-')
        {
            return false;
        }
    }

    index + 1 < lines.len() && sequence_end(lines, index) > index
}

fn sequence_end(lines: &[SourceLine<'_>], start: usize) -> usize {
    let base_indent = lines[start].indent;
    let mut index = start + 1;
    let mut last = start;

    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.trimmed();
        if trimmed.is_empty() {
            last = index;
            index += 1;
            continue;
        }
        if is_document_boundary(trimmed) || line.indent < base_indent {
            break;
        }
        if line.indent == base_indent && !trimmed.starts_with('-') {
            break;
        }
        last = index;
        index += 1;
    }

    while last > start && lines[last].is_blank() {
        last -= 1;
    }
    last
}

fn indented_block_end(lines: &[SourceLine<'_>], index: usize) -> Option<usize> {
    let line = &lines[index];
    let trimmed = line.trimmed();

    let expects_child_block = trimmed.ends_with(':')
        || trimmed.starts_with("?if ")
        || trimmed.starts_with("?elif ")
        || trimmed == "?else"
        || trimmed.starts_with("*for ")
        || trimmed
            .split_once(':')
            .is_some_and(|(_, value)| matches!(value.trim().chars().next(), Some('|') | Some('>')));
    if !expects_child_block {
        return None;
    }

    let child_indent = next_child_indent(lines, index)?;
    let mut last = None;

    for next in index + 1..lines.len() {
        let line = &lines[next];
        let trimmed = line.trimmed();
        if is_document_boundary(trimmed) {
            break;
        }
        if trimmed.is_empty() {
            if last.is_some() {
                last = Some(next);
            }
            continue;
        }
        if line.indent < child_indent {
            break;
        }
        last = Some(next);
    }

    let mut last = last?;
    while last > index && lines[last].is_blank() {
        last -= 1;
    }
    Some(last)
}

fn next_child_indent(lines: &[SourceLine<'_>], index: usize) -> Option<usize> {
    for next in index + 1..lines.len() {
        let line = &lines[next];
        let trimmed = line.trimmed();
        if trimmed.is_empty() {
            continue;
        }
        if is_document_boundary(trimmed) {
            return None;
        }
        return (line.indent > lines[index].indent).then_some(line.indent);
    }
    None
}

use tower_lsp::lsp_types::{
    InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams, Url, WorkspaceFolder,
};

use crate::{
    ast::{AstFile, Directive, DocumentItem, LetBinding, MappingEntry, Node, Scalar, ScalarKind},
    config::LymalsConfig,
    imports::resolve_guarded_import,
    parser,
    position::LineIndex,
    syntax::{FileId, ParsedFile, SourceSpan, SourceText},
};

#[derive(Debug, Clone)]
pub struct InlayHintsRequest<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub file_id: FileId,
    pub params: &'a InlayHintParams,
    pub workspace_folders: &'a [WorkspaceFolder],
    pub config: &'a LymalsConfig,
}

pub fn collect(request: InlayHintsRequest<'_>) -> Option<Vec<InlayHint>> {
    if !request.config.inlay_hints.enabled {
        return None;
    }

    let parsed = parser::parse_fallback(request.file_id, request.uri.as_str(), request.text);
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => &file.ast,
        #[cfg(feature = "upstream-lyma")]
        ParsedFile::Upstream(_) => return Some(Vec::new()),
    };

    let source = &parsed.source;
    let line_index = LineIndex::new(source.as_str());
    let range = request.params.range;
    let start = line_index
        .position_to_offset(source.as_str(), range.start)
        .ok()?;
    let end = line_index
        .position_to_offset(source.as_str(), range.end)
        .ok()?;
    let mut hints = Vec::new();
    collect_file_hints(file, source, &line_index, &request, start, end, &mut hints);
    hints.sort_by(|left, right| {
        left.position
            .line
            .cmp(&right.position.line)
            .then_with(|| left.position.character.cmp(&right.position.character))
            .then_with(|| inlay_label(left).cmp(inlay_label(right)))
    });
    Some(hints)
}

fn collect_file_hints(
    file: &AstFile,
    source: &SourceText,
    line_index: &LineIndex,
    request: &InlayHintsRequest<'_>,
    start: usize,
    end: usize,
    hints: &mut Vec<InlayHint>,
) {
    for document in &file.documents {
        for item in &document.items {
            match item {
                DocumentItem::Directive(directive) => {
                    collect_directive_hints(
                        directive, source, line_index, request, start, end, hints,
                    );
                }
                DocumentItem::Let(binding) => {
                    collect_let_hints(binding, source, line_index, request, start, end, hints);
                }
                DocumentItem::Node(node) => {
                    collect_node_hints(node, source, line_index, request, start, end, hints);
                }
                DocumentItem::Comment(_) => {}
            }
        }
    }
}

fn collect_directive_hints(
    directive: &Directive,
    source: &SourceText,
    line_index: &LineIndex,
    request: &InlayHintsRequest<'_>,
    start: usize,
    end: usize,
    hints: &mut Vec<InlayHint>,
) {
    if request.config.inlay_hints.profile_effects
        && directive.name == "@profile"
        && let Some(value_span) = directive_value_span(directive, source)
        && let Some(value) = source.slice(value_span).map(str::trim)
        && !value.is_empty()
    {
        push_hint(
            hints,
            line_index,
            source.as_str(),
            value_span,
            format!("activates `{value}` profile"),
            InlayHintKind::PARAMETER,
            start,
            end,
        );
    }

    if request.config.inlay_hints.import_resolution
        && matches!(directive.name.as_str(), "@import" | "@include")
        && let Some(value) = directive_target(directive)
        && let Ok(target) = resolve_guarded_import(
            request.uri,
            &value,
            request.workspace_folders,
            request.config,
        )
    {
        let anchor = directive_target_span(directive, source).unwrap_or(directive.span);
        push_hint(
            hints,
            line_index,
            source.as_str(),
            anchor,
            format!("→ {target}"),
            InlayHintKind::PARAMETER,
            start,
            end,
        );
    }
}

fn collect_let_hints(
    binding: &LetBinding,
    source: &SourceText,
    line_index: &LineIndex,
    request: &InlayHintsRequest<'_>,
    start: usize,
    end: usize,
    hints: &mut Vec<InlayHint>,
) {
    if !request.config.inlay_hints.let_bindings {
        return;
    }

    let Some(name_span) = let_name_span(binding, source) else {
        return;
    };
    let value = binding
        .value_span
        .and_then(|span| trimmed_span(source, span))
        .and_then(|span| source.slice(span).map(str::to_owned));
    let value_type = value
        .as_deref()
        .map(infer_inline_value_type)
        .unwrap_or("unknown");
    let label = match value {
        Some(value) => format!("= {value} : {value_type}"),
        None => format!(": {value_type}"),
    };
    push_hint(
        hints,
        line_index,
        source.as_str(),
        name_span,
        label,
        InlayHintKind::TYPE,
        start,
        end,
    );
}

fn collect_node_hints(
    node: &Node,
    source: &SourceText,
    line_index: &LineIndex,
    request: &InlayHintsRequest<'_>,
    start: usize,
    end: usize,
    hints: &mut Vec<InlayHint>,
) {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                if request.config.inlay_hints.key_paths {
                    push_hint(
                        hints,
                        line_index,
                        source.as_str(),
                        entry.key_span,
                        key_path(source, entry).unwrap_or_else(|| entry.key.clone()),
                        InlayHintKind::PARAMETER,
                        start,
                        end,
                    );
                }
                if let Some(value) = &entry.value {
                    collect_node_hints(value, source, line_index, request, start, end, hints);
                }
            }
        }
        Node::Sequence(sequence) => {
            for item in &sequence.items {
                if let Some(value) = &item.value {
                    collect_node_hints(value, source, line_index, request, start, end, hints);
                }
            }
        }
        Node::Scalar(scalar) => {
            if request.config.inlay_hints.inferred_types {
                push_hint(
                    hints,
                    line_index,
                    source.as_str(),
                    scalar.span,
                    format!(": {}", scalar_type_name(scalar)),
                    InlayHintKind::TYPE,
                    start,
                    end,
                );
            }
        }
        Node::Tag(tag) => {
            if let Some(value) = &tag.value {
                collect_node_hints(value, source, line_index, request, start, end, hints);
            }
        }
        Node::Spread(_) | Node::Conditional(_) | Node::Loop(_) | Node::Error(_) => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn push_hint(
    hints: &mut Vec<InlayHint>,
    line_index: &LineIndex,
    text: &str,
    anchor: SourceSpan,
    label: String,
    kind: InlayHintKind,
    start: usize,
    end: usize,
) {
    let offset = anchor.end;
    if offset < start || offset > end {
        return;
    }
    hints.push(InlayHint {
        position: line_index
            .offset_to_position(text, anchor.end)
            .expect("inlay hint anchor should be valid"),
        label: InlayHintLabel::String(label),
        kind: Some(kind),
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    });
}

fn directive_value_span(directive: &Directive, source: &SourceText) -> Option<SourceSpan> {
    let line = source.slice(directive.span)?;
    let name_offset = line.find(&directive.name)? + directive.name.len();
    let suffix = line.get(name_offset..)?;
    let trimmed = suffix.trim_start();
    let trimmed_prefix = suffix.len().saturating_sub(trimmed.len());
    if trimmed.is_empty() {
        return None;
    }
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + name_offset + trimmed_prefix,
        directive.span.end,
    ))
}

fn directive_target_span(directive: &Directive, source: &SourceText) -> Option<SourceSpan> {
    let value_span = directive_value_span(directive, source)?;
    let value = source.slice(value_span)?.trim();
    if let Some((target, _)) = value.rsplit_once(" as ") {
        return slice_span(value_span, value, target.trim());
    }
    slice_span(value_span, value, value)
}

fn slice_span(base: SourceSpan, full: &str, part: &str) -> Option<SourceSpan> {
    let start = full.find(part)?;
    let end = start + part.len();
    let part = strip_quotes(part);
    let full_prefix = if full.as_bytes().get(start) == Some(&b'\'')
        || full.as_bytes().get(start) == Some(&b'"')
    {
        1
    } else {
        0
    };
    let full_suffix = if part.len() != full[start..end].len() {
        1
    } else {
        0
    };
    Some(SourceSpan::new(
        base.file_id,
        base.start + start + full_prefix,
        base.start + end - full_suffix,
    ))
}

fn directive_target(directive: &Directive) -> Option<String> {
    let value = directive.value.as_deref()?.trim();
    let target = value
        .split_once(" as ")
        .map(|(target, _)| target)
        .unwrap_or(value);
    Some(strip_quotes(target).to_owned())
}

fn let_name_span(binding: &LetBinding, source: &SourceText) -> Option<SourceSpan> {
    let line = source.slice(binding.span)?;
    let local_start = line.find(&binding.name)?;
    Some(SourceSpan::new(
        binding.span.file_id,
        binding.span.start + local_start,
        binding.span.start + local_start + binding.name.len(),
    ))
}

fn trimmed_span(source: &SourceText, span: SourceSpan) -> Option<SourceSpan> {
    let text = source.slice(span)?;
    let start = text.len().saturating_sub(text.trim_start().len());
    let end = text.trim_end().len();
    Some(SourceSpan::new(
        span.file_id,
        span.start + start,
        span.start + end,
    ))
}

fn infer_inline_value_type(value: &str) -> &'static str {
    let trimmed = value.trim();
    if trimmed.starts_with("${") || trimmed.starts_with('=') {
        "lua-expression"
    } else if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        "string"
    } else if matches!(trimmed, "true" | "false") {
        "boolean"
    } else if matches!(trimmed, "null" | "nil") {
        "null"
    } else if trimmed.parse::<f64>().is_ok() {
        "number"
    } else {
        "plain"
    }
}

fn scalar_type_name(scalar: &Scalar) -> &'static str {
    match scalar.kind {
        ScalarKind::Plain => infer_inline_value_type(&scalar.text),
        ScalarKind::String | ScalarKind::BlockString => "string",
        ScalarKind::Number => "number",
        ScalarKind::LuaExpression => "lua-expression",
        ScalarKind::LuaBlock => "lua-block",
    }
}

fn strip_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn key_path(source: &SourceText, entry: &MappingEntry) -> Option<String> {
    let mut stack: Vec<(usize, String)> = vec![(0, "document[1]".to_owned())];
    for (start, line) in source.as_str().split('\n').scan(0usize, |offset, line| {
        let start = *offset;
        *offset += line.len() + 1;
        Some((start, line))
    }) {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('@')
            || trimmed.starts_with("let ")
        {
            continue;
        }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        let Some((raw_key, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        if raw_key.starts_with('-') {
            continue;
        }
        while stack.len() > 1
            && stack
                .last()
                .is_some_and(|(stack_indent, _)| *stack_indent > indent)
        {
            stack.pop();
        }
        let key = raw_key.trim().to_owned();
        let key_start = start + line.find(raw_key.trim())?;
        if key_start == entry.key_span.start {
            let mut parts = stack
                .iter()
                .map(|(_, part)| part.clone())
                .collect::<Vec<_>>();
            parts.push(key);
            return Some(parts.join("."));
        }
        if raw_value.trim().is_empty() {
            stack.push((indent + 1, key));
        }
    }
    None
}

fn inlay_label(hint: &InlayHint) -> &str {
    match &hint.label {
        InlayHintLabel::String(label) => label,
        InlayHintLabel::LabelParts(_) => "",
    }
}

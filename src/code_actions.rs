use std::collections::{BTreeSet, HashMap};

use tower_lsp::lsp_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand,
    Diagnostic as LspDiagnostic, NumberOrString, Range, TextEdit as LspTextEdit, WorkspaceEdit,
};

use crate::{document::Document, state::SessionSnapshot, syntax::SourceSpan};

const KNOWN_DIRECTIVES: &[&str] = &[
    "@luma", "@profile", "@schema", "@import", "@include", "@use", "@lua", "@meta",
];

const AMBIGUOUS_SCALARS: &[&str] = &["true", "false", "null", "nil", "yes", "no", "on", "off"];

#[derive(Debug, Clone, Copy)]
pub struct CodeActionRequest<'a> {
    pub document: &'a Document,
    pub snapshot: &'a SessionSnapshot,
    pub range: Range,
    pub context: &'a CodeActionContext,
}

pub fn collect(request: CodeActionRequest<'_>) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();
    let text = request.document.text();
    let lines = split_lines(&text);

    for diagnostic in &request.context.diagnostics {
        match diagnostic_code(diagnostic) {
            Some("L002") => {
                push_action(
                    &mut actions,
                    &mut seen,
                    duplicate_remove_action(request.document, &lines, diagnostic),
                );
                push_action(
                    &mut actions,
                    &mut seen,
                    duplicate_rename_action(request.document, &lines, diagnostic, &text),
                );
            }
            Some("L003") => push_action(
                &mut actions,
                &mut seen,
                tabs_to_spaces_action(request.document, diagnostic),
            ),
            Some("L009") => push_action(
                &mut actions,
                &mut seen,
                normalize_directive_action(request.document, &lines, diagnostic),
            ),
            _ => {}
        }
    }

    push_action(
        &mut actions,
        &mut seen,
        quote_ambiguous_scalar_action(request.document, &lines, request.range),
    );
    for action in selection_duplicate_actions(request.document, &lines, request.range) {
        push_action(&mut actions, &mut seen, Some(action));
    }
    push_action(
        &mut actions,
        &mut seen,
        selection_normalize_directive_action(request.document, &lines, request.range),
    );
    push_action(
        &mut actions,
        &mut seen,
        empty_value_to_null_action(request.document, &lines, request.range),
    );
    push_action(
        &mut actions,
        &mut seen,
        insert_luma_header_action(request.document, &lines),
    );

    if request.snapshot.config.imports.enabled {
        push_action(
            &mut actions,
            &mut seen,
            organize_directives_action(request.document, &lines),
        );
    }

    actions
}

fn push_action(
    actions: &mut Vec<CodeActionOrCommand>,
    seen: &mut BTreeSet<String>,
    action: Option<CodeAction>,
) {
    if let Some(action) = action
        && seen.insert(action.title.clone())
    {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }
}

fn tabs_to_spaces_action(document: &Document, diagnostic: &LspDiagnostic) -> Option<CodeAction> {
    let span = document.range_to_span(diagnostic.range).ok()?;
    let text = document.text();
    let slice = text.get(span.start..span.end)?;
    let replacement = slice.replace('\t', "  ");
    if replacement == slice {
        return None;
    }

    Some(code_action(
        "Replace tab with two spaces",
        CodeActionKind::QUICKFIX,
        vec![diagnostic.clone()],
        document,
        vec![edit(document, span, replacement)?],
        true,
    ))
}

fn duplicate_remove_action(
    document: &Document,
    lines: &[Line<'_>],
    diagnostic: &LspDiagnostic,
) -> Option<CodeAction> {
    let line = lines.get(diagnostic.range.start.line as usize)?;
    let key = mapping_key(line.text.trim())?;
    let delete_span = SourceSpan::new(document.file_id(), line.start, line.end_with_newline);

    Some(code_action(
        format!("Remove duplicate key `{key}`"),
        CodeActionKind::QUICKFIX,
        vec![diagnostic.clone()],
        document,
        vec![edit(document, delete_span, String::new())?],
        true,
    ))
}

fn duplicate_rename_action(
    document: &Document,
    lines: &[Line<'_>],
    diagnostic: &LspDiagnostic,
    text: &str,
) -> Option<CodeAction> {
    let line = lines.get(diagnostic.range.start.line as usize)?;
    let trimmed = line.text.trim();
    let key = mapping_key(trimmed)?;
    let colon = line.text.find(':')?;
    let key_start_in_line = line.text.find(key)?;
    let key_span = SourceSpan::new(
        document.file_id(),
        line.start + key_start_in_line,
        line.start + colon,
    );
    let new_name = next_available_key_name(text, key);
    if new_name == key {
        return None;
    }

    Some(code_action(
        format!("Rename duplicate key to `{new_name}`"),
        CodeActionKind::QUICKFIX,
        vec![diagnostic.clone()],
        document,
        vec![edit(document, key_span, new_name)?],
        false,
    ))
}

fn normalize_directive_action(
    document: &Document,
    lines: &[Line<'_>],
    diagnostic: &LspDiagnostic,
) -> Option<CodeAction> {
    let line = lines.get(diagnostic.range.start.line as usize)?;
    let trimmed = line.text.trim_start();
    let name = trimmed.split_whitespace().next()?;
    let replacement = best_directive_suggestion(name)?;
    if replacement == name {
        return None;
    }
    let start = line.start + line.leading_ws;
    let span = SourceSpan::new(document.file_id(), start, start + name.len());

    Some(code_action(
        format!("Normalize directive to `{replacement}`"),
        CodeActionKind::QUICKFIX,
        vec![diagnostic.clone()],
        document,
        vec![edit(document, span, replacement.to_string())?],
        true,
    ))
}

fn quote_ambiguous_scalar_action(
    document: &Document,
    lines: &[Line<'_>],
    range: Range,
) -> Option<CodeAction> {
    let selection = document.range_to_span(range).ok()?;

    for line in lines {
        if !spans_overlap(
            selection,
            SourceSpan::new(document.file_id(), line.start, line.end),
        ) {
            continue;
        }
        let (value, span) = scalar_span(document.file_id(), line)?;
        if !AMBIGUOUS_SCALARS.contains(&value) {
            continue;
        }
        return Some(code_action(
            format!("Quote ambiguous scalar `{value}`"),
            CodeActionKind::QUICKFIX,
            Vec::new(),
            document,
            vec![edit(document, span, format!("\"{value}\""))?],
            false,
        ));
    }

    None
}

fn selection_duplicate_actions(
    document: &Document,
    lines: &[Line<'_>],
    range: Range,
) -> Vec<CodeAction> {
    let Ok(selection) = document.range_to_span(range) else {
        return Vec::new();
    };

    let mut actions = Vec::new();
    let mut seen_keys = BTreeSet::new();
    let text = document.text();

    for line in lines {
        let trimmed = line.text.trim();
        let Some(key) = mapping_key(trimmed) else {
            continue;
        };
        if seen_keys.insert(key.to_string()) {
            continue;
        }
        let line_span = SourceSpan::new(document.file_id(), line.start, line.end);
        if !spans_overlap(selection, line_span) {
            continue;
        }

        let delete_span = SourceSpan::new(document.file_id(), line.start, line.end_with_newline);
        if let Some(delete_edit) = edit(document, delete_span, String::new()) {
            actions.push(code_action(
                format!("Remove duplicate key `{key}`"),
                CodeActionKind::QUICKFIX,
                Vec::new(),
                document,
                vec![delete_edit],
                true,
            ));
        }

        if let Some(colon) = line.text.find(':')
            && let Some(key_start_in_line) = line.text.find(key)
        {
            let key_span = SourceSpan::new(
                document.file_id(),
                line.start + key_start_in_line,
                line.start + colon,
            );
            let new_name = next_available_key_name(&text, key);
            if let Some(rename_edit) = edit(document, key_span, new_name.clone()) {
                actions.push(code_action(
                    format!("Rename duplicate key to `{new_name}`"),
                    CodeActionKind::QUICKFIX,
                    Vec::new(),
                    document,
                    vec![rename_edit],
                    false,
                ));
            }
        }
    }

    actions
}

fn selection_normalize_directive_action(
    document: &Document,
    lines: &[Line<'_>],
    range: Range,
) -> Option<CodeAction> {
    let selection = document.range_to_span(range).ok()?;

    for line in lines {
        let line_span = SourceSpan::new(document.file_id(), line.start, line.end);
        if !spans_overlap(selection, line_span) {
            continue;
        }
        let trimmed = line.text.trim_start();
        if !trimmed.starts_with('@') {
            continue;
        }
        let name = trimmed.split_whitespace().next()?;
        if KNOWN_DIRECTIVES.contains(&name) {
            continue;
        }
        let replacement = best_directive_suggestion(name)?;
        if replacement == name {
            continue;
        }
        let start = line.start + line.leading_ws;
        let span = SourceSpan::new(document.file_id(), start, start + name.len());
        return Some(code_action(
            format!("Normalize directive to `{replacement}`"),
            CodeActionKind::QUICKFIX,
            Vec::new(),
            document,
            vec![edit(document, span, replacement.to_string())?],
            true,
        ));
    }

    None
}

fn empty_value_to_null_action(
    document: &Document,
    lines: &[Line<'_>],
    range: Range,
) -> Option<CodeAction> {
    let selection = document.range_to_span(range).ok()?;

    for line in lines {
        if !spans_overlap(
            selection,
            SourceSpan::new(document.file_id(), line.start, line.end),
        ) {
            continue;
        }
        let trimmed = line.text.trim();
        if trimmed.starts_with('@') || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        let (key, value) = trimmed.split_once(':')?;
        if key.trim().is_empty() || !value.trim().is_empty() {
            continue;
        }

        let insert_at = SourceSpan::new(
            document.file_id(),
            line.start + line.text.len(),
            line.start + line.text.len(),
        );
        return Some(code_action(
            format!("Convert empty value for `{}` to null", key.trim()),
            CodeActionKind::QUICKFIX,
            Vec::new(),
            document,
            vec![edit(document, insert_at, " null".to_string())?],
            false,
        ));
    }

    None
}

fn insert_luma_header_action(document: &Document, lines: &[Line<'_>]) -> Option<CodeAction> {
    if lines
        .iter()
        .any(|line| line.text.trim_start().starts_with("@luma ") || line.text.trim() == "@luma")
    {
        return None;
    }

    let line_ending = preferred_line_ending(&document.text());
    Some(code_action(
        "Insert @luma 0.1",
        CodeActionKind::QUICKFIX,
        Vec::new(),
        document,
        vec![edit(
            document,
            SourceSpan::new(document.file_id(), 0, 0),
            format!("@luma 0.1{line_ending}"),
        )?],
        false,
    ))
}

fn organize_directives_action(document: &Document, lines: &[Line<'_>]) -> Option<CodeAction> {
    let start = lines
        .iter()
        .position(|line| line.text.trim_start().starts_with('@'))?;
    let mut end = start;
    while end < lines.len() && lines[end].text.trim_start().starts_with('@') {
        end += 1;
    }
    if end - start < 2 {
        return None;
    }

    let mut sorted = lines[start..end]
        .iter()
        .map(|line| line.text.to_string())
        .collect::<Vec<_>>();
    sorted.sort_by_key(|line| directive_sort_key(line));

    let original = lines[start..end]
        .iter()
        .map(|line| line.text)
        .collect::<Vec<_>>();
    if sorted.iter().map(String::as_str).eq(original) {
        return None;
    }

    let line_ending = preferred_line_ending(&document.text());
    let mut replacement = sorted.join(line_ending);
    if lines[end - 1].has_newline {
        replacement.push_str(line_ending);
    }

    Some(code_action(
        "Organize directives and imports",
        CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
        Vec::new(),
        document,
        vec![edit(
            document,
            SourceSpan::new(
                document.file_id(),
                lines[start].start,
                lines[end - 1].end_with_newline,
            ),
            replacement,
        )?],
        false,
    ))
}

fn code_action(
    title: impl Into<String>,
    kind: CodeActionKind,
    diagnostics: Vec<LspDiagnostic>,
    document: &Document,
    edits: Vec<LspTextEdit>,
    is_preferred: bool,
) -> CodeAction {
    CodeAction {
        title: title.into(),
        kind: Some(kind),
        diagnostics: (!diagnostics.is_empty()).then_some(diagnostics),
        edit: Some(WorkspaceEdit {
            changes: Some(HashMap::from([(document.uri().clone(), edits)])),
            document_changes: None,
            change_annotations: None,
        }),
        is_preferred: Some(is_preferred),
        disabled: None,
        command: None,
        data: None,
    }
}

fn edit(document: &Document, span: SourceSpan, new_text: String) -> Option<LspTextEdit> {
    Some(LspTextEdit {
        range: document.span_to_range(span).ok()?,
        new_text,
    })
}

fn diagnostic_code(diagnostic: &LspDiagnostic) -> Option<&str> {
    match diagnostic.code.as_ref()? {
        NumberOrString::String(code) => Some(code.as_str()),
        NumberOrString::Number(_) => None,
    }
}

fn mapping_key(trimmed: &str) -> Option<&str> {
    let (key, _) = trimmed.split_once(':')?;
    let key = key.trim();
    (!key.is_empty()).then_some(key)
}

fn next_available_key_name(text: &str, key: &str) -> String {
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{key}_{suffix}");
        let exists = split_lines(text)
            .iter()
            .filter_map(|line| mapping_key(line.text.trim()))
            .any(|existing| existing == candidate);
        if !exists {
            return candidate;
        }
        suffix += 1;
    }
}

fn best_directive_suggestion(name: &str) -> Option<&'static str> {
    if let Some(exact) = KNOWN_DIRECTIVES
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        return Some(exact);
    }

    KNOWN_DIRECTIVES
        .iter()
        .copied()
        .map(|candidate| {
            (
                candidate,
                levenshtein(&name.to_ascii_lowercase(), &candidate.to_ascii_lowercase()),
            )
        })
        .filter(|(_, distance)| *distance <= 2)
        .min_by_key(|(_, distance)| *distance)
        .map(|(candidate, _)| candidate)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut prev = (0..=right_chars.len()).collect::<Vec<_>>();

    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let cost = usize::from(left_char != *right_char);
            current.push(
                (prev[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(prev[right_index] + cost),
            );
        }
        prev = current;
    }

    *prev.last().unwrap_or(&0)
}

fn scalar_span<'a>(
    file_id: crate::syntax::FileId,
    line: &'a Line<'a>,
) -> Option<(&'a str, SourceSpan)> {
    let trimmed = line.text.trim_start();
    let (value, offset) = if trimmed.starts_with('@') {
        let name = trimmed.split_whitespace().next()?;
        let rest = trimmed[name.len()..].trim_start();
        (rest, line.text.find(rest)?)
    } else if let Some(rest) = trimmed.strip_prefix("let ") {
        let equals = rest.find('=')? + 4;
        let value = line.text[equals + 1..].trim_start();
        (value, line.text.find(value)?)
    } else if let Some((_, value)) = line.text.split_once(':') {
        let value = value.trim_start();
        (value, line.text.find(value)?)
    } else if let Some(rest) = trimmed.strip_prefix('-') {
        let value = rest.trim_start();
        (value, line.text.find(value)?)
    } else {
        (trimmed, line.leading_ws)
    };

    if value.is_empty()
        || value.starts_with('"')
        || value.starts_with('\'')
        || value.contains(char::is_whitespace)
    {
        return None;
    }

    let span = SourceSpan::new(
        file_id,
        line.start + offset,
        line.start + offset + value.len(),
    );
    Some((value, span))
}

fn spans_overlap(left: SourceSpan, right: SourceSpan) -> bool {
    left.start <= right.end && right.start <= left.end
}

fn directive_sort_key(line: &str) -> (usize, String) {
    let name = line.split_whitespace().next().unwrap_or_default();
    let bucket = match name {
        "@luma" => 0,
        "@profile" => 1,
        "@schema" => 2,
        "@import" => 3,
        "@include" => 4,
        "@use" => 5,
        _ => 6,
    };
    (bucket, line.to_ascii_lowercase())
}

fn preferred_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    start: usize,
    end: usize,
    end_with_newline: usize,
    leading_ws: usize,
    text: &'a str,
    has_newline: bool,
}

fn split_lines(text: &str) -> Vec<Line<'_>> {
    if text.is_empty() {
        return vec![Line {
            start: 0,
            end: 0,
            end_with_newline: 0,
            leading_ws: 0,
            text: "",
            has_newline: false,
        }];
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    for segment in text.split_inclusive('\n') {
        let has_newline = segment.ends_with('\n');
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let leading_ws = body
            .char_indices()
            .find_map(|(offset, ch)| (ch != ' ' && ch != '\t').then_some(offset))
            .unwrap_or(body.len());
        let end_with_newline = start + segment.len();
        lines.push(Line {
            start,
            end: start + body.len(),
            end_with_newline,
            leading_ws,
            text: body,
            has_newline,
        });
        start = end_with_newline;
    }
    lines
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::{
        CodeActionContext, CodeActionOrCommand, Diagnostic, NumberOrString, Position, Range, Url,
    };

    use super::{CodeActionRequest, collect};
    use crate::{document::Document, state::SessionSnapshot, syntax::FileId};

    #[test]
    fn collects_expected_quickfixes_and_source_actions() {
        let uri = Url::parse("file:///workspace/test.luma").unwrap();
        let document = Document::new(
            uri,
            1,
            "@profil dev\n@import \"./b.luma\"\n@import \"./a.luma\"\nname:\n  enabled: true\nservice: one\nservice: two\n",
            FileId(1),
        );
        let diagnostics = vec![
            Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 7)),
                code: Some(NumberOrString::String("L009".to_string())),
                ..Diagnostic::default()
            },
            Diagnostic {
                range: Range::new(Position::new(6, 0), Position::new(6, 7)),
                code: Some(NumberOrString::String("L002".to_string())),
                ..Diagnostic::default()
            },
        ];

        let actions = collect(CodeActionRequest {
            document: &document,
            snapshot: &SessionSnapshot::default(),
            range: Range::new(Position::new(3, 0), Position::new(4, 15)),
            context: &CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
        });

        let titles = actions
            .into_iter()
            .map(|action| match action {
                CodeActionOrCommand::CodeAction(action) => action.title,
                CodeActionOrCommand::Command(command) => command.title,
            })
            .collect::<Vec<_>>();

        assert!(
            titles
                .iter()
                .any(|title| title.contains("Normalize directive"))
        );
        assert!(
            titles
                .iter()
                .any(|title| title.contains("Remove duplicate key"))
        );
        assert!(
            titles
                .iter()
                .any(|title| title.contains("Rename duplicate key"))
        );
        assert!(
            titles
                .iter()
                .any(|title| title.contains("Convert empty value"))
        );
        assert!(
            titles
                .iter()
                .any(|title| title.contains("Insert @luma 0.1"))
        );
        assert!(
            titles
                .iter()
                .any(|title| title.contains("Organize directives and imports"))
        );
    }
}

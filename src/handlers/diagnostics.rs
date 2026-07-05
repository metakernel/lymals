use tower_lsp::lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation, DiagnosticSeverity as LspSeverity,
    DiagnosticTag as LspTag, Location, NumberOrString, Position, Range,
};

use crate::{
    diagnostics::{self, Diagnostic, DiagnosticSeverity, DiagnosticTag},
    document::Document,
};

pub(super) fn collect_lsp_diagnostics(document: &mut Document) -> Vec<LspDiagnostic> {
    let parsed = document.parsed();
    let source = parsed.source.clone();
    let diagnostics = diagnostics::collect(&parsed);
    diagnostics
        .iter()
        .map(|diagnostic| to_lsp_diagnostic(document, &source, diagnostic))
        .collect()
}

fn to_lsp_diagnostic(
    document: &Document,
    source_text: &crate::syntax::SourceText,
    diagnostic: &Diagnostic,
) -> LspDiagnostic {
    let range = diagnostic
        .primary_span
        .and_then(|span| diagnostics::stable_span(source_text, span))
        .and_then(|span| document.span_to_range(span).ok())
        .unwrap_or_else(default_range);

    let related_information = diagnostic
        .related_spans
        .iter()
        .filter_map(|related| {
            let span = diagnostics::stable_span(source_text, related.span)?;
            let range = document.span_to_range(span).ok()?;
            Some(DiagnosticRelatedInformation {
                location: Location {
                    uri: document.uri().clone(),
                    range,
                },
                message: related.message.clone(),
            })
        })
        .collect::<Vec<_>>();

    let tags = diagnostic
        .tags
        .iter()
        .map(|tag| match tag {
            DiagnosticTag::Deprecated => LspTag::DEPRECATED,
            DiagnosticTag::Unnecessary => LspTag::UNNECESSARY,
        })
        .collect::<Vec<_>>();

    LspDiagnostic {
        range,
        severity: Some(match diagnostic.severity {
            DiagnosticSeverity::Info => LspSeverity::INFORMATION,
            DiagnosticSeverity::Warning => LspSeverity::WARNING,
            DiagnosticSeverity::Error => LspSeverity::ERROR,
        }),
        code: Some(NumberOrString::String(diagnostic.code.clone())),
        source: Some(diagnostic.source.clone()),
        message: diagnostic_message(diagnostic),
        related_information: (!related_information.is_empty()).then_some(related_information),
        tags: (!tags.is_empty()).then_some(tags),
        ..LspDiagnostic::default()
    }
}

fn diagnostic_message(diagnostic: &Diagnostic) -> String {
    if diagnostic.notes.is_empty() {
        return diagnostic.message.clone();
    }

    format!("{}\n\n{}", diagnostic.message, diagnostic.notes.join("\n"))
}

fn default_range() -> Range {
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

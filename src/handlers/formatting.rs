use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DocumentFormattingParams, DocumentRangeFormattingParams, TextEdit as LspTextEdit,
};

use crate::{
    formatting,
    syntax::{SourceSpan, TextEdit},
};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_document_formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<LspTextEdit>>> {
        let snapshot = self.state.snapshot();
        if !snapshot.config.formatting.enabled {
            return Ok(None);
        }

        let uri = params.text_document.uri;
        let edits = self.state.with_document_mut(&uri, |document| {
            let text = document.text();
            let parsed = document.parsed();
            let formatted = formatting::format_text(
                document.file_id(),
                document.uri().as_str(),
                parsed.backend,
                &text,
            );
            if !formatted.changed {
                return None;
            }

            Some(vec![to_lsp_edit(
                document,
                TextEdit {
                    range: parsed.source.full_span(),
                    text: formatted.text,
                },
            )])
        });

        Ok(edits.flatten())
    }

    pub(super) async fn handle_document_range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<LspTextEdit>>> {
        let snapshot = self.state.snapshot();
        if !snapshot.config.formatting.enabled {
            return Ok(None);
        }

        let uri = params.text_document.uri;
        let edits = self.state.with_document(&uri, |document| {
            let text = document.text();
            let span = document.range_to_span(params.range).ok()?;
            let edit = formatting::format_range(&text, document.file_id(), span)?;
            Some(vec![to_lsp_edit(document, edit)])
        });

        Ok(edits.flatten())
    }
}

fn to_lsp_edit(document: &crate::document::Document, edit: TextEdit) -> LspTextEdit {
    LspTextEdit {
        range: document
            .span_to_range(SourceSpan::new(
                document.file_id(),
                edit.range.start,
                edit.range.end,
            ))
            .expect("edit spans should map to document range"),
        new_text: edit.text,
    }
}

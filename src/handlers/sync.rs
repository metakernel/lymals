use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, TextDocumentContentChangeEvent, Url,
};

use crate::document::Document;
use crate::position::LineIndex;
use crate::tasks::VersionedDocumentGuard;

use super::{LumaLanguageServer, diagnostics::collect_lsp_diagnostics};

impl LumaLanguageServer {
    pub(super) async fn handle_did_open(&self, params: DidOpenTextDocumentParams) {
        let text_document = params.text_document;
        let uri = text_document.uri;
        let version = text_document.version;

        self.state
            .open_document(uri.clone(), version, text_document.text);
        self.publish_document_diagnostics(&uri).await;
    }

    pub(super) async fn handle_did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let updated = self.state.with_document_mut(&uri, |document| {
            apply_content_changes(document, version, &params.content_changes)
        });

        match updated {
            Some(Ok(())) => self.publish_document_diagnostics(&uri).await,
            Some(Err(error)) => {
                self.trace(
                    "textDocument/didChange ignored",
                    Some(format!("uri={uri} reason={error}")),
                )
                .await;
            }
            None => {
                self.trace(
                    "textDocument/didChange ignored",
                    Some(format!("uri={uri} reason=document-not-open")),
                )
                .await;
            }
        }
    }

    pub(super) async fn handle_did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish_document_diagnostics(&params.text_document.uri)
            .await;
    }

    pub(super) async fn handle_did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.state.close_document(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    pub(super) async fn publish_document_diagnostics(&self, uri: &Url) {
        let snapshot = self.state.snapshot();
        let Some((guard, diagnostics)) = self.state.with_document_mut(uri, |document| {
            let version = document.version();
            let guard = VersionedDocumentGuard::new(uri.clone(), version);
            let diagnostics = collect_lsp_diagnostics(document, &snapshot);

            (guard, diagnostics)
        }) else {
            return;
        };

        if !guard.is_current(self.state.document_version(guard.uri())) {
            self.trace(
                "diagnostics result discarded",
                Some(format!(
                    "uri={} version={} reason=stale",
                    guard.uri(),
                    guard.version()
                )),
            )
            .await;
            return;
        }

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, Some(guard.version()))
            .await;
    }
}

fn apply_content_changes(
    document: &mut Document,
    version: i32,
    changes: &[TextDocumentContentChangeEvent],
) -> Result<(), String> {
    let mut text = document.text();

    for change in changes {
        match change.range {
            Some(range) => {
                let line_index = LineIndex::new(&text);
                let span = line_index
                    .range_to_span(&text, document.file_id(), range)
                    .map_err(|error| error.to_string())?;
                text.replace_range(span.start..span.end, &change.text);
            }
            None => text = change.text.clone(),
        }
    }

    document.update(version, text);
    Ok(())
}

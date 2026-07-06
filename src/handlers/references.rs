use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Location, ReferenceParams};

use crate::references::{self, ReferencesRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let snapshot = self.state.snapshot();
        let open_documents = self.state.open_document_snapshots();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            let locations = references::find_references(ReferencesRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                offset,
                include_declaration: params.context.include_declaration,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
                open_documents: &open_documents,
            });

            (!locations.is_empty()).then_some(locations)
        });

        Ok(response.flatten())
    }
}

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{CompletionParams, CompletionResponse};

use crate::completion::{self, CompletionRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let snapshot = self.state.snapshot();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            completion::complete(CompletionRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                position,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
            })
        });

        Ok(response.flatten())
    }
}

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{InlayHint, InlayHintParams};

use crate::inlay_hints::{self, InlayHintsRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_inlay_hints(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.clone();
        let snapshot = self.state.snapshot();

        let hints = self.state.with_document(&uri, |document| {
            let text = document.text();
            inlay_hints::collect(InlayHintsRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                params: &params,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
            })
        });

        Ok(hints.flatten())
    }
}

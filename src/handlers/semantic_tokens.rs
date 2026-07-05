use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{SemanticTokensParams, SemanticTokensResult};

use crate::semantic_tokens;

use super::LumaLanguageServer;

impl LumaLanguageServer {
    pub(super) async fn handle_semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let snapshot = self.state.snapshot();
        if !snapshot.config.semantic_tokens.enabled {
            return Ok(None);
        }

        let tokens = self.state.with_document(&uri, |document| {
            semantic_tokens::tokenize(
                document.file_id(),
                document.uri().as_str(),
                &document.text(),
            )
        });

        Ok(tokens.map(SemanticTokensResult::Tokens))
    }
}

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{CodeActionParams, CodeActionResponse};

use crate::code_actions::{self, CodeActionRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let snapshot = self.state.snapshot();
        let uri = params.text_document.uri;
        let range = params.range;
        let context = params.context;

        let response = self.state.with_document(&uri, |document| {
            code_actions::collect(CodeActionRequest {
                document,
                snapshot: &snapshot,
                range,
                context: &context,
            })
        });

        Ok(response.filter(|actions| !actions.is_empty()))
    }
}

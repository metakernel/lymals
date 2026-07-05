use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{SymbolInformation, WorkspaceSymbolParams};

use crate::index::WorkspaceIndex;

use super::LumaLanguageServer;

impl LumaLanguageServer {
    pub(super) async fn handle_workspace_symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let snapshot = self.state.snapshot();
        let open_documents = self.state.open_document_snapshots();
        let index = WorkspaceIndex::load(
            &open_documents,
            &snapshot.workspace.folders,
            &snapshot.config,
        );
        let symbols = index.workspace_symbols(&params.query);
        Ok(Some(symbols))
    }
}

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{SelectionRange, SelectionRangeParams};

use crate::selection_ranges::{self, SelectionRangesRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_selection_ranges(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let uri = params.text_document.uri.clone();
        let ranges = self.state.with_document(&uri, |document| {
            let text = document.text();
            selection_ranges::collect(SelectionRangesRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                params: &params,
            })
        });

        Ok(ranges.flatten())
    }
}

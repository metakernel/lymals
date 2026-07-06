use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{FoldingRange, FoldingRangeParams};

use crate::folding::{self, FoldingRangesRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_folding_ranges(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri.clone();
        let ranges = self.state.with_document(&uri, |document| {
            let text = document.text();
            folding::collect(FoldingRangesRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
            })
        });

        Ok(ranges.flatten())
    }
}

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};

use crate::hover::{self, HoverRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            let hover = hover::hover(HoverRequest {
                uri: document.uri().as_str(),
                text: &text,
                file_id: document.file_id(),
                offset,
            })?;
            let range = document.span_to_range(hover.span).ok()?;

            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: hover.markdown,
                }),
                range: Some(range),
            })
        });

        Ok(response.flatten())
    }
}

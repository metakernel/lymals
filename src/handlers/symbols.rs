use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Range};

use crate::{parser, symbols::DocumentSymbolNode, syntax::SourceSpan};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let parsed = parser::parse_fallback(document.file_id(), document.uri().as_str(), &text);
            #[cfg(feature = "upstream-lyma")]
            let crate::syntax::ParsedFile::Fallback(file) = &parsed.file else {
                return None;
            };

            #[cfg(not(feature = "upstream-lyma"))]
            let file = match &parsed.file {
                crate::syntax::ParsedFile::Fallback(file) => file,
            };

            let symbols = crate::symbols::build_document_symbols(&file.ast, &parsed.source)
                .into_iter()
                .map(|symbol| {
                    to_lsp_document_symbol(&symbol, |span| document.span_to_range(span).ok())
                })
                .collect::<Option<Vec<_>>>()?;

            Some(DocumentSymbolResponse::Nested(symbols))
        });

        Ok(response.flatten())
    }
}

fn to_lsp_document_symbol(
    symbol: &DocumentSymbolNode,
    to_range: impl Copy + Fn(SourceSpan) -> Option<Range>,
) -> Option<DocumentSymbol> {
    let range = to_range(symbol.span)?;
    let selection_range = to_range(symbol.selection_span)?;
    let children = symbol
        .children
        .iter()
        .map(|child| to_lsp_document_symbol(child, to_range))
        .collect::<Option<Vec<_>>>();

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: symbol.name.clone(),
        detail: symbol.detail.clone(),
        kind: symbol.kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: children.filter(|items| !items.is_empty()),
    })
}

mod fallback;
#[cfg(feature = "upstream-luma")]
mod upstream;

use crate::{
    diagnostics::Diagnostic,
    syntax::{
        FileId, FormatResult, ParsedFile, ParsedRanges, ParserBackend, SourceText, Symbol,
        TextEdit, Token,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument {
    pub backend: ParserBackend,
    pub source: SourceText,
    pub file: ParsedFile,
    pub ranges: ParsedRanges,
    pub tokens: Option<Vec<Token>>,
    pub symbols: Option<Vec<Symbol>>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParsedDocument {
    #[must_use]
    pub fn full_document_edit(&self, text: String) -> TextEdit {
        TextEdit {
            range: self.source.full_span(),
            text,
        }
    }
}

pub(crate) trait ParserAdapter: Send + Sync {
    fn backend(&self) -> ParserBackend;
    fn parse(&self, file_id: FileId, name: &str, text: &str) -> ParsedDocument;
    fn format_document(&self, parsed: &ParsedDocument) -> Option<FormatResult>;
    fn format_text_edits(&self, parsed: &ParsedDocument) -> Option<Vec<TextEdit>> {
        self.format_document(parsed)
            .map(|formatted| vec![parsed.full_document_edit(formatted.text)])
    }
}

#[must_use]
pub fn parse(file_id: FileId, name: &str, text: &str) -> ParsedDocument {
    active().parse(file_id, name, text)
}

#[must_use]
pub fn format_document(parsed: &ParsedDocument) -> Option<FormatResult> {
    active().format_document(parsed)
}

#[must_use]
pub fn format_text_edits(parsed: &ParsedDocument) -> Option<Vec<TextEdit>> {
    active().format_text_edits(parsed)
}

#[must_use]
pub fn backend() -> ParserBackend {
    active().backend()
}

#[must_use]
pub fn parse_fallback(file_id: FileId, name: &str, text: &str) -> ParsedDocument {
    fallback::parse_with_fallback(file_id, name, text)
}

fn active() -> &'static dyn ParserAdapter {
    #[cfg(feature = "upstream-luma")]
    {
        &upstream::UPSTREAM_PARSER
    }

    #[cfg(not(feature = "upstream-luma"))]
    {
        &fallback::FALLBACK_PARSER
    }
}

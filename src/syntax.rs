use std::sync::Arc;

use crate::ast::AstFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParserBackend {
    Fallback,
    UpstreamLyma,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceSpan {
    pub file_id: FileId,
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    #[must_use]
    pub const fn new(file_id: FileId, start: usize, end: usize) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceText {
    pub file_id: FileId,
    pub name: Arc<str>,
    pub text: Arc<str>,
    line_starts: Arc<[usize]>,
}

impl SourceText {
    #[must_use]
    pub fn new(file_id: FileId, name: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];

        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }

        Self {
            file_id,
            name: name.into(),
            text,
            line_starts: line_starts.into(),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn full_span(&self) -> SourceSpan {
        SourceSpan::new(self.file_id, 0, self.text.len())
    }

    #[must_use]
    pub fn position(&self, offset: usize) -> SourcePosition {
        let clamped = offset.min(self.text.len());
        let line_index = self.line_starts.partition_point(|start| *start <= clamped) - 1;
        let line_start = self.line_starts[line_index];

        SourcePosition {
            line: line_index + 1,
            column: clamped.saturating_sub(line_start) + 1,
        }
    }

    #[must_use]
    pub fn slice(&self, span: SourceSpan) -> Option<&str> {
        if span.file_id != self.file_id || span.start > span.end || span.end > self.text.len() {
            return None;
        }

        self.text.get(span.start..span.end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRanges {
    pub file: SourceSpan,
    pub documents: Vec<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackParsedFile {
    pub span: SourceSpan,
    pub document_spans: Vec<SourceSpan>,
    pub ast: AstFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedFile {
    Fallback(FallbackParsedFile),
    #[cfg(feature = "upstream-lyma")]
    Upstream(lyma::parser::LymaFile),
}

impl ParsedFile {
    #[must_use]
    pub fn fallback(span: SourceSpan, document_spans: Vec<SourceSpan>, ast: AstFile) -> Self {
        Self::Fallback(FallbackParsedFile {
            span,
            document_spans,
            ast,
        })
    }

    #[must_use]
    pub fn backend(&self) -> ParserBackend {
        match self {
            Self::Fallback(_) => ParserBackend::Fallback,
            #[cfg(feature = "upstream-lyma")]
            Self::Upstream(_) => ParserBackend::UpstreamLyma,
        }
    }

    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Fallback(file) => file.span,
            #[cfg(feature = "upstream-lyma")]
            Self::Upstream(file) => map_span(file.span),
        }
    }

    #[must_use]
    pub fn document_spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::Fallback(file) => file.document_spans.clone(),
            #[cfg(feature = "upstream-lyma")]
            Self::Upstream(file) => file
                .documents
                .iter()
                .map(|document| map_span(document.span))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenKind {
    Identifier,
    DirectiveName,
    TagName,
    Number,
    String,
    PlainString,
    Comment,
    DocumentSeparator,
    DocumentTerminator,
    BlockHeader,
    Colon,
    Dash,
    Spread,
    Equals,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    KeywordLet,
    KeywordAs,
    KeywordIn,
    LineBreak,
    EndOfFile,
    Error,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    Document,
    Directive,
    Variable,
    MappingKey,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: SourceSpan,
    pub selection_span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatResult {
    pub text: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: SourceSpan,
    pub text: String,
}

#[cfg(feature = "upstream-lyma")]
#[must_use]
pub fn map_span(span: lyma::parser::Span) -> SourceSpan {
    SourceSpan::new(FileId(span.file_id.0), span.start, span.end)
}

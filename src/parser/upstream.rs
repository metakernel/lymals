use crate::{
    diagnostics::{Diagnostic, DiagnosticSeverity, RelatedDiagnosticSpan},
    parser::{ParsedDocument, ParserAdapter},
    syntax::{
        FileId, FormatResult, ParsedFile, ParsedRanges, ParserBackend, SourceSpan, SourceText,
        Symbol, SymbolKind, TextEdit, Token, TokenKind, map_span,
    },
};

pub(super) static UPSTREAM_PARSER: UpstreamParser = UpstreamParser;

pub(super) struct UpstreamParser;

impl ParserAdapter for UpstreamParser {
    fn backend(&self) -> ParserBackend {
        ParserBackend::UpstreamLyma
    }

    fn parse(&self, file_id: FileId, name: &str, text: &str) -> ParsedDocument {
        let upstream_file_id = lyma::parser::FileId(file_id.0);
        let parsed = lyma::parser::parse_str(upstream_file_id, name, text);
        let lexed = lyma::parser::lex_str(upstream_file_id, name, text);
        let source = map_source(&parsed.source);
        let file = ParsedFile::Upstream(parsed.file.clone());
        let diagnostics = merge_diagnostics(&parsed.diagnostics, &lexed.diagnostics);
        let tokens = Some(lexed.tokens.iter().map(map_token).collect());
        let symbols = extract_symbols(&parsed.file);
        let ranges = ParsedRanges {
            file: file.span(),
            documents: file.document_spans(),
        };

        ParsedDocument {
            backend: ParserBackend::UpstreamLyma,
            source,
            file,
            ranges,
            tokens,
            symbols: (!symbols.is_empty()).then_some(symbols),
            diagnostics,
        }
    }

    fn format_document(&self, parsed: &ParsedDocument) -> Option<FormatResult> {
        let formatted = lyma::parser::format_str(
            lyma::parser::FileId(parsed.source.file_id.0),
            &parsed.source.name,
            parsed.source.as_str(),
        );

        Some(FormatResult {
            text: formatted.formatted.text,
            changed: formatted.formatted.changed,
        })
    }

    fn format_text_edits(&self, parsed: &ParsedDocument) -> Option<Vec<TextEdit>> {
        let (_, edit) =
            lyma::tooling::format_document_text_edit(&parsed.source.name, parsed.source.as_str());
        Some(vec![TextEdit {
            range: SourceSpan::new(parsed.source.file_id, edit.range.start, edit.range.end),
            text: edit.text,
        }])
    }
}

fn map_source(source: &lyma::parser::SourceText) -> SourceText {
    SourceText::new(
        FileId(source.source.id.0),
        source.source.name.clone(),
        source.source.text.clone(),
    )
}

fn merge_diagnostics(
    parsed: &[lyma::parser::Diagnostic],
    lexed: &[lyma::parser::Diagnostic],
) -> Vec<Diagnostic> {
    let mut merged = Vec::new();

    for diagnostic in parsed.iter().chain(lexed) {
        let mapped = map_diagnostic(diagnostic);
        if !merged.contains(&mapped) {
            merged.push(mapped);
        }
    }

    merged
}

fn map_diagnostic(diagnostic: &lyma::parser::Diagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code.code().to_owned(),
        severity: match diagnostic.severity {
            lyma::parser::Severity::Info => DiagnosticSeverity::Info,
            lyma::parser::Severity::Warning => DiagnosticSeverity::Warning,
            lyma::parser::Severity::Error => DiagnosticSeverity::Error,
        },
        source: "lyma.parser".to_owned(),
        message: diagnostic.message.clone(),
        primary_span: diagnostic.primary_span.map(map_span),
        related_spans: diagnostic
            .related_spans
            .iter()
            .map(|related| RelatedDiagnosticSpan {
                span: map_span(related.span),
                message: related.message.clone(),
            })
            .collect(),
        tags: Vec::new(),
        notes: diagnostic.notes.clone(),
    }
}

fn map_token(token: &lyma::parser::Token) -> Token {
    Token {
        kind: match token.kind {
            lyma::parser::TokenKind::Identifier => TokenKind::Identifier,
            lyma::parser::TokenKind::DirectiveName => TokenKind::DirectiveName,
            lyma::parser::TokenKind::TagName => TokenKind::TagName,
            lyma::parser::TokenKind::Number => TokenKind::Number,
            lyma::parser::TokenKind::String => TokenKind::String,
            lyma::parser::TokenKind::PlainString => TokenKind::PlainString,
            lyma::parser::TokenKind::Comment => TokenKind::Comment,
            lyma::parser::TokenKind::DocumentSeparator => TokenKind::DocumentSeparator,
            lyma::parser::TokenKind::DocumentTerminator => TokenKind::DocumentTerminator,
            lyma::parser::TokenKind::BlockHeader(_) => TokenKind::BlockHeader,
            lyma::parser::TokenKind::Colon => TokenKind::Colon,
            lyma::parser::TokenKind::Dash => TokenKind::Dash,
            lyma::parser::TokenKind::Spread => TokenKind::Spread,
            lyma::parser::TokenKind::Equals => TokenKind::Equals,
            lyma::parser::TokenKind::LeftBracket => TokenKind::LeftBracket,
            lyma::parser::TokenKind::RightBracket => TokenKind::RightBracket,
            lyma::parser::TokenKind::LeftBrace => TokenKind::LeftBrace,
            lyma::parser::TokenKind::RightBrace => TokenKind::RightBrace,
            lyma::parser::TokenKind::KeywordLet => TokenKind::KeywordLet,
            lyma::parser::TokenKind::KeywordAs => TokenKind::KeywordAs,
            lyma::parser::TokenKind::KeywordIn => TokenKind::KeywordIn,
            lyma::parser::TokenKind::LineBreak => TokenKind::LineBreak,
            lyma::parser::TokenKind::EndOfFile => TokenKind::EndOfFile,
            lyma::parser::TokenKind::Error => TokenKind::Error,
        },
        lexeme: token.lexeme.clone(),
        span: map_span(token.span),
    }
}

fn extract_symbols(file: &lyma::parser::LymaFile) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    for (index, document) in file.documents.iter().enumerate() {
        symbols.push(Symbol {
            name: format!("document{}", index + 1),
            kind: SymbolKind::Document,
            span: map_span(document.span),
            selection_span: document
                .separator_span
                .or(Some(document.span))
                .map(map_span)
                .unwrap_or_else(|| map_span(document.span)),
        });

        for item in &document.items {
            collect_document_item_symbols(item, &mut symbols);
        }
    }

    symbols
}

fn collect_document_item_symbols(item: &lyma::parser::DocumentItem, symbols: &mut Vec<Symbol>) {
    match item {
        lyma::parser::DocumentItem::Directive(directive) => {
            symbols.push(Symbol {
                name: directive_name(directive).to_owned(),
                kind: SymbolKind::Directive,
                span: map_span(directive_span(directive)),
                selection_span: map_span(directive_span(directive)),
            });
        }
        lyma::parser::DocumentItem::Let(binding) => {
            symbols.push(Symbol {
                name: binding.name.clone(),
                kind: SymbolKind::Variable,
                span: map_span(binding.span),
                selection_span: map_span(binding.span),
            });
            collect_node_symbols(&binding.value, symbols);
        }
        lyma::parser::DocumentItem::Root(node) => collect_node_symbols(node, symbols),
        lyma::parser::DocumentItem::Comment(_) => {}
    }
}

fn collect_node_symbols(node: &lyma::parser::LymaNode, symbols: &mut Vec<Symbol>) {
    match node {
        lyma::parser::LymaNode::Sequence(sequence) => {
            for item in &sequence.items {
                match item {
                    lyma::parser::SequenceItem::Value(value) => {
                        collect_node_symbols(value, symbols)
                    }
                    lyma::parser::SequenceItem::Directive(directive) => symbols.push(Symbol {
                        name: directive_name(directive).to_owned(),
                        kind: SymbolKind::Directive,
                        span: map_span(directive_span(directive)),
                        selection_span: map_span(directive_span(directive)),
                    }),
                    lyma::parser::SequenceItem::Conditional(block) => {
                        collect_node_symbols(
                            &lyma::parser::LymaNode::Sequence(block.if_branch.body.clone()),
                            symbols,
                        );
                        for branch in &block.else_if_branches {
                            collect_node_symbols(
                                &lyma::parser::LymaNode::Sequence(branch.body.clone()),
                                symbols,
                            );
                        }
                        if let Some(branch) = &block.else_branch {
                            collect_node_symbols(
                                &lyma::parser::LymaNode::Sequence(branch.body.clone()),
                                symbols,
                            );
                        }
                    }
                    lyma::parser::SequenceItem::Loop(block) => {
                        collect_node_symbols(
                            &lyma::parser::LymaNode::Sequence(block.body.clone()),
                            symbols,
                        );
                    }
                    lyma::parser::SequenceItem::Spread(_)
                    | lyma::parser::SequenceItem::Comment(_) => {}
                }
            }
        }
        lyma::parser::LymaNode::Mapping(mapping) => {
            for item in &mapping.items {
                match item {
                    lyma::parser::MappingItem::Entry(entry) => {
                        if let Some(symbol) = mapping_key_symbol(entry) {
                            symbols.push(symbol);
                        }
                        collect_node_symbols(&entry.value, symbols);
                    }
                    lyma::parser::MappingItem::Directive(directive) => symbols.push(Symbol {
                        name: directive_name(directive).to_owned(),
                        kind: SymbolKind::Directive,
                        span: map_span(directive_span(directive)),
                        selection_span: map_span(directive_span(directive)),
                    }),
                    lyma::parser::MappingItem::Let(binding) => {
                        symbols.push(Symbol {
                            name: binding.name.clone(),
                            kind: SymbolKind::Variable,
                            span: map_span(binding.span),
                            selection_span: map_span(binding.span),
                        });
                        collect_node_symbols(&binding.value, symbols);
                    }
                    lyma::parser::MappingItem::Conditional(block) => {
                        collect_node_symbols(
                            &lyma::parser::LymaNode::Mapping(block.if_branch.body.clone()),
                            symbols,
                        );
                        for branch in &block.else_if_branches {
                            collect_node_symbols(
                                &lyma::parser::LymaNode::Mapping(branch.body.clone()),
                                symbols,
                            );
                        }
                        if let Some(branch) = &block.else_branch {
                            collect_node_symbols(
                                &lyma::parser::LymaNode::Mapping(branch.body.clone()),
                                symbols,
                            );
                        }
                    }
                    lyma::parser::MappingItem::Loop(block) => {
                        collect_node_symbols(
                            &lyma::parser::LymaNode::Mapping(block.body.clone()),
                            symbols,
                        );
                    }
                    lyma::parser::MappingItem::Spread(_)
                    | lyma::parser::MappingItem::Comment(_) => {}
                }
            }
        }
        lyma::parser::LymaNode::Tagged(tagged) => {
            symbols.push(Symbol {
                name: tagged.tag.name.value.clone(),
                kind: SymbolKind::Tag,
                span: map_span(tagged.tag.span),
                selection_span: map_span(tagged.tag.span),
            });
            if let Some(value) = &tagged.value {
                collect_node_symbols(value, symbols);
            }
        }
        lyma::parser::LymaNode::Null { .. }
        | lyma::parser::LymaNode::Boolean { .. }
        | lyma::parser::LymaNode::Number(_)
        | lyma::parser::LymaNode::String(_)
        | lyma::parser::LymaNode::LuaExpression(_)
        | lyma::parser::LymaNode::LuaExpressionBlock(_)
        | lyma::parser::LymaNode::LuaChunk(_)
        | lyma::parser::LymaNode::LuaTableConstructor(_) => {}
    }
}

fn mapping_key_symbol(entry: &lyma::parser::MappingEntry) -> Option<Symbol> {
    match &entry.key {
        lyma::parser::MappingKey::Plain { value, span } => Some(Symbol {
            name: value.clone(),
            kind: SymbolKind::MappingKey,
            span: map_span(entry.span),
            selection_span: map_span(*span),
        }),
        lyma::parser::MappingKey::Quoted(string) => Some(Symbol {
            name: string.value.clone(),
            kind: SymbolKind::MappingKey,
            span: map_span(entry.span),
            selection_span: map_span(string.span),
        }),
        lyma::parser::MappingKey::Expression { .. } => None,
    }
}

fn directive_name(directive: &lyma::syntax::Directive) -> &'static str {
    match directive {
        lyma::syntax::Directive::Version(_) => "@lyma",
        lyma::syntax::Directive::Profile(_) => "@profile",
        lyma::syntax::Directive::Schema(_) => "@schema",
        lyma::syntax::Directive::Import(_) => "@import",
        lyma::syntax::Directive::Include(_) => "@include",
        lyma::syntax::Directive::Use(_) => "@use",
        lyma::syntax::Directive::LuaPrelude(_) => "@lua",
        lyma::syntax::Directive::Meta(_) => "@meta",
    }
}

fn directive_span(directive: &lyma::syntax::Directive) -> lyma::parser::Span {
    match directive {
        lyma::syntax::Directive::Version(value) => value.span,
        lyma::syntax::Directive::Profile(value) => value.span,
        lyma::syntax::Directive::Schema(value) => value.span,
        lyma::syntax::Directive::Import(value) => value.span,
        lyma::syntax::Directive::Include(value) => value.span,
        lyma::syntax::Directive::Use(value) => value.span,
        lyma::syntax::Directive::LuaPrelude(value) => value.span,
        lyma::syntax::Directive::Meta(value) => value.span,
    }
}

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
        ParserBackend::UpstreamLuma
    }

    fn parse(&self, file_id: FileId, name: &str, text: &str) -> ParsedDocument {
        let upstream_file_id = luma::parser::FileId(file_id.0);
        let parsed = luma::parser::parse_str(upstream_file_id, name, text);
        let lexed = luma::parser::lex_str(upstream_file_id, name, text);
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
            backend: ParserBackend::UpstreamLuma,
            source,
            file,
            ranges,
            tokens,
            symbols: (!symbols.is_empty()).then_some(symbols),
            diagnostics,
        }
    }

    fn format_document(&self, parsed: &ParsedDocument) -> Option<FormatResult> {
        let formatted = luma::parser::format_str(
            luma::parser::FileId(parsed.source.file_id.0),
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
            luma::tooling::format_document_text_edit(&parsed.source.name, parsed.source.as_str());
        Some(vec![TextEdit {
            range: SourceSpan::new(parsed.source.file_id, edit.range.start, edit.range.end),
            text: edit.text,
        }])
    }
}

fn map_source(source: &luma::parser::SourceText) -> SourceText {
    SourceText::new(
        FileId(source.source.id.0),
        source.source.name.clone(),
        source.source.text.clone(),
    )
}

fn merge_diagnostics(
    parsed: &[luma::parser::Diagnostic],
    lexed: &[luma::parser::Diagnostic],
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

fn map_diagnostic(diagnostic: &luma::parser::Diagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code.code().to_owned(),
        severity: match diagnostic.severity {
            luma::parser::Severity::Info => DiagnosticSeverity::Info,
            luma::parser::Severity::Warning => DiagnosticSeverity::Warning,
            luma::parser::Severity::Error => DiagnosticSeverity::Error,
        },
        source: "luma.parser".to_owned(),
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

fn map_token(token: &luma::parser::Token) -> Token {
    Token {
        kind: match token.kind {
            luma::parser::TokenKind::Identifier => TokenKind::Identifier,
            luma::parser::TokenKind::DirectiveName => TokenKind::DirectiveName,
            luma::parser::TokenKind::TagName => TokenKind::TagName,
            luma::parser::TokenKind::Number => TokenKind::Number,
            luma::parser::TokenKind::String => TokenKind::String,
            luma::parser::TokenKind::PlainString => TokenKind::PlainString,
            luma::parser::TokenKind::Comment => TokenKind::Comment,
            luma::parser::TokenKind::DocumentSeparator => TokenKind::DocumentSeparator,
            luma::parser::TokenKind::DocumentTerminator => TokenKind::DocumentTerminator,
            luma::parser::TokenKind::BlockHeader(_) => TokenKind::BlockHeader,
            luma::parser::TokenKind::Colon => TokenKind::Colon,
            luma::parser::TokenKind::Dash => TokenKind::Dash,
            luma::parser::TokenKind::Spread => TokenKind::Spread,
            luma::parser::TokenKind::Equals => TokenKind::Equals,
            luma::parser::TokenKind::LeftBracket => TokenKind::LeftBracket,
            luma::parser::TokenKind::RightBracket => TokenKind::RightBracket,
            luma::parser::TokenKind::LeftBrace => TokenKind::LeftBrace,
            luma::parser::TokenKind::RightBrace => TokenKind::RightBrace,
            luma::parser::TokenKind::KeywordLet => TokenKind::KeywordLet,
            luma::parser::TokenKind::KeywordAs => TokenKind::KeywordAs,
            luma::parser::TokenKind::KeywordIn => TokenKind::KeywordIn,
            luma::parser::TokenKind::LineBreak => TokenKind::LineBreak,
            luma::parser::TokenKind::EndOfFile => TokenKind::EndOfFile,
            luma::parser::TokenKind::Error => TokenKind::Error,
        },
        lexeme: token.lexeme.clone(),
        span: map_span(token.span),
    }
}

fn extract_symbols(file: &luma::parser::LumaFile) -> Vec<Symbol> {
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

fn collect_document_item_symbols(item: &luma::parser::DocumentItem, symbols: &mut Vec<Symbol>) {
    match item {
        luma::parser::DocumentItem::Directive(directive) => {
            symbols.push(Symbol {
                name: directive_name(directive).to_owned(),
                kind: SymbolKind::Directive,
                span: map_span(directive_span(directive)),
                selection_span: map_span(directive_span(directive)),
            });
        }
        luma::parser::DocumentItem::Let(binding) => {
            symbols.push(Symbol {
                name: binding.name.clone(),
                kind: SymbolKind::Variable,
                span: map_span(binding.span),
                selection_span: map_span(binding.span),
            });
            collect_node_symbols(&binding.value, symbols);
        }
        luma::parser::DocumentItem::Root(node) => collect_node_symbols(node, symbols),
        luma::parser::DocumentItem::Comment(_) => {}
    }
}

fn collect_node_symbols(node: &luma::parser::LumaNode, symbols: &mut Vec<Symbol>) {
    match node {
        luma::parser::LumaNode::Sequence(sequence) => {
            for item in &sequence.items {
                match item {
                    luma::parser::SequenceItem::Value(value) => {
                        collect_node_symbols(value, symbols)
                    }
                    luma::parser::SequenceItem::Directive(directive) => symbols.push(Symbol {
                        name: directive_name(directive).to_owned(),
                        kind: SymbolKind::Directive,
                        span: map_span(directive_span(directive)),
                        selection_span: map_span(directive_span(directive)),
                    }),
                    luma::parser::SequenceItem::Conditional(block) => {
                        collect_node_symbols(
                            &luma::parser::LumaNode::Sequence(block.if_branch.body.clone()),
                            symbols,
                        );
                        for branch in &block.else_if_branches {
                            collect_node_symbols(
                                &luma::parser::LumaNode::Sequence(branch.body.clone()),
                                symbols,
                            );
                        }
                        if let Some(branch) = &block.else_branch {
                            collect_node_symbols(
                                &luma::parser::LumaNode::Sequence(branch.body.clone()),
                                symbols,
                            );
                        }
                    }
                    luma::parser::SequenceItem::Loop(block) => {
                        collect_node_symbols(
                            &luma::parser::LumaNode::Sequence(block.body.clone()),
                            symbols,
                        );
                    }
                    luma::parser::SequenceItem::Spread(_)
                    | luma::parser::SequenceItem::Comment(_) => {}
                }
            }
        }
        luma::parser::LumaNode::Mapping(mapping) => {
            for item in &mapping.items {
                match item {
                    luma::parser::MappingItem::Entry(entry) => {
                        if let Some(symbol) = mapping_key_symbol(entry) {
                            symbols.push(symbol);
                        }
                        collect_node_symbols(&entry.value, symbols);
                    }
                    luma::parser::MappingItem::Directive(directive) => symbols.push(Symbol {
                        name: directive_name(directive).to_owned(),
                        kind: SymbolKind::Directive,
                        span: map_span(directive_span(directive)),
                        selection_span: map_span(directive_span(directive)),
                    }),
                    luma::parser::MappingItem::Let(binding) => {
                        symbols.push(Symbol {
                            name: binding.name.clone(),
                            kind: SymbolKind::Variable,
                            span: map_span(binding.span),
                            selection_span: map_span(binding.span),
                        });
                        collect_node_symbols(&binding.value, symbols);
                    }
                    luma::parser::MappingItem::Conditional(block) => {
                        collect_node_symbols(
                            &luma::parser::LumaNode::Mapping(block.if_branch.body.clone()),
                            symbols,
                        );
                        for branch in &block.else_if_branches {
                            collect_node_symbols(
                                &luma::parser::LumaNode::Mapping(branch.body.clone()),
                                symbols,
                            );
                        }
                        if let Some(branch) = &block.else_branch {
                            collect_node_symbols(
                                &luma::parser::LumaNode::Mapping(branch.body.clone()),
                                symbols,
                            );
                        }
                    }
                    luma::parser::MappingItem::Loop(block) => {
                        collect_node_symbols(
                            &luma::parser::LumaNode::Mapping(block.body.clone()),
                            symbols,
                        );
                    }
                    luma::parser::MappingItem::Spread(_)
                    | luma::parser::MappingItem::Comment(_) => {}
                }
            }
        }
        luma::parser::LumaNode::Tagged(tagged) => {
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
        luma::parser::LumaNode::Null { .. }
        | luma::parser::LumaNode::Boolean { .. }
        | luma::parser::LumaNode::Number(_)
        | luma::parser::LumaNode::String(_)
        | luma::parser::LumaNode::LuaExpression(_)
        | luma::parser::LumaNode::LuaExpressionBlock(_)
        | luma::parser::LumaNode::LuaChunk(_)
        | luma::parser::LumaNode::LuaTableConstructor(_) => {}
    }
}

fn mapping_key_symbol(entry: &luma::parser::MappingEntry) -> Option<Symbol> {
    match &entry.key {
        luma::parser::MappingKey::Plain { value, span } => Some(Symbol {
            name: value.clone(),
            kind: SymbolKind::MappingKey,
            span: map_span(entry.span),
            selection_span: map_span(*span),
        }),
        luma::parser::MappingKey::Quoted(string) => Some(Symbol {
            name: string.value.clone(),
            kind: SymbolKind::MappingKey,
            span: map_span(entry.span),
            selection_span: map_span(string.span),
        }),
        luma::parser::MappingKey::Expression { .. } => None,
    }
}

fn directive_name(directive: &luma::syntax::Directive) -> &'static str {
    match directive {
        luma::syntax::Directive::Version(_) => "@luma",
        luma::syntax::Directive::Profile(_) => "@profile",
        luma::syntax::Directive::Schema(_) => "@schema",
        luma::syntax::Directive::Import(_) => "@import",
        luma::syntax::Directive::Include(_) => "@include",
        luma::syntax::Directive::Use(_) => "@use",
        luma::syntax::Directive::LuaPrelude(_) => "@lua",
        luma::syntax::Directive::Meta(_) => "@meta",
    }
}

fn directive_span(directive: &luma::syntax::Directive) -> luma::parser::Span {
    match directive {
        luma::syntax::Directive::Version(value) => value.span,
        luma::syntax::Directive::Profile(value) => value.span,
        luma::syntax::Directive::Schema(value) => value.span,
        luma::syntax::Directive::Import(value) => value.span,
        luma::syntax::Directive::Include(value) => value.span,
        luma::syntax::Directive::Use(value) => value.span,
        luma::syntax::Directive::LuaPrelude(value) => value.span,
        luma::syntax::Directive::Meta(value) => value.span,
    }
}

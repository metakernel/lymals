#![cfg_attr(feature = "upstream-luma", allow(dead_code))]

use crate::{
    ast::{
        AstFile, Comment, ConditionalBranch, ConditionalNode, Directive, Document, DocumentItem,
        ErrorNode, LetBinding, LoopNode, Mapping, MappingEntry, Node, Scalar, ScalarKind, Sequence,
        SequenceItem, SpreadNode, TagNode,
    },
    diagnostics::{Diagnostic, DiagnosticSeverity},
    formatting, lexer,
    parser::{ParsedDocument, ParserAdapter},
    syntax::{
        FileId, FormatResult, ParsedFile, ParsedRanges, ParserBackend, SourceSpan, SourceText,
        Symbol, SymbolKind,
    },
};

pub(super) static FALLBACK_PARSER: FallbackParser = FallbackParser;

pub(super) struct FallbackParser;

impl ParserAdapter for FallbackParser {
    fn backend(&self) -> ParserBackend {
        ParserBackend::Fallback
    }

    fn parse(&self, file_id: FileId, name: &str, text: &str) -> ParsedDocument {
        parse_with_fallback(file_id, name, text)
    }

    fn format_document(&self, parsed: &ParsedDocument) -> Option<FormatResult> {
        Some(formatting::fallback_format_result(parsed.source.as_str()))
    }
}

pub(crate) fn parse_with_fallback(file_id: FileId, name: &str, text: &str) -> ParsedDocument {
    let source = SourceText::new(file_id, name, normalize_source(text));
    let lexed = lexer::lex(&source);
    let mut parser = Parser::new(&source);
    let ast = parser.parse_file();
    let diagnostics = parser.diagnostics;
    let document_spans: Vec<SourceSpan> =
        ast.documents.iter().map(|document| document.span).collect();
    let file = ParsedFile::fallback(source.full_span(), document_spans.clone(), ast.clone());
    let symbols = extract_symbols(&ast);

    ParsedDocument {
        backend: ParserBackend::Fallback,
        source,
        file,
        ranges: ParsedRanges {
            file: ast.span,
            documents: document_spans,
        },
        tokens: Some(lexed.tokens),
        symbols: (!symbols.is_empty()).then_some(symbols),
        diagnostics,
    }
}

struct Parser<'a> {
    source: &'a SourceText,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a SourceText) -> Self {
        Self {
            source,
            diagnostics: Vec::new(),
        }
    }

    fn parse_file(&mut self) -> AstFile {
        let lines = lines_with_offsets(self.source.as_str());
        let mut documents = Vec::new();
        let mut current = Vec::new();
        let mut separator_span = None;

        for line in &lines {
            let trimmed = line.text.trim();
            if trimmed == "---" {
                if !current.is_empty() || separator_span.is_some() {
                    documents.push(self.parse_document(&current, separator_span));
                    current.clear();
                }
                separator_span = Some(line.span(self.source.file_id));
            } else if trimmed == "..." {
                current.push(line.clone());
                documents.push(self.parse_document(&current, separator_span));
                current.clear();
                separator_span = None;
            } else {
                current.push(line.clone());
            }
        }

        if !current.is_empty() || documents.is_empty() {
            documents.push(self.parse_document(&current, separator_span));
        }

        AstFile {
            span: self.source.full_span(),
            documents,
        }
    }

    fn parse_document(&mut self, lines: &[Line], separator_span: Option<SourceSpan>) -> Document {
        let mut items = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            let line = &lines[index];
            let trimmed = line.text.trim();
            if trimmed.is_empty() || trimmed == "..." {
                index += 1;
                continue;
            }
            if let Some(comment) = self.parse_comment(line) {
                items.push(DocumentItem::Comment(comment));
                index += 1;
                continue;
            }
            if let Some(directive) = self.parse_directive(line) {
                items.push(DocumentItem::Directive(directive));
                index += 1;
                continue;
            }
            if let Some(binding) = self.parse_let(line) {
                items.push(DocumentItem::Let(binding));
                index += 1;
                continue;
            }

            let (node, consumed) = self.parse_node(lines, index, line.indent);
            items.push(DocumentItem::Node(node));
            index += consumed.max(1);
        }

        let span = if let (Some(first), Some(last)) = (lines.first(), lines.last()) {
            SourceSpan::new(self.source.file_id, first.start, last.end)
        } else {
            separator_span.unwrap_or(self.source.full_span())
        };

        Document {
            span,
            separator_span,
            items,
        }
    }

    fn parse_comment(&self, line: &Line) -> Option<Comment> {
        let trimmed = line.text.trim_start();
        trimmed.strip_prefix('#').map(|rest| Comment {
            span: line.span(self.source.file_id),
            text: rest.trim_start().to_owned(),
        })
    }

    fn parse_directive(&self, line: &Line) -> Option<Directive> {
        let trimmed = line.text.trim_start();
        let name = trimmed.split_whitespace().next()?;
        if !name.starts_with('@') {
            return None;
        }
        Some(Directive {
            name: name.to_owned(),
            span: line.span(self.source.file_id),
            value: trimmed[name.len()..]
                .trim()
                .is_empty()
                .not()
                .then(|| trimmed[name.len()..].trim().to_owned()),
        })
    }

    fn parse_let(&mut self, line: &Line) -> Option<LetBinding> {
        let trimmed = line.text.trim_start();
        if !trimmed.starts_with("let ") {
            return None;
        }
        let rest = &trimmed[4..];
        let Some((name, _value)) = rest.split_once('=') else {
            self.error(
                "F001",
                "expected '=' in let binding",
                line.span(self.source.file_id),
            );
            return Some(LetBinding {
                name: rest.trim().to_owned(),
                span: line.span(self.source.file_id),
                value_span: None,
            });
        };
        let value_start = line.start + line.text.find('=').unwrap_or(0) + 1;
        Some(LetBinding {
            name: name.trim().to_owned(),
            span: line.span(self.source.file_id),
            value_span: Some(SourceSpan::new(self.source.file_id, value_start, line.end)),
        })
    }

    fn parse_node(&mut self, lines: &[Line], index: usize, base_indent: usize) -> (Node, usize) {
        let line = &lines[index];
        let trimmed = line.text.trim();

        if trimmed.starts_with("...") && trimmed != "..." {
            return (
                Node::Spread(SpreadNode {
                    span: line.span(self.source.file_id),
                    target: trimmed.trim_start_matches("...").trim().to_owned(),
                }),
                1,
            );
        }
        if trimmed.starts_with("?if ") || trimmed.starts_with("?elif ") || trimmed == "?else" {
            return (self.parse_conditional(line), 1);
        }
        if trimmed.starts_with("*for ") {
            return (
                Node::Loop(LoopNode {
                    span: line.span(self.source.file_id),
                    header: trimmed[1..].trim().to_owned(),
                }),
                1,
            );
        }
        if let Some(rest) = trimmed.strip_prefix('!') {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let name = parts.next().unwrap_or_default().to_owned();
            let value = parts
                .next()
                .map(|value| Box::new(self.inline_scalar(line, value.trim())));
            return (
                Node::Tag(TagNode {
                    name,
                    span: line.span(self.source.file_id),
                    value,
                }),
                1,
            );
        }
        if trimmed.starts_with("-") {
            return self.parse_sequence(lines, index, base_indent);
        }
        if trimmed.contains(':') {
            return self.parse_mapping(lines, index, base_indent);
        }
        (self.inline_scalar(line, trimmed), 1)
    }

    fn parse_sequence(
        &mut self,
        lines: &[Line],
        index: usize,
        base_indent: usize,
    ) -> (Node, usize) {
        let mut items = Vec::new();
        let mut end = index;
        while end < lines.len() {
            let line = &lines[end];
            if line.text.trim().is_empty() {
                end += 1;
                continue;
            }
            if line.indent != base_indent || !line.text.trim_start().starts_with('-') {
                break;
            }
            let trimmed = line.text.trim_start()[1..].trim();
            let value = (!trimmed.is_empty()).then(|| Box::new(self.inline_scalar(line, trimmed)));
            items.push(SequenceItem {
                span: line.span(self.source.file_id),
                value,
            });
            end += 1;
        }
        (
            Node::Sequence(Sequence {
                span: SourceSpan::new(self.source.file_id, lines[index].start, lines[end - 1].end),
                items,
            }),
            end - index,
        )
    }

    fn parse_mapping(&mut self, lines: &[Line], index: usize, base_indent: usize) -> (Node, usize) {
        let mut entries = Vec::new();
        let mut end = index;
        while end < lines.len() {
            let line = &lines[end];
            let trimmed = line.text.trim();
            if trimmed.is_empty() {
                end += 1;
                continue;
            }
            if line.indent != base_indent || !trimmed.contains(':') || trimmed.starts_with('-') {
                break;
            }
            let (key, value) = match trimmed.split_once(':') {
                Some(parts) => parts,
                None => break,
            };
            let key_start = line.start + line.text.find(key).unwrap_or(0);
            let key_span = SourceSpan::new(self.source.file_id, key_start, key_start + key.len());
            let value = value.trim();
            let (value_node, metadata) = if value.is_empty() {
                (None, None)
            } else if value == "|"
                || value == ">"
                || value.starts_with("|")
                || value.starts_with('>')
            {
                let (block, consumed) =
                    self.parse_block_string(lines, end, value.chars().next().unwrap_or('|'));
                end += consumed - 1;
                (Some(Box::new(block)), None)
            } else {
                let meta = value.strip_prefix('@').map(str::to_owned);
                (Some(Box::new(self.inline_scalar(line, value))), meta)
            };
            entries.push(MappingEntry {
                key: key.trim().to_owned(),
                key_span,
                span: line.span(self.source.file_id),
                value: value_node,
                metadata,
            });
            end += 1;
        }

        if entries.is_empty() {
            self.error(
                "F002",
                "expected mapping entry",
                lines[index].span(self.source.file_id),
            );
            return (
                Node::Error(ErrorNode {
                    span: lines[index].span(self.source.file_id),
                    message: "expected mapping entry".to_owned(),
                }),
                1,
            );
        }

        (
            Node::Mapping(Mapping {
                span: SourceSpan::new(self.source.file_id, lines[index].start, lines[end - 1].end),
                entries,
            }),
            end - index,
        )
    }

    fn parse_block_string(
        &mut self,
        lines: &[Line],
        header_index: usize,
        marker: char,
    ) -> (Node, usize) {
        let header = &lines[header_index];
        let expected_indent = header.indent + 2;
        let mut end = header_index + 1;
        while end < lines.len() {
            let line = &lines[end];
            if line.text.trim().is_empty() {
                end += 1;
                continue;
            }
            if line.indent < expected_indent {
                break;
            }
            end += 1;
        }
        if end == header_index + 1 {
            self.error(
                "F003",
                "expected indented block string content",
                header.span(self.source.file_id),
            );
        }
        let span = SourceSpan::new(
            self.source.file_id,
            header.start,
            lines[end.saturating_sub(1)].end,
        );
        let text = self.source.slice(span).unwrap_or_default().to_owned();
        (
            Node::Scalar(Scalar {
                kind: if marker == '|' {
                    ScalarKind::BlockString
                } else {
                    ScalarKind::BlockString
                },
                span,
                text,
            }),
            end - header_index,
        )
    }

    fn parse_conditional(&mut self, line: &Line) -> Node {
        let label = if line.text.trim() == "?else" {
            "else"
        } else if line.text.trim().starts_with("?elif ") {
            "elif"
        } else {
            "if"
        };
        Node::Conditional(ConditionalNode {
            span: line.span(self.source.file_id),
            condition: line.text.trim()[1..].trim().to_owned(),
            branches: vec![ConditionalBranch {
                label: label.to_owned(),
                span: line.span(self.source.file_id),
            }],
        })
    }

    fn inline_scalar(&mut self, line: &Line, value: &str) -> Node {
        let kind = if value.starts_with("${") || value.starts_with('=') {
            ScalarKind::LuaExpression
        } else if value.starts_with("lua{") || value.starts_with("```lua") {
            ScalarKind::LuaBlock
        } else if value.starts_with('"') || value.starts_with('\'') {
            ScalarKind::String
        } else if value.parse::<f64>().is_ok() {
            ScalarKind::Number
        } else {
            ScalarKind::Plain
        };

        let Some(offset) = line.text.find(value) else {
            self.error(
                "F004",
                "could not locate scalar text",
                line.span(self.source.file_id),
            );
            return Node::Error(ErrorNode {
                span: line.span(self.source.file_id),
                message: "could not locate scalar text".to_owned(),
            });
        };
        let span = SourceSpan::new(
            self.source.file_id,
            line.start + offset,
            line.start + offset + value.len(),
        );
        Node::Scalar(Scalar {
            kind,
            span,
            text: value.to_owned(),
        })
    }

    fn error(&mut self, code: &str, message: &str, span: SourceSpan) {
        let mut diagnostic =
            Diagnostic::new(code, DiagnosticSeverity::Error, message).with_source("lumals.parser");
        diagnostic.primary_span = Some(span);
        self.diagnostics.push(diagnostic);
    }
}

fn extract_symbols(file: &AstFile) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for (index, document) in file.documents.iter().enumerate() {
        symbols.push(Symbol {
            name: format!("document{}", index + 1),
            kind: SymbolKind::Document,
            span: document.span,
            selection_span: document.separator_span.unwrap_or(document.span),
        });
        for item in &document.items {
            match item {
                DocumentItem::Directive(directive) => symbols.push(Symbol {
                    name: directive.name.clone(),
                    kind: SymbolKind::Directive,
                    span: directive.span,
                    selection_span: directive.span,
                }),
                DocumentItem::Let(binding) => symbols.push(Symbol {
                    name: binding.name.clone(),
                    kind: SymbolKind::Variable,
                    span: binding.span,
                    selection_span: binding.span,
                }),
                DocumentItem::Node(node) => collect_node_symbols(node, &mut symbols),
                DocumentItem::Comment(_) => {}
            }
        }
    }
    symbols
}

fn collect_node_symbols(node: &Node, symbols: &mut Vec<Symbol>) {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                symbols.push(Symbol {
                    name: entry.key.clone(),
                    kind: SymbolKind::MappingKey,
                    span: entry.span,
                    selection_span: entry.key_span,
                });
                if let Some(value) = &entry.value {
                    collect_node_symbols(value, symbols);
                }
            }
        }
        Node::Sequence(sequence) => {
            for item in &sequence.items {
                if let Some(value) = &item.value {
                    collect_node_symbols(value, symbols);
                }
            }
        }
        Node::Tag(tag) => {
            symbols.push(Symbol {
                name: tag.name.clone(),
                kind: SymbolKind::Tag,
                span: tag.span,
                selection_span: tag.span,
            });
            if let Some(value) = &tag.value {
                collect_node_symbols(value, symbols);
            }
        }
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => {}
    }
}

#[derive(Debug, Clone)]
struct Line {
    start: usize,
    end: usize,
    indent: usize,
    text: String,
}

impl Line {
    fn span(&self, file_id: FileId) -> SourceSpan {
        SourceSpan::new(file_id, self.start, self.end)
    }
}

fn lines_with_offsets(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    for segment in text.split_inclusive('\n') {
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        let end = start + body.len();
        lines.push(Line {
            start,
            end,
            indent: body.chars().take_while(|ch| *ch == ' ').count(),
            text: body.to_owned(),
        });
        start += segment.len();
    }
    if text.is_empty()
        || text.ends_with('\n').not() && lines.last().is_none_or(|line| line.end != text.len())
    {
        let body = &text[start..];
        lines.push(Line {
            start,
            end: text.len(),
            indent: body.chars().take_while(|ch| *ch == ' ').count(),
            text: body.to_owned(),
        });
    }
    lines
}

fn normalize_source(text: &str) -> String {
    text.replace("\r\n", "\n")
}

trait BoolExt {
    fn not(self) -> bool;
}

impl BoolExt for bool {
    fn not(self) -> bool {
        !self
    }
}

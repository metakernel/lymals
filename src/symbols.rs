use std::fmt;

use tower_lsp::lsp_types::SymbolKind as LspSymbolKind;

use crate::ast::{
    AstFile, Directive, Document, DocumentItem, LetBinding, Mapping, MappingEntry, Node, Sequence,
    TagNode,
};
use crate::syntax::{SourceSpan, Symbol, SymbolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DefinitionKind {
    DocumentRoot,
    Directive,
    Import,
    Alias,
    Let,
    Key,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    Document(usize),
    Key(String),
    Index(usize),
    Directive(String),
    Import(String),
    Alias(String),
    Let(String),
    Tag(String),
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(index) => write!(f, "document[{index}]"),
            Self::Key(key) => f.write_str(key),
            Self::Index(index) => write!(f, "[{index}]"),
            Self::Directive(name) => f.write_str(name),
            Self::Import(target) => write!(f, "@import({target})"),
            Self::Alias(name) => write!(f, "alias({name})"),
            Self::Let(name) => write!(f, "${name}"),
            Self::Tag(name) => write!(f, "!{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SymbolPath {
    pub segments: Vec<PathSegment>,
}

impl SymbolPath {
    #[must_use]
    pub fn root(document_index: usize) -> Self {
        Self {
            segments: vec![PathSegment::Document(document_index)],
        }
    }

    #[must_use]
    pub fn child(&self, segment: PathSegment) -> Self {
        let mut segments = self.segments.clone();
        segments.push(segment);
        Self { segments }
    }
}

impl fmt::Display for SymbolPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.segments.iter().enumerate() {
            match segment {
                PathSegment::Index(_) => write!(f, "{segment}")?,
                _ if index == 0 => write!(f, "{segment}")?,
                _ => write!(f, ".{segment}")?,
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Definition {
    pub name: String,
    pub kind: DefinitionKind,
    pub span: SourceSpan,
    pub selection_span: SourceSpan,
    pub path: SymbolPath,
    pub detail: Option<String>,
}

impl Definition {
    #[must_use]
    pub fn as_symbol(&self) -> Option<Symbol> {
        let kind = match self.kind {
            DefinitionKind::DocumentRoot => SymbolKind::Document,
            DefinitionKind::Directive | DefinitionKind::Import => SymbolKind::Directive,
            DefinitionKind::Alias | DefinitionKind::Let => SymbolKind::Variable,
            DefinitionKind::Key => SymbolKind::MappingKey,
            DefinitionKind::Tag => SymbolKind::Tag,
        };

        Some(Symbol {
            name: self.name.clone(),
            kind,
            span: self.span,
            selection_span: self.selection_span,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbolNode {
    pub name: String,
    pub detail: Option<String>,
    pub kind: LspSymbolKind,
    pub span: SourceSpan,
    pub selection_span: SourceSpan,
    pub children: Vec<DocumentSymbolNode>,
}

#[must_use]
pub fn build_document_symbols(
    file: &AstFile,
    source: &crate::syntax::SourceText,
) -> Vec<DocumentSymbolNode> {
    file.documents
        .iter()
        .enumerate()
        .map(|(index, document)| build_document_symbol(index + 1, document, source))
        .collect()
}

fn build_document_symbol(
    index: usize,
    document: &Document,
    source: &crate::syntax::SourceText,
) -> DocumentSymbolNode {
    let mut children = Vec::new();

    for item in &document.items {
        match item {
            DocumentItem::Directive(directive) => {
                children.push(build_directive_symbol(directive, source));
            }
            DocumentItem::Let(binding) => {
                children.push(DocumentSymbolNode {
                    name: binding.name.clone(),
                    detail: None,
                    kind: LspSymbolKind::VARIABLE,
                    span: binding.span,
                    selection_span: let_name_span(binding, source).unwrap_or(binding.span),
                    children: Vec::new(),
                });
            }
            DocumentItem::Node(node) => children.extend(build_node_symbols(node, source)),
            DocumentItem::Comment(_) => {}
        }
    }

    let selection_span = document.separator_span.unwrap_or(document.span);
    let span = match document.separator_span {
        Some(separator_span) => span_union(document.span, separator_span),
        None => document.span,
    };

    DocumentSymbolNode {
        name: format!("document{index}"),
        detail: None,
        kind: LspSymbolKind::MODULE,
        span,
        selection_span,
        children,
    }
}

/// Returns the smallest span that contains both `a` and `b`.
///
/// Used to guarantee that a `DocumentSymbol`'s outer `span` (LSP `range`)
/// always contains its `selection_span` (LSP `selectionRange`), which the
/// LSP spec requires and which `vscode-languageclient` validates client-side.
/// The fallback parser's document `span` covers only the document's content
/// lines, while `separator_span` covers the preceding `---`/`...` marker
/// line, so the two can be disjoint without this union.
fn span_union(a: SourceSpan, b: SourceSpan) -> SourceSpan {
    SourceSpan::new(a.file_id, a.start.min(b.start), a.end.max(b.end))
}

fn build_directive_symbol(
    directive: &Directive,
    source: &crate::syntax::SourceText,
) -> DocumentSymbolNode {
    let mut children = Vec::new();

    if let Some(value) = directive.value.as_deref()
        && matches!(directive.name.as_str(), "@import" | "@include" | "@use")
    {
        let (target, alias) = split_directive_value(value);

        if let Some(target) = target {
            children.push(DocumentSymbolNode {
                name: target,
                detail: Some(directive.name.clone()),
                kind: LspSymbolKind::FILE,
                span: directive.span,
                selection_span: directive_target_span(directive, source).unwrap_or(directive.span),
                children: Vec::new(),
            });
        }

        if let Some(alias) = alias {
            children.push(DocumentSymbolNode {
                name: alias,
                detail: Some(directive.name.clone()),
                kind: LspSymbolKind::VARIABLE,
                span: directive.span,
                selection_span: directive_alias_span(directive, source).unwrap_or(directive.span),
                children: Vec::new(),
            });
        }
    }

    DocumentSymbolNode {
        name: directive.name.clone(),
        detail: directive.value.clone(),
        kind: LspSymbolKind::NAMESPACE,
        span: directive.span,
        selection_span: directive_name_span(directive, source).unwrap_or(directive.span),
        children,
    }
}

fn build_node_symbols(node: &Node, source: &crate::syntax::SourceText) -> Vec<DocumentSymbolNode> {
    match node {
        Node::Mapping(mapping) => build_mapping_symbols(mapping, source),
        Node::Sequence(sequence) => build_sequence_symbols(sequence, source),
        Node::Tag(tag) => vec![build_tag_symbol(tag, source)],
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => Vec::new(),
    }
}

fn build_mapping_symbols(
    mapping: &Mapping,
    source: &crate::syntax::SourceText,
) -> Vec<DocumentSymbolNode> {
    mapping
        .entries
        .iter()
        .map(|entry| build_mapping_entry_symbol(entry, source))
        .collect()
}

fn build_mapping_entry_symbol(
    entry: &MappingEntry,
    source: &crate::syntax::SourceText,
) -> DocumentSymbolNode {
    let mut children = Vec::new();

    if let Some(metadata) = &entry.metadata {
        let selection_span = entry_metadata_span(entry, source).unwrap_or(entry.span);
        children.push(DocumentSymbolNode {
            name: format!("@{metadata}"),
            detail: Some("metadata".to_string()),
            kind: LspSymbolKind::CLASS,
            span: entry.span,
            selection_span,
            children: Vec::new(),
        });
    }

    if let Some(value) = &entry.value {
        children.extend(build_node_symbols(value, source));
    }

    DocumentSymbolNode {
        name: entry.key.clone(),
        detail: None,
        kind: LspSymbolKind::KEY,
        span: entry.span,
        selection_span: entry.key_span,
        children,
    }
}

fn build_sequence_symbols(
    sequence: &Sequence,
    source: &crate::syntax::SourceText,
) -> Vec<DocumentSymbolNode> {
    sequence
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| DocumentSymbolNode {
            name: format!("[{index}]"),
            detail: None,
            kind: LspSymbolKind::ARRAY,
            span: item.span,
            selection_span: item.span,
            children: item
                .value
                .as_deref()
                .map(|value| build_node_symbols(value, source))
                .unwrap_or_default(),
        })
        .collect()
}

fn build_tag_symbol(tag: &TagNode, source: &crate::syntax::SourceText) -> DocumentSymbolNode {
    let mut children = Vec::new();
    if let Some(value) = &tag.value {
        children.extend(build_node_symbols(value, source));
    }

    DocumentSymbolNode {
        name: format!("!{}", tag.name),
        detail: None,
        kind: LspSymbolKind::OPERATOR,
        span: tag.span,
        selection_span: tag_name_span(tag, source).unwrap_or(tag.span),
        children,
    }
}

fn directive_name_span(
    directive: &Directive,
    source: &crate::syntax::SourceText,
) -> Option<SourceSpan> {
    let text = source.slice(directive.span)?;
    let local_start = text.find(&directive.name)?;
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + local_start,
        directive.span.start + local_start + directive.name.len(),
    ))
}

fn let_name_span(binding: &LetBinding, source: &crate::syntax::SourceText) -> Option<SourceSpan> {
    let text = source.slice(binding.span)?;
    let local_start = text.find(&binding.name)?;
    Some(SourceSpan::new(
        binding.span.file_id,
        binding.span.start + local_start,
        binding.span.start + local_start + binding.name.len(),
    ))
}

fn directive_target_span(
    directive: &Directive,
    source: &crate::syntax::SourceText,
) -> Option<SourceSpan> {
    let text = source.slice(directive.span)?;
    let value_start = text.find(&directive.name)? + directive.name.len();
    let value = text.get(value_start..)?.trim_start();
    let trimmed_offset = value_start + text.get(value_start..)?.len().saturating_sub(value.len());

    if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(SourceSpan::new(
            directive.span.file_id,
            directive.span.start + trimmed_offset + 1,
            directive.span.start + trimmed_offset + 1 + end,
        ));
    }

    if let Some(rest) = value.strip_prefix('\'') {
        let end = rest.find('\'')?;
        return Some(SourceSpan::new(
            directive.span.file_id,
            directive.span.start + trimmed_offset + 1,
            directive.span.start + trimmed_offset + 1 + end,
        ));
    }

    let local_end = value.find(" as ").unwrap_or(value.len());
    let target = value.get(..local_end)?.trim_end();
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + trimmed_offset,
        directive.span.start + trimmed_offset + target.len(),
    ))
}

fn directive_alias_span(
    directive: &Directive,
    source: &crate::syntax::SourceText,
) -> Option<SourceSpan> {
    let text = source.slice(directive.span)?;
    let local_start = text.rfind(" as ")? + 4;
    let alias = text.get(local_start..)?.trim();
    let trimmed_prefix = text.get(local_start..)?.len().saturating_sub(alias.len());
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + local_start + trimmed_prefix,
        directive.span.start + local_start + trimmed_prefix + alias.len(),
    ))
}

fn tag_name_span(tag: &TagNode, source: &crate::syntax::SourceText) -> Option<SourceSpan> {
    let text = source.slice(tag.span)?;
    let bang = text.find('!')?;
    let name_start = bang + 1;
    let name_len = text[name_start..]
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        .count();
    (name_len > 0).then_some(SourceSpan::new(
        tag.span.file_id,
        tag.span.start + name_start,
        tag.span.start + name_start + name_len,
    ))
}

fn entry_metadata_span(
    entry: &MappingEntry,
    source: &crate::syntax::SourceText,
) -> Option<SourceSpan> {
    let metadata = entry.metadata.as_ref()?;
    let text = source.slice(entry.span)?;
    let needle = format!("@{metadata}");
    let local_start = text.find(&needle)? + 1;
    Some(SourceSpan::new(
        entry.span.file_id,
        entry.span.start + local_start,
        entry.span.start + local_start + metadata.len(),
    ))
}

fn split_directive_value(value: &str) -> (Option<String>, Option<String>) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    if let Some((target, alias)) = trimmed.rsplit_once(" as ") {
        return (
            Some(strip_quotes(target.trim()).to_owned()),
            Some(alias.trim().to_owned()),
        );
    }

    (Some(strip_quotes(trimmed).to_owned()), None)
}

fn strip_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && *bytes.last().unwrap_or(&0) == b'\'')
            || (bytes[0] == b'"' && *bytes.last().unwrap_or(&0) == b'"'))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_fallback;
    use crate::syntax::{FileId, ParsedFile};

    /// Regression test for a real bug caught by end-to-end VS Code extension
    /// testing: `vscode-languageclient` validates that every `DocumentSymbol`'s
    /// `range` contains its `selectionRange`. The fallback parser's
    /// `Document::span` covers only the document's content lines, while
    /// `Document::separator_span` covers the preceding `---`/`...` marker line
    /// that precedes it, so a document whose content begins immediately after
    /// its separator previously produced a root document symbol whose `span`
    /// excluded the separator line even though `selection_span` pointed at it,
    /// violating the LSP spec and throwing
    /// "selectionRange must be contained in fullRange" in real clients.
    #[test]
    fn document_root_symbol_span_always_contains_its_selection_span() {
        let text = "# comment\n---\nmessage: hello\n";
        let parsed = parse_fallback(FileId(1), "fixture.lyma", text);

        #[cfg(feature = "upstream-lyma")]
        let ParsedFile::Fallback(file) = &parsed.file else {
            panic!("parse_fallback must return ParsedFile::Fallback");
        };
        #[cfg(not(feature = "upstream-lyma"))]
        let ParsedFile::Fallback(file) = &parsed.file;

        let symbols = build_document_symbols(&file.ast, &parsed.source);
        assert!(!symbols.is_empty(), "expected at least one document symbol");

        for symbol in &symbols {
            assert_span_containment_recursively(symbol);
        }
    }

    fn assert_span_containment_recursively(symbol: &DocumentSymbolNode) {
        assert!(
            span_contains(symbol.span, symbol.selection_span),
            "symbol {:?} span {:?} does not contain selection_span {:?}",
            symbol.name,
            symbol.span,
            symbol.selection_span,
        );

        for child in &symbol.children {
            assert_span_containment_recursively(child);
        }
    }

    fn span_contains(outer: SourceSpan, inner: SourceSpan) -> bool {
        outer.file_id == inner.file_id && outer.start <= inner.start && inner.end <= outer.end
    }
}

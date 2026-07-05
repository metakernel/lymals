use crate::{
    ast::{AstFile, DocumentItem, Mapping, Node, Sequence},
    symbols::{Definition, DefinitionKind, PathSegment, SymbolPath},
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticDocument {
    pub definitions: Vec<Definition>,
}

impl SemanticDocument {
    #[must_use]
    pub fn from_ast(file: &AstFile) -> Self {
        let mut definitions = Vec::new();

        for (doc_offset, document) in file.documents.iter().enumerate() {
            let document_index = doc_offset + 1;
            let document_path = SymbolPath::root(document_index);

            definitions.push(Definition {
                name: format!("document{document_index}"),
                kind: DefinitionKind::DocumentRoot,
                span: document.span,
                selection_span: document.separator_span.unwrap_or(document.span),
                path: document_path.clone(),
                detail: None,
            });

            for item in &document.items {
                match item {
                    DocumentItem::Directive(directive) => {
                        let directive_path =
                            document_path.child(PathSegment::Directive(directive.name.clone()));
                        definitions.push(Definition {
                            name: directive.name.clone(),
                            kind: DefinitionKind::Directive,
                            span: directive.span,
                            selection_span: directive.span,
                            path: directive_path,
                            detail: directive.value.clone(),
                        });

                        if let Some(value) = &directive.value {
                            if matches!(directive.name.as_str(), "@import" | "@include" | "@use") {
                                let (target, alias) = split_directive_value(value);
                                if let Some(target) = target.clone() {
                                    definitions.push(Definition {
                                        name: target.clone(),
                                        kind: DefinitionKind::Import,
                                        span: directive.span,
                                        selection_span: directive.span,
                                        path: document_path
                                            .child(PathSegment::Import(target.clone())),
                                        detail: Some(directive.name.clone()),
                                    });
                                }
                                if let Some(alias) = alias {
                                    definitions.push(Definition {
                                        name: alias.clone(),
                                        kind: DefinitionKind::Alias,
                                        span: directive.span,
                                        selection_span: directive.span,
                                        path: document_path.child(PathSegment::Alias(alias)),
                                        detail: target,
                                    });
                                }
                            }
                        }
                    }
                    DocumentItem::Let(binding) => definitions.push(Definition {
                        name: binding.name.clone(),
                        kind: DefinitionKind::Let,
                        span: binding.span,
                        selection_span: binding.span,
                        path: document_path.child(PathSegment::Let(binding.name.clone())),
                        detail: binding
                            .value_span
                            .map(|span| format!("{}..{}", span.start, span.end)),
                    }),
                    DocumentItem::Node(node) => {
                        collect_node_definitions(node, &document_path, &mut definitions)
                    }
                    DocumentItem::Comment(_) => {}
                }
            }
        }

        Self { definitions }
    }
}

fn collect_node_definitions(node: &Node, path: &SymbolPath, definitions: &mut Vec<Definition>) {
    match node {
        Node::Mapping(mapping) => collect_mapping_definitions(mapping, path, definitions),
        Node::Sequence(sequence) => collect_sequence_definitions(sequence, path, definitions),
        Node::Tag(tag) => {
            let tag_path = path.child(PathSegment::Tag(tag.name.clone()));
            definitions.push(Definition {
                name: tag.name.clone(),
                kind: DefinitionKind::Tag,
                span: tag.span,
                selection_span: tag.span,
                path: tag_path,
                detail: None,
            });
            if let Some(value) = &tag.value {
                collect_node_definitions(value, path, definitions);
            }
        }
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => {}
    }
}

fn collect_mapping_definitions(
    mapping: &Mapping,
    path: &SymbolPath,
    definitions: &mut Vec<Definition>,
) {
    for entry in &mapping.entries {
        let entry_path = path.child(PathSegment::Key(entry.key.clone()));
        definitions.push(Definition {
            name: entry.key.clone(),
            kind: DefinitionKind::Key,
            span: entry.span,
            selection_span: entry.key_span,
            path: entry_path.clone(),
            detail: entry.metadata.clone(),
        });

        if let Some(value) = &entry.value {
            collect_node_definitions(value, &entry_path, definitions);
        }
    }
}

fn collect_sequence_definitions(
    sequence: &Sequence,
    path: &SymbolPath,
    definitions: &mut Vec<Definition>,
) {
    for (index, item) in sequence.items.iter().enumerate() {
        let item_path = path.child(PathSegment::Index(index));
        if let Some(value) = &item.value {
            collect_node_definitions(value, &item_path, definitions);
        }
    }
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
    use crate::{
        ast::{AstFile, Directive, Document, DocumentItem, LetBinding},
        syntax::{FileId, SourceSpan},
    };

    use super::SemanticDocument;

    #[test]
    fn extracts_import_alias_and_let_definitions() {
        let file_id = FileId(0);
        let file = AstFile {
            span: SourceSpan::new(file_id, 0, 10),
            documents: vec![Document {
                span: SourceSpan::new(file_id, 0, 10),
                separator_span: None,
                items: vec![
                    DocumentItem::Directive(Directive {
                        name: "@import".to_string(),
                        span: SourceSpan::new(file_id, 0, 10),
                        value: Some("\"shared.luma\" as shared".to_string()),
                    }),
                    DocumentItem::Let(LetBinding {
                        name: "env".to_string(),
                        span: SourceSpan::new(file_id, 11, 20),
                        value_span: None,
                    }),
                ],
            }],
        };

        let semantic = SemanticDocument::from_ast(&file);
        let names = semantic
            .definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>();

        assert!(names.contains(&"shared.luma"));
        assert!(names.contains(&"shared"));
        assert!(names.contains(&"env"));
    }
}

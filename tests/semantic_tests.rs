use std::{fs, path::Path};

use lymals::{
    ast::{
        AstFile, Directive, Document, DocumentItem, LetBinding, Mapping, MappingEntry, Node,
        Scalar, ScalarKind, Sequence, SequenceItem, TagNode,
    },
    index::{DocumentIndex, WorkspaceIndex},
    semantic::SemanticDocument,
    syntax::{FileId, SourceSpan},
};
use pretty_assertions::assert_eq;
use tower_lsp::lsp_types::Url;

#[test]
fn semantic_index_matches_golden_fixture() {
    let file_id = FileId(7);
    let ast = AstFile {
        span: span(file_id, 0, 400),
        documents: vec![Document {
            span: span(file_id, 0, 400),
            separator_span: Some(span(file_id, 0, 3)),
            items: vec![
                DocumentItem::Directive(Directive {
                    name: "@lyma".to_string(),
                    span: span(file_id, 0, 10),
                    value: Some("1".to_string()),
                }),
                DocumentItem::Directive(Directive {
                    name: "@import".to_string(),
                    span: span(file_id, 11, 40),
                    value: Some("\"./shared.lyma\" as shared".to_string()),
                }),
                DocumentItem::Directive(Directive {
                    name: "@include".to_string(),
                    span: span(file_id, 41, 70),
                    value: Some("partials/base.lyma".to_string()),
                }),
                DocumentItem::Let(LetBinding {
                    name: "region".to_string(),
                    span: span(file_id, 71, 95),
                    value_span: Some(span(file_id, 83, 95)),
                }),
                DocumentItem::Node(Node::Mapping(Mapping {
                    span: span(file_id, 96, 399),
                    entries: vec![
                        MappingEntry {
                            key: "services".to_string(),
                            key_span: span(file_id, 96, 104),
                            span: span(file_id, 96, 220),
                            metadata: None,
                            value: Some(Box::new(Node::Sequence(Sequence {
                                span: span(file_id, 105, 220),
                                items: vec![
                                    SequenceItem {
                                        span: span(file_id, 110, 160),
                                        value: Some(Box::new(Node::Mapping(Mapping {
                                            span: span(file_id, 110, 160),
                                            entries: vec![
                                                MappingEntry {
                                                    key: "name".to_string(),
                                                    key_span: span(file_id, 110, 114),
                                                    span: span(file_id, 110, 128),
                                                    metadata: None,
                                                    value: Some(Box::new(Node::Scalar(Scalar {
                                                        kind: ScalarKind::String,
                                                        span: span(file_id, 116, 127),
                                                        text: "api".to_string(),
                                                    }))),
                                                },
                                                MappingEntry {
                                                    key: "handler".to_string(),
                                                    key_span: span(file_id, 129, 136),
                                                    span: span(file_id, 129, 160),
                                                    metadata: None,
                                                    value: Some(Box::new(Node::Tag(TagNode {
                                                        name: "lambda".to_string(),
                                                        span: span(file_id, 138, 160),
                                                        value: Some(Box::new(Node::Scalar(
                                                            Scalar {
                                                                kind: ScalarKind::Plain,
                                                                span: span(file_id, 146, 160),
                                                                text: "worker".to_string(),
                                                            },
                                                        ))),
                                                    }))),
                                                },
                                            ],
                                        }))),
                                    },
                                    SequenceItem {
                                        span: span(file_id, 161, 220),
                                        value: Some(Box::new(Node::Mapping(Mapping {
                                            span: span(file_id, 161, 220),
                                            entries: vec![MappingEntry {
                                                key: "name".to_string(),
                                                key_span: span(file_id, 161, 165),
                                                span: span(file_id, 161, 180),
                                                metadata: None,
                                                value: Some(Box::new(Node::Scalar(Scalar {
                                                    kind: ScalarKind::String,
                                                    span: span(file_id, 167, 179),
                                                    text: "worker".to_string(),
                                                }))),
                                            }],
                                        }))),
                                    },
                                ],
                            }))),
                        },
                        MappingEntry {
                            key: "config".to_string(),
                            key_span: span(file_id, 221, 227),
                            span: span(file_id, 221, 399),
                            metadata: None,
                            value: Some(Box::new(Node::Mapping(Mapping {
                                span: span(file_id, 228, 399),
                                entries: vec![MappingEntry {
                                    key: "retries".to_string(),
                                    key_span: span(file_id, 228, 235),
                                    span: span(file_id, 228, 245),
                                    metadata: None,
                                    value: Some(Box::new(Node::Scalar(Scalar {
                                        kind: ScalarKind::Number,
                                        span: span(file_id, 237, 244),
                                        text: "3".to_string(),
                                    }))),
                                }],
                            }))),
                        },
                    ],
                })),
            ],
        }],
    };

    let semantic = SemanticDocument::from_ast(&ast);
    let index = DocumentIndex::new(semantic.clone());
    let expected_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/semantic/indexed_definitions.golden");
    let actual = render(&semantic, &index);

    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        fs::write(&expected_path, &actual).unwrap();
    }

    let expected = fs::read_to_string(expected_path).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn workspace_index_invalidates_documents() {
    let file_id = FileId(1);
    let uri = Url::parse("file:///workspace/main.lyma").unwrap();
    let ast = AstFile {
        span: span(file_id, 0, 10),
        documents: vec![Document {
            span: span(file_id, 0, 10),
            separator_span: None,
            items: vec![DocumentItem::Directive(Directive {
                name: "@import".to_string(),
                span: span(file_id, 0, 10),
                value: Some("dep.lyma as dep".to_string()),
            })],
        }],
    };

    let mut workspace = WorkspaceIndex::default();
    workspace.upsert(uri.clone(), SemanticDocument::from_ast(&ast));
    assert_eq!(workspace.definitions_named("dep").len(), 1);
    assert!(workspace.invalidate(&uri).is_some());
    assert_eq!(workspace.invalidations(), 1);
    assert!(workspace.document(&uri).is_none());
}

fn render(semantic: &SemanticDocument, index: &DocumentIndex) -> String {
    let mut out = String::new();
    out.push_str("definitions:\n");
    for definition in &semantic.definitions {
        out.push_str(&format!(
            "- {:?}: {} @ {} [{}..{}]\n",
            definition.kind,
            definition.name,
            definition.path,
            definition.selection_span.start,
            definition.selection_span.end
        ));
        if let Some(detail) = &definition.detail {
            out.push_str(&format!("  detail: {}\n", detail));
        }
    }

    out.push_str("symbols:\n");
    for symbol in index.symbols() {
        out.push_str(&format!("- {:?}: {}\n", symbol.kind, symbol.name));
    }
    out
}

fn span(file_id: FileId, start: usize, end: usize) -> SourceSpan {
    SourceSpan::new(file_id, start, end)
}

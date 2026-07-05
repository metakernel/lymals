use std::{
    fs,
    path::{Path, PathBuf},
};

use lumals::{
    ast::{AstFile, DocumentItem, Node, ScalarKind},
    parser,
    syntax::{FileId, ParsedFile, TokenKind},
};
use pretty_assertions::assert_eq;

#[test]
fn fallback_parser_matches_golden_fixtures() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parser");
    for input in fixture_paths(&fixture_dir) {
        let name = input.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&input).unwrap();
        let parsed = parser::parse_fallback(FileId(1), &name, &text);
        let actual = summarize(
            &parsed.file,
            &parsed.diagnostics,
            parsed.tokens.as_deref().unwrap_or(&[]),
        );
        let golden = input.with_extension("golden");

        if std::env::var_os("UPDATE_GOLDENS").is_some() {
            fs::write(&golden, &actual).unwrap();
        }

        let expected = fs::read_to_string(&golden).unwrap();
        assert_eq!(actual, expected, "fixture {}", name);
    }
}

fn fixture_paths(dir: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "luma"))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn summarize(
    file: &ParsedFile,
    diagnostics: &[lumals::diagnostics::Diagnostic],
    tokens: &[lumals::syntax::Token],
) -> String {
    let parsed = match file {
        ParsedFile::Fallback(parsed) => parsed,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return "non-fallback backend\n".to_owned(),
    };

    let mut out = String::new();
    out.push_str(&format!("documents: {}\n", parsed.ast.documents.len()));
    summarize_ast(&parsed.ast, &mut out);

    out.push_str("symbols:\n");
    let mut symbols = Vec::new();
    for document in &parsed.ast.documents {
        for item in &document.items {
            match item {
                DocumentItem::Directive(directive) => {
                    symbols.push(format!("directive:{}", directive.name))
                }
                DocumentItem::Let(binding) => symbols.push(format!("variable:{}", binding.name)),
                DocumentItem::Node(node) => collect_node_symbols(node, &mut symbols),
                DocumentItem::Comment(_) => {}
            }
        }
    }
    symbols.sort();
    for symbol in symbols {
        out.push_str("- ");
        out.push_str(&symbol);
        out.push('\n');
    }

    out.push_str("diagnostics:\n");
    for diagnostic in diagnostics {
        out.push_str(&format!("- {}: {}\n", diagnostic.code, diagnostic.message));
    }

    out.push_str("token-kinds:\n");
    let mut kinds = tokens
        .iter()
        .map(|token| format!("{:?}", token.kind))
        .collect::<Vec<_>>();
    kinds.dedup();
    for kind in kinds {
        if kind != format!("{:?}", TokenKind::LineBreak) {
            out.push_str("- ");
            out.push_str(&kind);
            out.push('\n');
        }
    }
    out
}

fn summarize_ast(file: &AstFile, out: &mut String) {
    for (doc_index, document) in file.documents.iter().enumerate() {
        out.push_str(&format!("document {}:\n", doc_index + 1));
        for item in &document.items {
            match item {
                DocumentItem::Directive(directive) => {
                    out.push_str(&format!("- directive {}\n", directive.name));
                }
                DocumentItem::Comment(comment) => {
                    out.push_str(&format!("- comment {}\n", comment.text));
                }
                DocumentItem::Let(binding) => {
                    out.push_str(&format!("- let {}\n", binding.name));
                }
                DocumentItem::Node(node) => summarize_node(node, out),
            }
        }
    }
}

fn summarize_node(node: &Node, out: &mut String) {
    match node {
        Node::Mapping(mapping) => {
            let keys = mapping
                .entries
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("- mapping [{}]\n", keys));
            for entry in &mapping.entries {
                if let Some(metadata) = &entry.metadata {
                    out.push_str(&format!("  metadata {}\n", metadata));
                }
                if let Some(value) = &entry.value {
                    summarize_node(value, out);
                }
            }
        }
        Node::Sequence(sequence) => {
            out.push_str(&format!("- sequence {}\n", sequence.items.len()));
            for item in &sequence.items {
                if let Some(value) = &item.value {
                    summarize_node(value, out);
                }
            }
        }
        Node::Scalar(scalar) => {
            out.push_str(&format!(
                "- scalar {:?}: {}\n",
                scalar.kind,
                scalar.text.replace('\n', "\\n")
            ));
        }
        Node::Tag(tag) => {
            out.push_str(&format!("- tag {}\n", tag.name));
            if let Some(value) = &tag.value {
                summarize_node(value, out);
            }
        }
        Node::Spread(spread) => out.push_str(&format!("- spread {}\n", spread.target)),
        Node::Conditional(conditional) => {
            out.push_str(&format!("- conditional {}\n", conditional.condition));
        }
        Node::Loop(loop_node) => out.push_str(&format!("- loop {}\n", loop_node.header)),
        Node::Error(error) => out.push_str(&format!("- error {}\n", error.message)),
    }
}

fn collect_node_symbols(node: &Node, symbols: &mut Vec<String>) {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                symbols.push(format!("key:{}", entry.key));
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
            symbols.push(format!("tag:{}", tag.name));
            if let Some(value) = &tag.value {
                collect_node_symbols(value, symbols);
            }
        }
        Node::Scalar(scalar) if scalar.kind == ScalarKind::LuaBlock => {
            symbols.push("scalar:lua-block".to_owned());
        }
        Node::Scalar(_)
        | Node::Spread(_)
        | Node::Conditional(_)
        | Node::Loop(_)
        | Node::Error(_) => {}
    }
}

use std::{fmt::Write, fs};

use serde_json::{Value, json};
use tower_lsp::{
    jsonrpc::{Error, Result},
    lsp_types::{ExecuteCommandParams, Url},
};

use crate::{
    ast::{self, Node},
    commands::{self, Command},
    formatting, index, parser,
    syntax::{FileId, ParsedFile, SourceSpan, SourceText},
    workspace,
};

use super::LumaLanguageServer;

impl LumaLanguageServer {
    pub(super) async fn handle_execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<Value>> {
        let command = Command::parse(&params.command).ok_or_else(Error::method_not_found)?;
        let arguments = params.arguments;

        let value = match command {
            Command::RestartIndex => {
                commands::expect_no_arguments(command, &arguments)?;
                self.execute_restart_index().await?
            }
            Command::ShowSyntaxTree => {
                let uri = commands::parse_uri_argument(command, &arguments)?;
                self.execute_show_syntax_tree(uri).await?
            }
            Command::ShowConfig => {
                commands::expect_no_arguments(command, &arguments)?;
                self.execute_show_config().await?
            }
            Command::FormatWorkspaceFile => {
                let uri = commands::parse_uri_argument(command, &arguments)?;
                self.execute_format_workspace_file(uri).await?
            }
            Command::ExplainDiagnostic => {
                let code = commands::parse_diagnostic_code_argument(&arguments)?;
                self.execute_explain_diagnostic(&code).await?
            }
        };

        Ok(Some(value))
    }

    async fn execute_restart_index(&self) -> Result<Value> {
        let snapshot = self.state.snapshot();
        let open_documents = self.state.open_document_snapshots();
        let index = index::WorkspaceIndex::load(
            &open_documents,
            &snapshot.workspace.folders,
            &snapshot.config,
        );

        let indexed_documents = index.documents().len();

        Ok(json!({
            "command": commands::RESTART_INDEX,
            "parseOnly": true,
            "workspaceFolders": snapshot.workspace.folders.len(),
            "openDocuments": open_documents.len(),
            "indexedDocuments": indexed_documents,
        }))
    }

    async fn execute_show_syntax_tree(&self, uri: Url) -> Result<Value> {
        let (file_id, text) = self.read_document_or_workspace_file(&uri, false)?;
        let parsed = parser::parse(file_id, uri.as_str(), &text);
        let tree = render_syntax_tree(&parsed.source, &parsed.file);

        Ok(json!({
            "command": commands::SHOW_SYNTAX_TREE,
            "uri": uri,
            "backend": format!("{:?}", parsed.backend),
            "tree": tree,
        }))
    }

    async fn execute_show_config(&self) -> Result<Value> {
        let snapshot = self.state.snapshot();
        let config = serde_json::to_value(&snapshot.config).map_err(|_| Error::internal_error())?;
        let folders = snapshot
            .workspace
            .folders
            .iter()
            .map(|folder| folder.uri.as_str().to_string())
            .collect::<Vec<_>>();

        Ok(json!({
            "command": commands::SHOW_CONFIG,
            "parseOnly": true,
            "workspaceFolders": folders,
            "config": config,
        }))
    }

    async fn execute_format_workspace_file(&self, uri: Url) -> Result<Value> {
        let (file_id, text) = self.read_document_or_workspace_file(&uri, true)?;
        let parsed = parser::parse(file_id, uri.as_str(), &text);
        let formatted = formatting::format_text(file_id, uri.as_str(), parsed.backend, &text);

        Ok(json!({
            "command": commands::FORMAT_WORKSPACE_FILE,
            "uri": uri,
            "parseOnly": true,
            "changed": formatted.changed,
            "text": formatted.text,
        }))
    }

    async fn execute_explain_diagnostic(&self, code: &str) -> Result<Value> {
        let Some((title, remediation)) = commands::diagnostic_explanation(code) else {
            return Err(Error::invalid_params("unknown diagnostic code"));
        };

        Ok(json!({
            "command": commands::EXPLAIN_DIAGNOSTIC,
            "code": code,
            "title": title,
            "remediation": remediation,
            "parseOnly": true,
        }))
    }

    fn read_document_or_workspace_file(
        &self,
        uri: &Url,
        require_workspace_file: bool,
    ) -> Result<(FileId, String)> {
        if !require_workspace_file {
            if let Some(document) = self
                .state
                .with_document(uri, |document| (document.file_id(), document.text()))
            {
                return Ok(document);
            }
        }

        let snapshot = self.state.snapshot();
        if !workspace::is_workspace_luma_uri(uri, &snapshot.workspace.folders, &snapshot.config) {
            return Err(Error::invalid_params(
                "workspace file must stay within configured roots and end with .luma",
            ));
        }

        let path = workspace::file_url_to_path(uri)
            .ok_or_else(|| Error::invalid_params("workspace file URI must use the file scheme"))?;
        let metadata = fs::metadata(&path)
            .map_err(|_| Error::invalid_params("workspace file could not be read"))?;
        if metadata.len() > u64::from(snapshot.config.max_indexed_file_bytes) {
            return Err(Error::invalid_params(
                "workspace file exceeds the configured size limit",
            ));
        }
        let text = fs::read_to_string(path)
            .map_err(|_| Error::invalid_params("workspace file could not be read"))?;
        Ok((FileId(0), text))
    }
}

fn render_syntax_tree(source: &SourceText, file: &ParsedFile) -> String {
    let mut out = String::new();

    match file {
        ParsedFile::Fallback(file) => {
            writeln!(&mut out, "File {}", span_label(source, file.span)).ok();
            for (index, document) in file.ast.documents.iter().enumerate() {
                writeln!(
                    &mut out,
                    "  Document[{index}] {}",
                    span_label(source, document.span)
                )
                .ok();

                for item in &document.items {
                    render_document_item(&mut out, source, item, 4);
                }
            }
        }
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => {
            writeln!(&mut out, "File {}", span_label(source, file.span())).ok();
            for (index, span) in file.document_spans().iter().enumerate() {
                writeln!(
                    &mut out,
                    "  Document[{index}] {}",
                    span_label(source, *span)
                )
                .ok();
            }
        }
    }

    out
}

fn render_document_item(
    out: &mut String,
    source: &SourceText,
    item: &ast::DocumentItem,
    indent: usize,
) {
    let padding = " ".repeat(indent);
    match item {
        ast::DocumentItem::Directive(directive) => {
            writeln!(
                out,
                "{padding}Directive {} {}",
                directive.name,
                span_label(source, directive.span)
            )
            .ok();
        }
        ast::DocumentItem::Comment(comment) => {
            writeln!(out, "{padding}Comment {}", span_label(source, comment.span)).ok();
        }
        ast::DocumentItem::Let(binding) => {
            writeln!(
                out,
                "{padding}Let {} {}",
                binding.name,
                span_label(source, binding.span)
            )
            .ok();
        }
        ast::DocumentItem::Node(node) => render_node(out, source, node, indent),
    }
}

fn render_node(out: &mut String, source: &SourceText, node: &Node, indent: usize) {
    let padding = " ".repeat(indent);
    match node {
        Node::Mapping(mapping) => {
            writeln!(out, "{padding}Mapping {}", span_label(source, mapping.span)).ok();
            for entry in &mapping.entries {
                writeln!(
                    out,
                    "{}Key {} {}",
                    " ".repeat(indent + 2),
                    entry.key,
                    span_label(source, entry.key_span)
                )
                .ok();
                if let Some(value) = &entry.value {
                    render_node(out, source, value, indent + 4);
                }
            }
        }
        Node::Sequence(sequence) => {
            writeln!(
                out,
                "{padding}Sequence {}",
                span_label(source, sequence.span)
            )
            .ok();
            for item in &sequence.items {
                writeln!(
                    out,
                    "{}Item {}",
                    " ".repeat(indent + 2),
                    span_label(source, item.span)
                )
                .ok();
                if let Some(value) = &item.value {
                    render_node(out, source, value, indent + 4);
                }
            }
        }
        Node::Scalar(scalar) => {
            writeln!(
                out,
                "{padding}Scalar {:?} {}",
                scalar.kind,
                span_label(source, scalar.span)
            )
            .ok();
        }
        Node::Tag(tag) => {
            writeln!(
                out,
                "{padding}Tag {} {}",
                tag.name,
                span_label(source, tag.span)
            )
            .ok();
            if let Some(value) = &tag.value {
                render_node(out, source, value, indent + 2);
            }
        }
        Node::Spread(spread) => {
            writeln!(
                out,
                "{padding}Spread {} {}",
                spread.target,
                span_label(source, spread.span)
            )
            .ok();
        }
        Node::Conditional(conditional) => {
            writeln!(
                out,
                "{padding}Conditional {} {}",
                conditional.condition,
                span_label(source, conditional.span)
            )
            .ok();
        }
        Node::Loop(loop_node) => {
            writeln!(
                out,
                "{padding}Loop {} {}",
                loop_node.header,
                span_label(source, loop_node.span)
            )
            .ok();
        }
        Node::Error(error) => {
            writeln!(
                out,
                "{padding}Error {} {}",
                error.message,
                span_label(source, error.span)
            )
            .ok();
        }
    }
}

fn span_label(source: &SourceText, span: SourceSpan) -> String {
    let start = source.position(span.start);
    let end = source.position(span.end);
    format!(
        "{}:{}-{}:{}",
        start.line, start.column, end.line, end.column
    )
}

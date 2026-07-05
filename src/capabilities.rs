use tower_lsp::lsp_types::{
    ClientCapabilities, CodeActionKind, CodeActionOptions, CodeActionProviderCapability,
    CompletionOptions, DeclarationCapability, ExecuteCommandOptions,
    FoldingRangeProviderCapability,
    HoverProviderCapability, ImplementationProviderCapability, OneOf, PositionEncodingKind,
    ReferencesOptions, RenameOptions, SelectionRangeProviderCapability,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensServerCapabilities,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TypeDefinitionProviderCapability,
    WorkspaceFoldersServerCapabilities,
    WorkspaceServerCapabilities,
};

use crate::{commands, semantic_tokens};

pub fn negotiate(client_capabilities: &ClientCapabilities) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(negotiate_position_encoding(client_capabilities)),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..TextDocumentSyncOptions::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![
                "@".to_string(),
                "/".to_string(),
                "\"".to_string(),
                "'".to_string(),
                ":".to_string(),
                "$".to_string(),
            ]),
            ..CompletionOptions::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        declaration_provider: Some(DeclarationCapability::Simple(true)),
        type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
        implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Right(ReferencesOptions {
            work_done_progress_options: Default::default(),
        })),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![
                CodeActionKind::QUICKFIX,
                CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
            ]),
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        })),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: Default::default(),
                legend: semantic_tokens::legend(),
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        inlay_hint_provider: Some(OneOf::Left(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: commands::Command::registration(),
            work_done_progress_options: Default::default(),
        }),
        document_on_type_formatting_provider: None,
        workspace: negotiate_workspace_capabilities(client_capabilities),
        ..ServerCapabilities::default()
    }
}

pub fn supports_workspace_configuration(client_capabilities: &ClientCapabilities) -> bool {
    client_capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.configuration)
        .unwrap_or(false)
}

fn negotiate_position_encoding(client_capabilities: &ClientCapabilities) -> PositionEncodingKind {
    let encodings = client_capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref());

    match encodings {
        Some(encodings)
            if encodings
                .iter()
                .any(|encoding| encoding == &PositionEncodingKind::UTF8) =>
        {
            PositionEncodingKind::UTF8
        }
        Some(encodings)
            if encodings
                .iter()
                .any(|encoding| encoding == &PositionEncodingKind::UTF32) =>
        {
            PositionEncodingKind::UTF32
        }
        _ => PositionEncodingKind::UTF16,
    }
}

fn negotiate_workspace_capabilities(
    client_capabilities: &ClientCapabilities,
) -> Option<WorkspaceServerCapabilities> {
    client_capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.workspace_folders)
        .map(|supported| WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(supported),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: None,
        })
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::{
        ClientCapabilities, GeneralClientCapabilities, PositionEncodingKind,
        WorkspaceClientCapabilities,
    };

    use super::{negotiate, supports_workspace_configuration};

    #[test]
    fn prefers_utf8_when_client_supports_it() {
        let capabilities = ClientCapabilities {
            general: Some(GeneralClientCapabilities {
                position_encodings: Some(vec![
                    PositionEncodingKind::UTF16,
                    PositionEncodingKind::UTF8,
                ]),
                ..GeneralClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        };

        let negotiated = negotiate(&capabilities);

        assert_eq!(
            negotiated.position_encoding,
            Some(PositionEncodingKind::UTF8)
        );
    }

    #[test]
    fn advertises_workspace_folders_only_when_client_supports_them() {
        let capabilities = ClientCapabilities {
            workspace: Some(WorkspaceClientCapabilities {
                workspace_folders: Some(true),
                ..WorkspaceClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        };

        let negotiated = negotiate(&capabilities);

        assert_eq!(
            negotiated
                .workspace
                .and_then(|workspace| workspace.workspace_folders)
                .and_then(|folders| folders.supported),
            Some(true)
        );
    }

    #[test]
    fn detects_workspace_configuration_support() {
        let capabilities = ClientCapabilities {
            workspace: Some(WorkspaceClientCapabilities {
                configuration: Some(true),
                ..WorkspaceClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        };

        assert!(supports_workspace_configuration(&capabilities));
    }
}

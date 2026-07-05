use std::sync::Arc;

#[path = "handlers/code_actions.rs"]
mod code_actions;
#[path = "handlers/commands.rs"]
mod commands;
#[path = "handlers/completion.rs"]
mod completion;
#[path = "handlers/diagnostics.rs"]
mod diagnostics;
#[path = "handlers/folding.rs"]
mod folding;
#[path = "handlers/formatting.rs"]
mod formatting;
#[path = "handlers/hover.rs"]
mod hover;
#[path = "handlers/inlay_hints.rs"]
mod inlay_hints;
#[path = "handlers/navigation.rs"]
mod navigation;
#[path = "handlers/references.rs"]
mod references;
#[path = "handlers/rename.rs"]
mod rename;
#[path = "handlers/selection_ranges.rs"]
mod selection_ranges;
#[path = "handlers/semantic_tokens.rs"]
mod semantic_tokens;
#[path = "handlers/symbols.rs"]
mod symbols;
#[path = "handlers/sync.rs"]
mod sync;
#[path = "handlers/workspace.rs"]
mod workspace;
#[path = "handlers/workspace_symbols.rs"]
mod workspace_symbols;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::notification::LogTrace;
use tower_lsp::lsp_types::request::{
    GotoDeclarationParams, GotoDeclarationResponse, GotoImplementationParams,
    GotoImplementationResponse, GotoTypeDefinitionParams, GotoTypeDefinitionResponse,
};
use tower_lsp::lsp_types::{
    CodeActionParams, CodeActionResponse, CompletionParams, CompletionResponse, ConfigurationItem,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentFormattingParams, DocumentRangeFormattingParams,
    DocumentSymbolParams, DocumentSymbolResponse, ExecuteCommandParams, FoldingRange,
    FoldingRangeParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    InitializeParams, InitializeResult, InitializedParams, InlayHint, InlayHintParams,
    LogTraceParams, MessageType, PrepareRenameResponse, ReferenceParams, RenameParams,
    SelectionRange, SelectionRangeParams, SemanticTokensParams, SemanticTokensResult, ServerInfo,
    SetTraceParams, SymbolInformation, TextEdit, TraceValue, Url, WorkspaceEdit,
    WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer, LspService};

use crate::VERSION;
use crate::capabilities;
use crate::config::{CONFIG_SECTION, LumalsConfig};
use crate::state::{LifecyclePhase, SessionSnapshot, SessionState};
use crate::workspace::folders_from_initialize_params;

#[derive(Clone)]
pub struct LumaLanguageServer {
    client: Client,
    state: Arc<SessionState>,
}

impl std::fmt::Debug for LumaLanguageServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LumaLanguageServer").finish_non_exhaustive()
    }
}

impl LumaLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(SessionState::new()),
        }
    }

    pub fn state_snapshot(&self) -> SessionSnapshot {
        self.state.snapshot()
    }

    pub fn has_open_document(&self, uri: &Url) -> bool {
        self.state.has_document(uri)
    }

    pub fn open_document_count(&self) -> usize {
        self.state.document_count()
    }

    pub async fn set_trace(&self, params: SetTraceParams) {
        self.state.set_trace(params.value);
        self.trace(
            "trace level updated",
            Some(format!("value={:?}", params.value)),
        )
        .await;
    }

    async fn trace(&self, message: impl Into<String>, verbose: Option<String>) {
        let snapshot = self.state.snapshot();

        match snapshot.trace {
            TraceValue::Off => {}
            TraceValue::Messages => {
                self.client
                    .send_notification::<LogTrace>(LogTraceParams {
                        message: message.into(),
                        verbose: None,
                    })
                    .await;
            }
            TraceValue::Verbose => {
                self.client
                    .send_notification::<LogTrace>(LogTraceParams {
                        message: message.into(),
                        verbose,
                    })
                    .await;
            }
        }
    }

    async fn refresh_configuration_from_client(&self) {
        let snapshot = self.state.snapshot();
        if !capabilities::supports_workspace_configuration(&snapshot.client_capabilities) {
            self.state.set_config(LumalsConfig::default());
            return;
        }

        match self
            .client
            .configuration(vec![ConfigurationItem {
                scope_uri: None,
                section: Some(CONFIG_SECTION.to_string()),
            }])
            .await
        {
            Ok(values) => {
                let config = values.first().map(LumalsConfig::from_lsp_value).transpose();

                match config {
                    Ok(Some(config)) => {
                        self.state.set_config(config);
                        self.trace(
                            "configuration loaded",
                            Some("source=workspace/configuration".to_string()),
                        )
                        .await;
                    }
                    Ok(None) => {
                        self.state.set_config(LumalsConfig::default());
                        self.trace(
                            "configuration defaults applied",
                            Some(
                                "source=defaults reason=empty-workspace/configuration-response"
                                    .to_string(),
                            ),
                        )
                        .await;
                    }
                    Err(error) => {
                        self.state.set_config(LumalsConfig::default());
                        self.client
                            .log_message(
                                MessageType::WARNING,
                                format!(
                                    "lumals configuration was invalid; using defaults: {error}"
                                ),
                            )
                            .await;
                        self.trace(
                            "configuration defaults applied",
                            Some(format!(
                                "source=defaults reason=workspace/configuration-parse-error error={error}"
                            )),
                        )
                        .await;
                    }
                }
            }
            Err(error) => {
                self.state.set_config(LumalsConfig::default());
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!(
                            "lumals could not read workspace configuration; using defaults: {error}"
                        ),
                    )
                    .await;
                self.trace(
                    "configuration defaults applied",
                    Some(format!(
                        "source=defaults reason=workspace/configuration-request-failed error={error}"
                    )),
                )
                .await;
            }
        }
    }

    async fn apply_configuration_value(&self, value: &serde_json::Value) {
        match LumalsConfig::from_lsp_value(value) {
            Ok(config) => {
                self.state.set_config(config);
                self.trace(
                    "configuration updated",
                    Some("source=workspace/didChangeConfiguration".to_string()),
                )
                .await;
            }
            Err(error) => {
                self.state.set_config(LumalsConfig::default());
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!(
                            "lumals ignored invalid configuration update; using defaults: {error}"
                        ),
                    )
                    .await;
                self.trace(
                    "configuration defaults applied",
                    Some(format!(
                        "source=defaults reason=workspace/didChangeConfiguration-parse-error error={error}"
                    )),
                )
                .await;
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LumaLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let workspace_folders = folders_from_initialize_params(
            params.workspace_folders.clone(),
            params.root_uri.clone(),
            #[allow(deprecated)]
            params.root_path.clone(),
        );
        self.state
            .set_client_capabilities(params.capabilities.clone());
        self.state
            .set_trace(params.trace.unwrap_or(TraceValue::Off));
        self.state.set_workspace_folder_entries(workspace_folders);

        Ok(InitializeResult {
            capabilities: capabilities::negotiate(&params.capabilities),
            server_info: Some(ServerInfo {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: Some(VERSION.to_string()),
            }),
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.state.mark_initialized();
        self.refresh_configuration_from_client().await;
        self.register_luma_file_watchers().await;
        self.client
            .log_message(MessageType::INFO, "lumals initialized")
            .await;
        self.trace(
            "server initialized",
            Some("lifecycle=initialized capabilities=minimal".to_string()),
        )
        .await;
    }

    async fn shutdown(&self) -> Result<()> {
        self.state.mark_shutdown();
        self.client
            .log_message(MessageType::INFO, "lumals shutting down")
            .await;
        self.trace(
            "server shutting down",
            Some("lifecycle=shutdown".to_string()),
        )
        .await;
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.handle_did_open(params).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.handle_did_change(params).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.handle_did_save(params).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.handle_did_close(params).await;
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        if params.settings.is_null() {
            self.refresh_configuration_from_client().await;
        } else {
            self.apply_configuration_value(&params.settings).await;
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        self.handle_did_change_workspace_folders(params).await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.handle_did_change_watched_files(params).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.handle_completion(params).await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        self.handle_code_action(params).await
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        self.handle_hover(params).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        self.handle_goto_definition(params).await
    }

    async fn goto_declaration(
        &self,
        params: GotoDeclarationParams,
    ) -> Result<Option<GotoDeclarationResponse>> {
        self.handle_goto_declaration(params).await
    }

    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> Result<Option<GotoTypeDefinitionResponse>> {
        self.handle_goto_type_definition(params).await
    }

    async fn goto_implementation(
        &self,
        params: GotoImplementationParams,
    ) -> Result<Option<GotoImplementationResponse>> {
        self.handle_goto_implementation(params).await
    }

    async fn references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<tower_lsp::lsp_types::Location>>> {
        self.handle_references(params).await
    }

    async fn prepare_rename(
        &self,
        params: tower_lsp::lsp_types::TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        self.handle_prepare_rename(params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        self.handle_rename(params).await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        self.handle_document_symbol(params).await
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        self.handle_document_formatting(params).await
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        self.handle_document_range_formatting(params).await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        self.handle_folding_ranges(params).await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        self.handle_workspace_symbol(params).await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        self.handle_semantic_tokens_full(params).await
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        self.handle_inlay_hints(params).await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        self.handle_selection_ranges(params).await
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        self.handle_execute_command(params).await
    }
}

pub fn service() -> (LspService<LumaLanguageServer>, tower_lsp::ClientSocket) {
    LspService::build(LumaLanguageServer::new)
        .custom_method("$/setTrace", LumaLanguageServer::set_trace)
        .finish()
}

pub fn is_shutdown(snapshot: &SessionSnapshot) -> bool {
    snapshot.phase == LifecyclePhase::Shutdown
}

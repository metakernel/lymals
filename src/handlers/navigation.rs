use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse,
    request::{
        GotoDeclarationParams, GotoDeclarationResponse, GotoImplementationParams,
        GotoImplementationResponse, GotoTypeDefinitionParams, GotoTypeDefinitionResponse,
    },
};

use crate::navigation::{self, NavigationRequest};

use super::LumaLanguageServer;

impl LumaLanguageServer {
    pub(super) async fn handle_goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let snapshot = self.state.snapshot();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            let locations = navigation::goto_definition(NavigationRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                offset,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
            });

            goto_definition_response(locations)
        });

        Ok(response.flatten())
    }

    pub(super) async fn handle_goto_declaration(
        &self,
        params: GotoDeclarationParams,
    ) -> Result<Option<GotoDeclarationResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let snapshot = self.state.snapshot();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            let locations = navigation::goto_declaration(NavigationRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                offset,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
            });

            goto_declaration_response(locations)
        });

        Ok(response.flatten())
    }

    pub(super) async fn handle_goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> Result<Option<GotoTypeDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let snapshot = self.state.snapshot();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            let locations = navigation::goto_type_definition(NavigationRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                offset,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
            });

            goto_definition_response(locations)
        });

        Ok(response.flatten())
    }

    pub(super) async fn handle_goto_implementation(
        &self,
        params: GotoImplementationParams,
    ) -> Result<Option<GotoImplementationResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let snapshot = self.state.snapshot();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            let locations = navigation::goto_implementation(NavigationRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                offset,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
            });

            goto_implementation_response(locations)
        });

        Ok(response.flatten())
    }
}

fn goto_definition_response(
    locations: Vec<tower_lsp::lsp_types::Location>,
) -> Option<GotoDefinitionResponse> {
    if locations.is_empty() {
        None
    } else if locations.len() == 1 {
        Some(GotoDefinitionResponse::Scalar(
            locations.into_iter().next()?,
        ))
    } else {
        Some(GotoDefinitionResponse::Array(locations))
    }
}

fn goto_declaration_response(
    locations: Vec<tower_lsp::lsp_types::Location>,
) -> Option<GotoDeclarationResponse> {
    (!locations.is_empty()).then_some(GotoDeclarationResponse::Array(locations))
}

fn goto_implementation_response(
    locations: Vec<tower_lsp::lsp_types::Location>,
) -> Option<GotoImplementationResponse> {
    (!locations.is_empty()).then_some(GotoImplementationResponse::Array(locations))
}

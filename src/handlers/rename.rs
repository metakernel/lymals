use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    PrepareRenameResponse, RenameParams, TextDocumentPositionParams, WorkspaceEdit,
};

use crate::rename::{self, RenameError, RenameRequest};

use super::LymaLanguageServer;

impl LymaLanguageServer {
    pub(super) async fn handle_prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let position = params.position;
        let snapshot = self.state.snapshot();
        let open_documents = self.state.open_document_snapshots();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            rename::prepare_rename(RenameRequest {
                uri: document.uri(),
                text: &text,
                file_id: document.file_id(),
                offset,
                workspace_folders: &snapshot.workspace.folders,
                config: &snapshot.config,
                open_documents: &open_documents,
            })
            .map(|item| PrepareRenameResponse::RangeWithPlaceholder {
                range: item.range,
                placeholder: item.placeholder,
            })
        });

        Ok(response.flatten())
    }

    pub(super) async fn handle_rename(
        &self,
        params: RenameParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;
        let snapshot = self.state.snapshot();
        let open_documents = self.state.open_document_snapshots();

        let response = self.state.with_document(&uri, |document| {
            let text = document.text();
            let offset = document.position_to_offset(position).ok()?;
            Some(rename::rename(
                RenameRequest {
                    uri: document.uri(),
                    text: &text,
                    file_id: document.file_id(),
                    offset,
                    workspace_folders: &snapshot.workspace.folders,
                    config: &snapshot.config,
                    open_documents: &open_documents,
                },
                &new_name,
            ))
        });

        match response.flatten() {
            Some(Ok(edit)) => Ok(Some(edit)),
            Some(Err(error)) => Err(rename_error(error)),
            None => Err(rename_error(RenameError::UnsupportedRange)),
        }
    }
}

fn rename_error(error: RenameError) -> tower_lsp::jsonrpc::Error {
    tower_lsp::jsonrpc::Error {
        code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
        message: error.to_string().into(),
        data: None,
    }
}

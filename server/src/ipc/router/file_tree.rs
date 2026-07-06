use core::file_tree::FileTreeError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::file_tree::{
    FileTreeSnapshot, GetFileTreeParams, SetFileTreeExpandedPathsParams,
};
use super::{parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "get" => get_file_tree(state, request),
        "setExpandedPaths" => set_expanded_paths(state, request),
        _ => unsupported_action(request),
    }
}

fn get_file_tree(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GetFileTreeParams>(request) {
        Ok(params) => match state.file_tree(
            params.workspace_id.map(Into::into),
            params.tab_id.map(Into::into),
        ) {
            Ok(file_tree) => {
                ServerMessage::ok(request.id, FileTreeSnapshot::from_file_tree(&file_tree))
            }
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn set_expanded_paths(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SetFileTreeExpandedPathsParams>(request) {
        Ok(params) => {
            let updated = state.set_file_tree_expanded_paths(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                params.expanded_paths,
            );

            if updated {
                ServerMessage::ok(request.id, true)
            } else {
                ServerMessage::error(
                    request.id,
                    "file_tree.tab_not_found",
                    "file tree tab does not exist",
                )
            }
        }
        Err(response) => response,
    }
}

fn file_tree_error_code(error: &FileTreeError) -> &'static str {
    match error {
        FileTreeError::WorkspaceNotFound => "file_tree.workspace_not_found",
        FileTreeError::TabNotFound => "file_tree.tab_not_found",
        FileTreeError::RootNotDirectory(_) => "file_tree.root_not_directory",
        FileTreeError::Io { .. } => "file_tree.read_failed",
    }
}

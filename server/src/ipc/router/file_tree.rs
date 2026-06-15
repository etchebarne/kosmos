use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::file_tree::{FileTreeParams, FileTreeSnapshot};
use super::{parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "list" => list_file_tree(state, request),
        _ => unsupported_action(request),
    }
}

fn list_file_tree(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<FileTreeParams>(request) {
        Ok(params) => {
            match state.file_tree(params.workspace_id.map(Into::into), params.options()) {
                Ok(tree) => ServerMessage::ok(request.id, FileTreeSnapshot::from_tree(&tree)),
                Err(error) => ServerMessage::error(
                    request.id,
                    file_tree_error_code(&error),
                    error.to_string(),
                ),
            }
        }
        Err(response) => response,
    }
}

fn file_tree_error_code(error: &core::file_tree::FileTreeError) -> &'static str {
    match error {
        core::file_tree::FileTreeError::WorkspaceUnavailable => "workspace.not_found",
        core::file_tree::FileTreeError::Io { .. } => "file_tree.read_failed",
    }
}

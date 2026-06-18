use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::file_tree::{
    CopyFileTreeEntryParams, CreateFileTreeEntryParams, DeleteFileTreeEntryParams, FileTreeParams,
    FileTreeSnapshot, MoveFileTreeEntryParams, RenameFileTreeEntryParams,
};
use super::{parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "list" => list_file_tree(state, request),
        "create" => create_file_tree_entry(state, request),
        "rename" => rename_file_tree_entry(state, request),
        "delete" => delete_file_tree_entry(state, request),
        "move" => move_file_tree_entry(state, request),
        "copy" => copy_file_tree_entry(state, request),
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

fn create_file_tree_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CreateFileTreeEntryParams>(request) {
        Ok(params) => {
            let options = params.options();
            file_tree_response(
                request.id,
                state.create_file_tree_entry(
                    params.workspace_id.map(Into::into),
                    params.parent_path,
                    &params.name,
                    params.kind.into(),
                    options,
                ),
            )
        }
        Err(response) => response,
    }
}

fn rename_file_tree_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<RenameFileTreeEntryParams>(request) {
        Ok(params) => {
            let options = params.options();
            file_tree_response(
                request.id,
                state.rename_file_tree_entry(
                    params.workspace_id.map(Into::into),
                    params.path,
                    &params.name,
                    options,
                ),
            )
        }
        Err(response) => response,
    }
}

fn delete_file_tree_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<DeleteFileTreeEntryParams>(request) {
        Ok(params) => {
            let options = params.options();
            file_tree_response(
                request.id,
                state.delete_file_tree_entry(
                    params.workspace_id.map(Into::into),
                    params.path,
                    options,
                ),
            )
        }
        Err(response) => response,
    }
}

fn move_file_tree_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<MoveFileTreeEntryParams>(request) {
        Ok(params) => {
            let options = params.options();
            file_tree_response(
                request.id,
                state.move_file_tree_entry(
                    params.workspace_id.map(Into::into),
                    params.path,
                    params.target_directory_path,
                    options,
                ),
            )
        }
        Err(response) => response,
    }
}

fn copy_file_tree_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CopyFileTreeEntryParams>(request) {
        Ok(params) => {
            let options = params.options();
            file_tree_response(
                request.id,
                state.copy_file_tree_entry(
                    params.workspace_id.map(Into::into),
                    params.path,
                    params.target_directory_path,
                    options,
                ),
            )
        }
        Err(response) => response,
    }
}

fn file_tree_response(
    request_id: u64,
    result: Result<core::file_tree::FileTree, core::file_tree::FileTreeError>,
) -> ServerMessage {
    match result {
        Ok(tree) => ServerMessage::ok(request_id, FileTreeSnapshot::from_tree(&tree)),
        Err(error) => {
            ServerMessage::error(request_id, file_tree_error_code(&error), error.to_string())
        }
    }
}

fn file_tree_error_code(error: &core::file_tree::FileTreeError) -> &'static str {
    match error {
        core::file_tree::FileTreeError::WorkspaceUnavailable => "workspace.not_found",
        core::file_tree::FileTreeError::AlreadyExists(_) => "file_tree.already_exists",
        core::file_tree::FileTreeError::InvalidCopy => "file_tree.invalid_copy",
        core::file_tree::FileTreeError::InvalidMove => "file_tree.invalid_move",
        core::file_tree::FileTreeError::InvalidName(_) => "file_tree.invalid_name",
        core::file_tree::FileTreeError::InvalidPath(_) => "file_tree.invalid_path",
        core::file_tree::FileTreeError::Io { .. } => "file_tree.read_failed",
        core::file_tree::FileTreeError::NotDirectory(_) => "file_tree.not_directory",
        core::file_tree::FileTreeError::OutsideWorkspace(_) => "file_tree.outside_workspace",
        core::file_tree::FileTreeError::RootOperation => "file_tree.root_operation",
    }
}

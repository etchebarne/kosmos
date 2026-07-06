use core::file_tree::FileTreeError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::file_tree::{
    CreateFileTreeEntryParams, DeleteFileTreeEntriesParams, DeleteFileTreeEntryParams,
    FileTreeResolvedPath, FileTreeSnapshot, GetFileTreeParams, RenameFileTreeEntryParams,
    ResolveFileTreePathParams, SetFileTreeExpandedPathsParams, TransferFileTreeEntriesParams,
};
use super::{parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "get" => get_file_tree(state, request),
        "setExpandedPaths" => set_expanded_paths(state, request),
        "createEntry" => create_entry(state, request),
        "renameEntry" => rename_entry(state, request),
        "moveEntries" => move_entries(state, request),
        "copyEntries" => copy_entries(state, request),
        "deleteEntry" => delete_entry(state, request),
        "deleteEntries" => delete_entries(state, request),
        "resolvePath" => resolve_path(state, request),
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

fn create_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CreateFileTreeEntryParams>(request) {
        Ok(params) => match state.create_file_tree_entry(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            params.parent_path.as_deref(),
            &params.name,
            params.kind.into(),
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn rename_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<RenameFileTreeEntryParams>(request) {
        Ok(params) => match state.rename_file_tree_entry(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.source_path,
            &params.destination_path,
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn move_entries(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<TransferFileTreeEntriesParams>(request) {
        Ok(params) => match state.move_file_tree_entries(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.source_paths,
            params.target_directory_path.as_deref(),
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn copy_entries(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<TransferFileTreeEntriesParams>(request) {
        Ok(params) => match state.copy_file_tree_entries(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.source_paths,
            params.target_directory_path.as_deref(),
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn delete_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<DeleteFileTreeEntryParams>(request) {
        Ok(params) => match state.delete_file_tree_entry(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.path,
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn delete_entries(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<DeleteFileTreeEntriesParams>(request) {
        Ok(params) => match state.delete_file_tree_entries(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.paths,
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn resolve_path(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ResolveFileTreePathParams>(request) {
        Ok(params) => match state.resolve_file_tree_path(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            params.path.as_deref(),
        ) {
            Ok(path) => ServerMessage::ok(
                request.id,
                FileTreeResolvedPath::new(path.to_string_lossy().into_owned()),
            ),
            Err(error) => {
                ServerMessage::error(request.id, file_tree_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn file_tree_error_code(error: &FileTreeError) -> &'static str {
    match error {
        FileTreeError::WorkspaceNotFound => "file_tree.workspace_not_found",
        FileTreeError::TabNotFound => "file_tree.tab_not_found",
        FileTreeError::RootNotDirectory(_) => "file_tree.root_not_directory",
        FileTreeError::InvalidPath(_) => "file_tree.invalid_path",
        FileTreeError::InvalidName(_) => "file_tree.invalid_name",
        FileTreeError::EntryNotFound(_) => "file_tree.entry_not_found",
        FileTreeError::DirectoryNotFound(_) => "file_tree.directory_not_found",
        FileTreeError::EntryAlreadyExists(_) => "file_tree.entry_already_exists",
        FileTreeError::CannotMoveIntoSelf { .. } => "file_tree.cannot_move_into_self",
        FileTreeError::UnsupportedEntry(_) => "file_tree.unsupported_entry",
        FileTreeError::Io { .. } => "file_tree.access_failed",
    }
}

use core::tabs::file_tree::FileTreeError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::file_tree::{
    CreateFileTreeEntryParams, DeleteFileTreeEntriesParams, FileTreeChildrenSnapshot,
    FileTreeResolvedPath, FileTreeSnapshot, GetFileTreeChildrenParams, GetFileTreeParams,
    RenameFileTreeEntryParams, ResolveFileTreePathParams, SetFileTreeExpandedPathsParams,
    TransferFileTreeEntriesParams,
};
use super::{RouteDefinition, parse_params};

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    match action {
        "get" => Some(RouteDefinition::external(get_file_tree)),
        "getChildren" => Some(RouteDefinition::external(get_file_tree_children)),
        "setExpandedPaths" => Some(RouteDefinition::full(set_expanded_paths)),
        "createEntry" => Some(RouteDefinition::external(create_entry)),
        "renameEntry" => Some(RouteDefinition::external(rename_entry)),
        "moveEntries" => Some(RouteDefinition::external(move_entries)),
        "copyEntries" => Some(RouteDefinition::external(copy_entries)),
        "deleteEntries" => Some(RouteDefinition::external(delete_entries)),
        "resolvePath" => Some(RouteDefinition::external(resolve_path)),
        _ => None,
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

fn get_file_tree_children(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GetFileTreeChildrenParams>(request) {
        Ok(params) => match state.file_tree_children(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.path,
        ) {
            Ok(directory) => ServerMessage::ok(
                request.id,
                FileTreeChildrenSnapshot::from_directory(&directory),
            ),
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
        FileTreeError::TooManyEntries { .. } => "file_tree.too_many_entries",
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

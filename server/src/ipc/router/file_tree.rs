use core::tabs::file_tree::FileTreeError;
use core::tabs::git::{GitError, GitRepository};

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::file_tree::{
    CreateFileTreeEntryParams, DeleteFileTreeEntriesParams, FileTreeChildrenSnapshot,
    FileTreeGitStatusSnapshot, FileTreePathMapper, FileTreeResolvedPath, FileTreeSnapshot,
    GetFileTreeChildrenParams, GetFileTreeGitStatusParams, GetFileTreeParams,
    RenameFileTreeEntryParams, ResolveFileTreePathParams, SetFileTreeExpandedPathsParams,
    TransferFileTreeEntriesParams,
};
use super::{Route, RouteDefinition, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "get",
        definition: RouteDefinition::external(get_file_tree),
    },
    Route {
        action: "gitStatus",
        definition: RouteDefinition::external(get_git_status),
    },
    Route {
        action: "getChildren",
        definition: RouteDefinition::external(get_file_tree_children),
    },
    Route {
        action: "setExpandedPaths",
        definition: RouteDefinition::full(set_expanded_paths),
    },
    Route {
        action: "createEntry",
        definition: RouteDefinition::external(create_entry),
    },
    Route {
        action: "renameEntry",
        definition: RouteDefinition::external(rename_entry),
    },
    Route {
        action: "moveEntries",
        definition: RouteDefinition::external(move_entries),
    },
    Route {
        action: "copyEntries",
        definition: RouteDefinition::external(copy_entries),
    },
    Route {
        action: "deleteEntries",
        definition: RouteDefinition::external(delete_entries),
    },
    Route {
        action: "resolvePath",
        definition: RouteDefinition::external(resolve_path),
    },
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn get_git_status(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<GetFileTreeGitStatusParams>(request) {
        Ok(params) => {
            let workspace_id = params.workspace_id.map(Into::into);
            let tab_id = params.tab_id.into();
            let root = match state.file_tree_root(workspace_id, tab_id) {
                Ok(root) => root,
                Err(error) => return file_tree_error(request.id, error),
            };
            let mapper = FileTreePathMapper::new(root);

            match GitRepository::workspace_changes(root) {
                Ok(changes) => ServerMessage::ok(
                    request.id,
                    FileTreeGitStatusSnapshot::from_changes(&changes, &mapper),
                ),
                Err(GitError::Discover { .. } | GitError::NotWorktree(_)) => {
                    ServerMessage::ok(request.id, FileTreeGitStatusSnapshot::empty())
                }
                Err(error) => ServerMessage::error(
                    request.id,
                    "file_tree.git_status_failed",
                    error.to_string(),
                ),
            }
        }
        Err(response) => response,
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
        Ok(params) => {
            let workspace_id = params.workspace_id.map(Into::into);
            let tab_id = params.tab_id.into();
            let mapper = match file_tree_path_mapper(state, workspace_id, tab_id) {
                Ok(mapper) => mapper,
                Err(error) => return file_tree_error(request.id, error),
            };
            let path = match mapper.relative_entry_path(&params.path) {
                Ok(path) => path,
                Err(error) => return file_tree_error(request.id, error),
            };

            match state.file_tree_children(workspace_id, tab_id, &path) {
                Ok(directory) => ServerMessage::ok(
                    request.id,
                    FileTreeChildrenSnapshot::from_directory(&directory, &mapper),
                ),
                Err(error) => file_tree_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn set_expanded_paths(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SetFileTreeExpandedPathsParams>(request) {
        Ok(params) => {
            let workspace_id = params.workspace_id.map(Into::into);
            let tab_id = params.tab_id.into();
            let mapper = match file_tree_path_mapper(state, workspace_id, tab_id) {
                Ok(mapper) => mapper,
                Err(error) => return file_tree_error(request.id, error),
            };
            let expanded_paths = match params
                .expanded_paths
                .iter()
                .map(|path| mapper.relative_path(path))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(paths) => paths.into_iter().flatten().collect(),
                Err(error) => return file_tree_error(request.id, error),
            };
            let updated = state.set_file_tree_expanded_paths(workspace_id, tab_id, expanded_paths);

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
        Ok(params) => {
            let workspace_id = params.workspace_id.map(Into::into);
            let tab_id = params.tab_id.into();
            let mapper = match file_tree_path_mapper(state, workspace_id, tab_id) {
                Ok(mapper) => mapper,
                Err(error) => return file_tree_error(request.id, error),
            };
            let parent_path = match relative_optional_path(&mapper, params.parent_path.as_deref()) {
                Ok(path) => path,
                Err(error) => return file_tree_error(request.id, error),
            };

            match state.create_file_tree_entry(
                workspace_id,
                tab_id,
                parent_path.as_deref(),
                &params.name,
                params.kind.into(),
            ) {
                Ok(()) => ServerMessage::ok(request.id, true),
                Err(error) => file_tree_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn rename_entry(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<RenameFileTreeEntryParams>(request) {
        Ok(params) => {
            let workspace_id = params.workspace_id.map(Into::into);
            let tab_id = params.tab_id.into();
            let mapper = match file_tree_path_mapper(state, workspace_id, tab_id) {
                Ok(mapper) => mapper,
                Err(error) => return file_tree_error(request.id, error),
            };
            let source_path = match mapper.relative_entry_path(&params.source_path) {
                Ok(path) => path,
                Err(error) => return file_tree_error(request.id, error),
            };
            let destination_path = match mapper.relative_entry_path(&params.destination_path) {
                Ok(path) => path,
                Err(error) => return file_tree_error(request.id, error),
            };

            match state.rename_file_tree_entry(
                workspace_id,
                tab_id,
                &source_path,
                &destination_path,
            ) {
                Ok(()) => ServerMessage::ok(request.id, true),
                Err(error) => file_tree_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn move_entries(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<TransferFileTreeEntriesParams>(request) {
        Ok(params) => transfer_entries(state, request, params, false),
        Err(response) => response,
    }
}

fn copy_entries(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<TransferFileTreeEntriesParams>(request) {
        Ok(params) => transfer_entries(state, request, params, true),
        Err(response) => response,
    }
}

fn delete_entries(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<DeleteFileTreeEntriesParams>(request) {
        Ok(params) => {
            let workspace_id = params.workspace_id.map(Into::into);
            let tab_id = params.tab_id.into();
            let mapper = match file_tree_path_mapper(state, workspace_id, tab_id) {
                Ok(mapper) => mapper,
                Err(error) => return file_tree_error(request.id, error),
            };
            let paths = match relative_entry_paths(&mapper, &params.paths) {
                Ok(paths) => paths,
                Err(error) => return file_tree_error(request.id, error),
            };

            match state.delete_file_tree_entries(workspace_id, tab_id, &paths) {
                Ok(()) => ServerMessage::ok(request.id, true),
                Err(error) => file_tree_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn resolve_path(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ResolveFileTreePathParams>(request) {
        Ok(params) => {
            let workspace_id = params.workspace_id.map(Into::into);
            let tab_id = params.tab_id.into();
            let mapper = match file_tree_path_mapper(state, workspace_id, tab_id) {
                Ok(mapper) => mapper,
                Err(error) => return file_tree_error(request.id, error),
            };
            let path = match relative_optional_path(&mapper, params.path.as_deref()) {
                Ok(path) => path,
                Err(error) => return file_tree_error(request.id, error),
            };

            match state.resolve_file_tree_path(workspace_id, tab_id, path.as_deref()) {
                Ok(path) => ServerMessage::ok(
                    request.id,
                    FileTreeResolvedPath::new(path.to_string_lossy().into_owned()),
                ),
                Err(error) => file_tree_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn transfer_entries(
    state: &mut core::State,
    request: &RequestEnvelope,
    params: TransferFileTreeEntriesParams,
    copy: bool,
) -> ServerMessage {
    let workspace_id = params.workspace_id.map(Into::into);
    let tab_id = params.tab_id.into();
    let mapper = match file_tree_path_mapper(state, workspace_id, tab_id) {
        Ok(mapper) => mapper,
        Err(error) => return file_tree_error(request.id, error),
    };
    let source_paths = match relative_entry_paths(&mapper, &params.source_paths) {
        Ok(paths) => paths,
        Err(error) => return file_tree_error(request.id, error),
    };
    let target_directory_path =
        match relative_optional_path(&mapper, params.target_directory_path.as_deref()) {
            Ok(path) => path,
            Err(error) => return file_tree_error(request.id, error),
        };
    let result = if copy {
        state.copy_file_tree_entries(
            workspace_id,
            tab_id,
            &source_paths,
            target_directory_path.as_deref(),
        )
    } else {
        state.move_file_tree_entries(
            workspace_id,
            tab_id,
            &source_paths,
            target_directory_path.as_deref(),
        )
    };

    match result {
        Ok(()) => ServerMessage::ok(request.id, true),
        Err(error) => file_tree_error(request.id, error),
    }
}

fn file_tree_path_mapper(
    state: &core::State,
    workspace_id: Option<core::tree::WorkspaceId>,
    tab_id: core::tree::TabId,
) -> Result<FileTreePathMapper, FileTreeError> {
    state
        .file_tree_root(workspace_id, tab_id)
        .map(FileTreePathMapper::new)
}

fn relative_optional_path(
    mapper: &FileTreePathMapper,
    path: Option<&str>,
) -> Result<Option<String>, FileTreeError> {
    match path {
        Some(path) => mapper.relative_path(path),
        None => Ok(None),
    }
}

fn relative_entry_paths(
    mapper: &FileTreePathMapper,
    paths: &[String],
) -> Result<Vec<String>, FileTreeError> {
    paths
        .iter()
        .map(|path| mapper.relative_entry_path(path))
        .collect()
}

fn file_tree_error(id: u64, error: FileTreeError) -> ServerMessage {
    ServerMessage::error(id, file_tree_error_code(&error), error.to_string())
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

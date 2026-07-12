use core::tabs::editor::EditorError;
use core::tabs::git::GitError;

use super::super::messages::editor::{
    EditorDocumentParams, EditorDocumentPayload, EditorGitLineHunksPayload, OpenEditorTabParams,
    SaveEditorDocumentParams,
};
use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::{Route, RouteDefinition, find_route, parse_params, workspace_list_response};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "openTab",
        definition: RouteDefinition::full(open_tab),
    },
    Route {
        action: "document",
        definition: RouteDefinition::external(document),
    },
    Route {
        action: "gitLineHunks",
        definition: RouteDefinition::external(git_line_hunks),
    },
    Route {
        action: "save",
        definition: RouteDefinition::external(save),
    },
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn git_line_hunks(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<EditorDocumentParams>(request) {
        Ok(params) => match state
            .editor_git_line_hunks(params.workspace_id.map(Into::into), params.tab_id.into())
        {
            Ok(hunks) => {
                ServerMessage::ok(request.id, EditorGitLineHunksPayload::from_hunks(&hunks))
            }
            Err(GitError::Discover { .. } | GitError::NotWorktree(_)) => {
                ServerMessage::ok(request.id, EditorGitLineHunksPayload::empty())
            }
            Err(error) => ServerMessage::error(
                request.id,
                "editor.git_line_hunks_failed",
                error.to_string(),
            ),
        },
        Err(response) => response,
    }
}

fn open_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<OpenEditorTabParams>(request) {
        Ok(params) => match state.open_editor_tab(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.path,
        ) {
            Ok(()) => workspace_list_response(request.id, state),
            Err(error) => editor_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn document(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<EditorDocumentParams>(request) {
        Ok(params) => {
            match state.editor_document(params.workspace_id.map(Into::into), params.tab_id.into()) {
                Ok(document) => {
                    ServerMessage::ok(request.id, EditorDocumentPayload::from_document(&document))
                }
                Err(error) => editor_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn save(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SaveEditorDocumentParams>(request) {
        Ok(params) => match state.save_editor_document(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.content,
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => editor_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn editor_error(id: u64, error: EditorError) -> ServerMessage {
    ServerMessage::error(id, editor_error_code(&error), error.to_string())
}

fn editor_error_code(error: &EditorError) -> &'static str {
    match error {
        EditorError::WorkspaceNotFound => "editor.workspace_not_found",
        EditorError::SourceTabNotFound => "editor.source_tab_not_found",
        EditorError::TabNotFound => "editor.tab_not_found",
        EditorError::WorkspaceNotDirectory(_) => "editor.workspace_not_directory",
        EditorError::InvalidPath(_) => "editor.invalid_path",
        EditorError::FileNotFound(_) => "editor.file_not_found",
        EditorError::SymlinkNotAllowed(_) => "editor.symlink_not_allowed",
        EditorError::NotRegularFile(_) => "editor.not_regular_file",
        EditorError::PathOutsideWorkspace(_) => "editor.path_outside_workspace",
        EditorError::FileTooLarge { .. } => "editor.file_too_large",
        EditorError::ContentTooLarge { .. } => "editor.content_too_large",
        EditorError::InvalidUtf8(_) => "editor.invalid_utf8",
        EditorError::Io { .. } => "editor.access_failed",
    }
}

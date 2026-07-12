use core::tabs::editor::EditorError;
use core::tabs::git::GitError;

use super::super::messages::editor::{
    ChangeEditorSessionParams, EditorDocumentParams, EditorDocumentPayload,
    EditorGitLineHunksPayload, OpenEditorSessionParams, OpenEditorTabParams,
    SaveEditorDocumentParams,
};
use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::workspace::WorkspaceListSnapshot;
use super::{Route, RouteDefinition, find_route, parse_params, workspace_list_response};

pub(super) const ROUTES: &[Route] = &[
    Route::new::<OpenEditorTabParams, WorkspaceListSnapshot>(
        "openTab",
        RouteDefinition::full(open_tab),
    ),
    Route::new::<EditorDocumentParams, EditorDocumentPayload>(
        "document",
        RouteDefinition::application(document),
    ),
    Route::new::<EditorDocumentParams, EditorGitLineHunksPayload>(
        "gitLineHunks",
        RouteDefinition::external(git_line_hunks),
    ),
    Route::new::<OpenEditorSessionParams, EditorDocumentPayload>(
        "openSession",
        RouteDefinition::application(open_session),
    ),
    Route::new::<ChangeEditorSessionParams, EditorDocumentPayload>(
        "changeSession",
        RouteDefinition::application(change_session),
    ),
    Route::new::<SaveEditorDocumentParams, EditorDocumentPayload>(
        "save",
        RouteDefinition::application(save),
    ),
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

fn document(application: &mut core::Application, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<EditorDocumentParams>(request) {
        Ok(params) => {
            match application
                .editor_session_document(params.workspace_id.map(Into::into), params.tab_id.into())
            {
                Ok(session) => ServerMessage::ok(
                    request.id,
                    EditorDocumentPayload::from_session(session, true),
                ),
                Err(error) => application_error(request.id, error),
            }
        }
        Err(response) => response,
    }
}

fn open_session(application: &mut core::Application, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<OpenEditorSessionParams>(request) {
        Ok(params) => match application.open_editor_session(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.path,
            params.content,
            params.revision,
        ) {
            Ok(update) => ServerMessage::ok(request.id, session_update_payload(update)),
            Err(error) => application_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn change_session(application: &mut core::Application, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ChangeEditorSessionParams>(request) {
        Ok(params) => match application.change_editor_session(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            params.content,
            params.revision,
        ) {
            Ok(update) => ServerMessage::ok(request.id, session_update_payload(update)),
            Err(error) => application_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn save(application: &mut core::Application, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SaveEditorDocumentParams>(request) {
        Ok(params) => match application.save_editor_session_unformatted(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            params.revision,
        ) {
            Ok(session) => ServerMessage::ok(
                request.id,
                EditorDocumentPayload::from_session(session, true),
            ),
            Err(error) => application_error(request.id, error),
        },
        Err(response) => response,
    }
}

fn session_update_payload(update: core::EditorSessionUpdate) -> EditorDocumentPayload {
    match update {
        core::EditorSessionUpdate::Applied(session) => {
            EditorDocumentPayload::from_session(session, true)
        }
        core::EditorSessionUpdate::Stale(session) => {
            EditorDocumentPayload::from_session(session, false)
        }
    }
}

fn application_error(id: u64, error: core::ApplicationError) -> ServerMessage {
    let code = match &error {
        core::ApplicationError::Editor(error) => editor_error_code(error),
        core::ApplicationError::EditorSession(core::EditorSessionError::ContentTooLarge) => {
            "editor.content_too_large"
        }
        core::ApplicationError::EditorSession(core::EditorSessionError::StaleRevision {
            ..
        }) => "editor.stale_revision",
        core::ApplicationError::EditorSession(_) => "editor.session_invalid",
        _ => "editor.session_failed",
    };
    ServerMessage::error(id, code, error.to_string())
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

use core::tabs::editor::EditorError;

use super::super::messages::editor::{
    ChangeEditorSessionParams, EditorDocumentParams, EditorDocumentPayload,
    EditorGitLineHunksPayload, OpenEditorLocationParams, OpenEditorLocationPayload,
    OpenEditorSessionParams, OpenEditorTabParams, SaveEditorDocumentParams,
    SaveEditorDocumentPayload,
};
use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::workspace::WorkspaceListSnapshot;
use super::{Route, RouteDefinition, find_route, parse_params, workspace_list_response};

pub(super) const ROUTES: &[Route] = &[
    Route::new::<OpenEditorTabParams, WorkspaceListSnapshot>(
        "openTab",
        RouteDefinition::full(open_tab),
    ),
    Route::new::<OpenEditorLocationParams, OpenEditorLocationPayload>(
        "openLocation",
        RouteDefinition::persistent_application(open_location),
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
    Route::new::<SaveEditorDocumentParams, SaveEditorDocumentPayload>(
        "save",
        RouteDefinition::editor_save(prepare_save, complete_save),
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

fn open_location(
    operation: &mut core::PreparedPersistentOperation,
    request: &RequestEnvelope,
) -> ServerMessage {
    match parse_params::<OpenEditorLocationParams>(request) {
        Ok(params) => {
            match operation.open_editor_location(params.workspace_id.into(), &params.path) {
                Ok(location) => {
                    ServerMessage::ok(request.id, OpenEditorLocationPayload::from_core(location))
                }
                Err(error) => application_error(request.id, error),
            }
        }
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

fn prepare_save(
    application: &mut core::Application,
    request: &RequestEnvelope,
) -> Result<core::PreparedEditorSessionSave, ServerMessage> {
    match parse_params::<SaveEditorDocumentParams>(request) {
        Ok(params) => application
            .prepare_save_editor_session(
                params.workspace_id.map(Into::into),
                params.tab_id.into(),
                params.revision,
            )
            .map_err(|error| application_error(request.id, error)),
        Err(response) => Err(response),
    }
}

fn complete_save(
    application: &mut core::Application,
    execution: core::ExecutedEditorSessionSave,
    request: &RequestEnvelope,
) -> ServerMessage {
    match application.complete_save_editor_session(execution) {
        Ok(result) => ServerMessage::ok(request.id, SaveEditorDocumentPayload::from_core(result)),
        Err(error) => application_error(request.id, error),
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
        core::ApplicationError::RequestCancelled => "language_servers.request_cancelled",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::messages::envelope::Domain;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn open_location_maps_one_core_snapshot_and_preserves_state_on_failure() {
        let (mut application, root, database, workspace_id) = application();
        let mut success = application.prepare_persistent_operation().unwrap();

        let response = open_location(&mut success, &request(1, workspace_id, "document.txt"));
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(
            response["result"]["snapshot"]["activeWorkspaceId"],
            workspace_id.value()
        );
        assert_eq!(
            response["result"]["target"]["workspaceId"],
            workspace_id.value()
        );
        assert_eq!(response["result"]["target"]["tabId"], 2);
        assert_eq!(response["result"]["target"]["path"], "document.txt");
        success.persist().unwrap();
        application.complete_persistent_operation(success).unwrap();

        let before = application.state().workspaces().clone();
        let mut failed = application.prepare_persistent_operation().unwrap();
        let response = open_location(&mut failed, &request(2, workspace_id, "missing.txt"));
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["error"]["code"], "editor.file_not_found");
        assert_eq!(failed.state().workspaces(), &before);
        assert_eq!(application.state().workspaces(), &before);
        application.abandon_persistent_operation();

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn save_maps_success_to_the_revisioned_core_result() {
        let (mut application, root, database, workspace_id, tab_id) = editor_application();
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "saved".to_owned(), 2)
            .unwrap();

        let response = execute_save(&mut application, &save_request(1, workspace_id, tab_id, 2));
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["result"]["savedRevision"], 2);
        assert_eq!(response["result"]["currentRevision"], 2);
        assert_eq!(response["result"]["savedContent"], "saved");
        assert_eq!(response["result"]["warnings"], serde_json::json!([]));

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn save_maps_stale_revisions_without_executing_the_core_operation() {
        let (mut application, root, database, workspace_id, tab_id) = editor_application();
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "newer".to_owned(), 2)
            .unwrap();

        let response = execute_save(&mut application, &save_request(1, workspace_id, tab_id, 1));
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["error"]["code"], "editor.stale_revision");
        assert_eq!(
            std::fs::read_to_string(root.join("document.txt")).unwrap(),
            "before"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn save_maps_disk_failures_as_fatal_errors() {
        let (mut application, root, database, workspace_id, tab_id) = editor_application();
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "saved".to_owned(), 2)
            .unwrap();
        std::fs::remove_file(root.join("document.txt")).unwrap();
        std::fs::create_dir(root.join("document.txt")).unwrap();

        let response = execute_save(&mut application, &save_request(1, workspace_id, tab_id, 2));
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["error"]["code"], "editor.not_regular_file");

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn save_preserves_formatter_failures_as_non_fatal_warnings() {
        let (mut application, root, database, workspace_id, tab_id) = editor_application();
        application
            .update_setting(
                core::settings::EDITOR_FORMAT_ON_SAVE,
                core::settings::SettingValue::Boolean(true),
            )
            .unwrap();
        application
            .open_editor_session(
                Some(workspace_id),
                tab_id,
                "document.txt",
                "before".to_owned(),
                1,
            )
            .unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "saved".to_owned(), 2)
            .unwrap();

        let response = execute_save(&mut application, &save_request(1, workspace_id, tab_id, 2));
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["result"]["savedContent"], "saved");
        assert_eq!(response["result"]["warnings"][0]["kind"], "formatting");

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    fn application() -> (
        core::Application,
        std::path::PathBuf,
        std::path::PathBuf,
        core::tree::WorkspaceId,
    ) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kosmos-server-editor-location-{nonce}"));
        let database = root.join("state.sqlite3");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("document.txt"), "before").unwrap();
        let store = core::DurableStore::open(&database).unwrap();
        let mut state = core::State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            core::tree::PaneId::new(1),
            core::tree::TabId::new(1),
            core::tree::TabKind::FileTree,
        ));

        (
            core::Application::new(state, store),
            root,
            database,
            workspace_id,
        )
    }

    fn editor_application() -> (
        core::Application,
        std::path::PathBuf,
        std::path::PathBuf,
        core::tree::WorkspaceId,
        core::tree::TabId,
    ) {
        let (mut application, root, database, workspace_id) = application();
        application
            .state_mut()
            .open_editor_tab(
                Some(workspace_id),
                core::tree::TabId::new(1),
                "document.txt",
            )
            .unwrap();
        let tab_id = application.state().editor_view_states()[0].tab_id();
        (application, root, database, workspace_id, tab_id)
    }

    fn execute_save(
        application: &mut core::Application,
        request: &RequestEnvelope,
    ) -> ServerMessage {
        match prepare_save(application, request) {
            Ok(prepared) => complete_save(
                application,
                prepared.execute(&core::language_servers::LanguageServerRequestCancellation::new()),
                request,
            ),
            Err(response) => response,
        }
    }

    fn request(id: u64, workspace_id: core::tree::WorkspaceId, path: &str) -> RequestEnvelope {
        RequestEnvelope {
            id,
            domain: Domain::Editor,
            action: "openLocation".to_owned(),
            params: serde_json::json!({
                "workspaceId": workspace_id.value(),
                "path": path,
            }),
        }
    }

    fn save_request(
        id: u64,
        workspace_id: core::tree::WorkspaceId,
        tab_id: core::tree::TabId,
        revision: u64,
    ) -> RequestEnvelope {
        RequestEnvelope {
            id,
            domain: Domain::Editor,
            action: "save".to_owned(),
            params: serde_json::json!({
                "workspaceId": workspace_id.value(),
                "tabId": tab_id.value(),
                "revision": revision,
            }),
        }
    }
}

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::tab::{
    ActivateTabParams, CloseDecisionPayload, CloseResultPayload, CloseTabParams, MoveTabParams,
    OpenTabParams, ResolveCloseParams, SetTabKindParams, SplitTabParams,
};
use super::super::messages::workspace::WorkspaceListSnapshot;
use super::{Route, RouteDefinition, command_response, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route::new::<OpenTabParams, WorkspaceListSnapshot>("open", RouteDefinition::full(open_tab)),
    Route::new::<ActivateTabParams, WorkspaceListSnapshot>(
        "activate",
        RouteDefinition::full(activate_tab),
    ),
    Route::new::<SetTabKindParams, WorkspaceListSnapshot>(
        "setKind",
        RouteDefinition::full(set_tab_kind),
    ),
    Route::new::<CloseTabParams, CloseResultPayload>(
        "close",
        RouteDefinition::application(close_tab),
    ),
    Route::new::<ResolveCloseParams, CloseResultPayload>(
        "resolveClose",
        RouteDefinition::application(resolve_close),
    ),
    Route::new::<MoveTabParams, WorkspaceListSnapshot>("move", RouteDefinition::full(move_tab)),
    Route::new::<SplitTabParams, WorkspaceListSnapshot>("split", RouteDefinition::full(split_tab)),
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn open_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<OpenTabParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.open_tab(
                params.workspace_id.map(Into::into),
                params.pane_id.map(Into::into),
                params.title,
                params.kind.unwrap_or_default().into(),
            ),
            state,
            "tab.open_failed",
            "tab could not be opened",
        ),
        Err(response) => response,
    }
}

fn activate_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ActivateTabParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.activate_tab(
                params.workspace_id.map(Into::into),
                params.pane_id.into(),
                params.tab_id.into(),
            ),
            state,
            "tab.not_found",
            "tab does not exist",
        ),
        Err(response) => response,
    }
}

fn set_tab_kind(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SetTabKindParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.set_tab_kind(
                params.workspace_id.map(Into::into),
                params.pane_id.into(),
                params.tab_id.into(),
                params.kind.into(),
            ),
            state,
            "tab.not_found",
            "tab does not exist",
        ),
        Err(response) => response,
    }
}

fn close_tab(application: &mut core::Application, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CloseTabParams>(request) {
        Ok(params) => {
            let workspace_id = params
                .workspace_id
                .map(Into::into)
                .or_else(|| application.state().workspaces().active_workspace_id());
            let result = workspace_id.map_or(
                Err(core::ApplicationError::InvalidCloseDecision),
                |workspace_id| {
                    application.begin_close(core::CloseIntent {
                        target: core::CloseTarget::Tab {
                            workspace_id,
                            pane_id: params.pane_id.into(),
                            tab_id: params.tab_id.into(),
                        },
                    })
                },
            );
            close_response(application, request.id, result)
        }
        Err(response) => response,
    }
}

fn resolve_close(application: &mut core::Application, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ResolveCloseParams>(request) {
        Ok(params) => {
            let decision = match params.decision {
                CloseDecisionPayload::Cancel => core::CloseDecision::Cancel {
                    close_id: params.close_id,
                },
                CloseDecisionPayload::Resolve { documents } => core::CloseDecision::Resolve {
                    close_id: params.close_id,
                    documents: documents
                        .into_iter()
                        .map(|document| core::CloseDocumentDecisionRequest {
                            id: core::EditorSessionId::new(
                                document.workspace_id.into(),
                                document.tab_id.into(),
                            ),
                            revision: document.revision,
                            decision: document.decision.into(),
                        })
                        .collect(),
                },
            };
            let result = application.resolve_close(decision);
            close_response(application, request.id, result)
        }
        Err(response) => response,
    }
}

pub(super) fn close_response(
    application: &core::Application,
    id: u64,
    result: Result<core::CloseIntentResult, core::ApplicationError>,
) -> ServerMessage {
    match result {
        Ok(result) => ServerMessage::ok(
            id,
            CloseResultPayload::from_core(
                result,
                WorkspaceListSnapshot::from_list(application.state().workspaces()),
            ),
        ),
        Err(error) => ServerMessage::error(id, "close.decision_failed", error.to_string()),
    }
}

fn move_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<MoveTabParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.move_tab(
                params.workspace_id.map(Into::into),
                params.pane_id.into(),
                params.target_pane_id.into(),
                params.tab_id.into(),
                params.target_index,
            ),
            state,
            "tab.move_failed",
            "tab could not be moved",
        ),
        Err(response) => response,
    }
}

fn split_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SplitTabParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.split_tab(
                params.workspace_id.map(Into::into),
                params.pane_id.into(),
                params
                    .target_pane_id
                    .map(Into::into)
                    .unwrap_or_else(|| params.pane_id.into()),
                params.tab_id.into(),
                params.axis.into(),
                params.new_pane_first.unwrap_or(false),
            ),
            state,
            "tab.split_failed",
            "tab could not be split into a new pane",
        ),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::messages::envelope::Domain;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dirty_tab_close_returns_a_typed_decision_and_rejects_a_stale_discard() {
        let (mut application, root, database, workspace_id, tab_id) = application();
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
            .change_editor_session(Some(workspace_id), tab_id, "changed".to_owned(), 2)
            .unwrap();
        let response = close_tab(
            &mut application,
            &RequestEnvelope {
                id: 1,
                domain: Domain::Tab,
                action: "close".to_owned(),
                params: serde_json::json!({
                    "workspaceId": workspace_id.value(),
                    "paneId": 1,
                    "tabId": tab_id.value(),
                }),
            },
        );
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["result"]["status"], "requiresDocumentDecision");
        let close_id = response["result"]["closeId"].as_u64().unwrap();
        application
            .change_editor_session(Some(workspace_id), tab_id, "newer".to_owned(), 3)
            .unwrap();

        let stale = resolve_close(
            &mut application,
            &RequestEnvelope {
                id: 2,
                domain: Domain::Tab,
                action: "resolveClose".to_owned(),
                params: serde_json::json!({
                    "closeId": close_id,
                    "decision": {
                        "kind": "resolve",
                        "documents": [{
                            "workspaceId": workspace_id.value(),
                            "tabId": tab_id.value(),
                            "revision": 2,
                            "decision": "discard"
                        }]
                    }
                }),
            },
        );
        let stale = serde_json::to_value(stale).unwrap();
        assert_eq!(stale["error"]["code"], "close.decision_failed");
        assert!(
            application
                .state()
                .editor_session_target(Some(workspace_id), tab_id)
                .is_ok()
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    fn application() -> (
        core::Application,
        std::path::PathBuf,
        std::path::PathBuf,
        core::tree::WorkspaceId,
        core::tree::TabId,
    ) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kosmos-server-close-{nonce}"));
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
        state
            .open_editor_tab(
                Some(workspace_id),
                core::tree::TabId::new(1),
                "document.txt",
            )
            .unwrap();
        let tab_id = state.editor_view_states()[0].tab_id();
        (
            core::Application::new(state, store),
            root,
            database,
            workspace_id,
            tab_id,
        )
    }
}

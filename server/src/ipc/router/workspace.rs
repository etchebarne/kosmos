use std::path::PathBuf;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::tab::{CloseDecisionPayload, CloseResultPayload, ResolveCloseParams};
use super::super::messages::workspace::{
    ActivateWorkspaceParams, CloseWorkspaceParams, MoveWorkspaceParams, OpenWorkspaceParams,
};
use super::super::messages::{EmptyParams, workspace::WorkspaceListSnapshot};
use super::{
    Route, RouteDefinition, command_response, find_route, parse_params, workspace_list_response,
};

pub(super) const ROUTES: &[Route] = &[
    Route::new::<EmptyParams, WorkspaceListSnapshot>(
        "list",
        RouteDefinition::snapshot(list_workspaces),
    ),
    Route::new::<EmptyParams, bool>(
        "flush",
        RouteDefinition::persistence_barrier(flush_persistence),
    ),
    Route::new::<OpenWorkspaceParams, WorkspaceListSnapshot>(
        "open",
        RouteDefinition::full(open_workspace),
    ),
    Route::new::<ActivateWorkspaceParams, WorkspaceListSnapshot>(
        "activate",
        RouteDefinition::active_workspace(activate_workspace),
    ),
    Route::new::<MoveWorkspaceParams, WorkspaceListSnapshot>(
        "move",
        RouteDefinition::full(move_workspace),
    ),
    Route::new::<CloseWorkspaceParams, CloseResultPayload>(
        "close",
        RouteDefinition::application(close_workspace),
    ),
    Route::new::<ResolveCloseParams, CloseResultPayload>(
        "resolveClose",
        RouteDefinition::application(resolve_close),
    ),
    Route::new::<EmptyParams, CloseResultPayload>(
        "closeApplication",
        RouteDefinition::application(close_application),
    ),
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn flush_persistence(_state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    ServerMessage::ok(request.id, true)
}

fn list_workspaces(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    workspace_list_response(request.id, state)
}

fn open_workspace(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<OpenWorkspaceParams>(request) {
        Ok(params) => {
            state.open_workspace(PathBuf::from(params.path));
            workspace_list_response(request.id, state)
        }
        Err(response) => response,
    }
}

fn activate_workspace(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ActivateWorkspaceParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.activate_workspace(params.workspace_id.into()),
            state,
            "workspace.not_found",
            "workspace does not exist",
        ),
        Err(response) => response,
    }
}

fn move_workspace(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<MoveWorkspaceParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.move_workspace(params.workspace_id.into(), params.target_index),
            state,
            "workspace.move_failed",
            "workspace could not be moved",
        ),
        Err(response) => response,
    }
}

fn close_workspace(
    application: &mut core::Application,
    request: &RequestEnvelope,
) -> ServerMessage {
    match parse_params::<CloseWorkspaceParams>(request) {
        Ok(params) => {
            let workspace_id = params
                .workspace_id
                .map(Into::into)
                .or_else(|| application.state().workspaces().active_workspace_id());
            let result = workspace_id.map_or(
                Err(core::ApplicationError::InvalidCloseDecision),
                |workspace_id| {
                    application.begin_close(core::CloseIntent {
                        target: core::CloseTarget::Workspace { workspace_id },
                    })
                },
            );
            super::tab::close_response(application, request.id, result)
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
            super::tab::close_response(application, request.id, result)
        }
        Err(response) => response,
    }
}

fn close_application(
    application: &mut core::Application,
    request: &RequestEnvelope,
) -> ServerMessage {
    let result = application.begin_close(core::CloseIntent {
        target: core::CloseTarget::Application,
    });
    super::tab::close_response(application, request.id, result)
}

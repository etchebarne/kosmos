use std::path::PathBuf;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::workspace::{
    ActivateWorkspaceParams, CloseWorkspaceParams, OpenWorkspaceParams,
};
use super::{
    Route, RouteDefinition, command_response, find_route, parse_params, workspace_list_response,
};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "list",
        definition: RouteDefinition::snapshot(list_workspaces),
    },
    Route {
        action: "flush",
        definition: RouteDefinition::persistence_barrier(flush_persistence),
    },
    Route {
        action: "open",
        definition: RouteDefinition::full(open_workspace),
    },
    Route {
        action: "activate",
        definition: RouteDefinition::active_workspace(activate_workspace),
    },
    Route {
        action: "close",
        definition: RouteDefinition::full(close_workspace),
    },
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

fn close_workspace(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CloseWorkspaceParams>(request) {
        Ok(params) => command_response(
            request.id,
            state
                .close_workspace(params.workspace_id.map(Into::into))
                .is_some(),
            state,
            "workspace.close_failed",
            "workspace could not be closed",
        ),
        Err(response) => response,
    }
}

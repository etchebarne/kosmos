use std::path::PathBuf;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::workspace::{
    ActivateWorkspaceParams, CloseWorkspaceParams, OpenWorkspaceParams,
};
use super::{RouteDefinition, command_response, parse_params, workspace_list_response};

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    match action {
        "list" => Some(RouteDefinition::snapshot(list_workspaces)),
        "flush" => Some(RouteDefinition::persistence_barrier(flush_persistence)),
        "open" => Some(RouteDefinition::full(open_workspace)),
        "activate" => Some(RouteDefinition::active_workspace(activate_workspace)),
        "close" => Some(RouteDefinition::full(close_workspace)),
        _ => None,
    }
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

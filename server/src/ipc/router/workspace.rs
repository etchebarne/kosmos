use std::path::PathBuf;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::workspace::{
    ActivateWorkspaceParams, CloseWorkspaceParams, OpenWorkspaceParams,
};
use super::{command_response, parse_params, unsupported_action, workspace_list_response};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "list" => workspace_list_response(request.id, state),
        "open" => open_workspace(state, request),
        "activate" => activate_workspace(state, request),
        "close" => close_workspace(state, request),
        _ => unsupported_action(request),
    }
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

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::pane::{ActivatePaneParams, MovePaneParams, SplitPaneParams};
use super::{command_response, parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "split" => split_pane(state, request),
        "activate" => activate_pane(state, request),
        "move" => move_pane(state, request),
        _ => unsupported_action(request),
    }
}

fn split_pane(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<SplitPaneParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.split_pane(
                params.workspace_id.map(Into::into),
                params.pane_id.map(Into::into),
                params.axis.into(),
                params.new_pane_first.unwrap_or(false),
            ),
            state,
            "pane.split_failed",
            "pane could not be split",
        ),
        Err(response) => response,
    }
}

fn move_pane(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<MovePaneParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.move_pane(
                params.workspace_id.map(Into::into),
                params.pane_id.into(),
                params.target_pane_id.into(),
                params.axis.into(),
                params.new_pane_first.unwrap_or(false),
            ),
            state,
            "pane.move_failed",
            "pane could not be moved",
        ),
        Err(response) => response,
    }
}

fn activate_pane(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ActivatePaneParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.activate_pane(params.workspace_id.map(Into::into), params.pane_id.into()),
            state,
            "pane.not_found",
            "pane does not exist",
        ),
        Err(response) => response,
    }
}

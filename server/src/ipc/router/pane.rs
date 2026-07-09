use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::pane::{
    ActivatePaneParams, MovePaneParams, ResizeSplitParams, SplitPaneParams,
};
use super::{RouteDefinition, command_response, parse_params};

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    let handler = match action {
        "split" => split_pane,
        "activate" => activate_pane,
        "move" => move_pane,
        "resize" => resize_split,
        _ => return None,
    };

    Some(RouteDefinition::full(handler))
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

fn resize_split(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ResizeSplitParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.resize_split(
                params.workspace_id.map(Into::into),
                params.split_id.into(),
                params.ratio,
            ),
            state,
            "pane.resize_failed",
            "pane split could not be resized",
        ),
        Err(response) => response,
    }
}

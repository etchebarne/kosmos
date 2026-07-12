use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::pane::{
    ActivatePaneParams, MovePaneParams, ResizeSplitParams, SplitPaneParams,
};
use super::{Route, RouteDefinition, command_response, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "split",
        definition: RouteDefinition::full(split_pane),
    },
    Route {
        action: "activate",
        definition: RouteDefinition::full(activate_pane),
    },
    Route {
        action: "move",
        definition: RouteDefinition::full(move_pane),
    },
    Route {
        action: "resize",
        definition: RouteDefinition::full(resize_split),
    },
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
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

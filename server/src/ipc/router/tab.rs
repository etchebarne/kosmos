use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::tab::{
    ActivateTabParams, CloseTabParams, MoveTabParams, OpenTabParams, SetTabKindParams,
    SplitTabParams,
};
use super::{Route, RouteDefinition, command_response, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route {
        action: "open",
        definition: RouteDefinition::full(open_tab),
    },
    Route {
        action: "activate",
        definition: RouteDefinition::full(activate_tab),
    },
    Route {
        action: "setKind",
        definition: RouteDefinition::full(set_tab_kind),
    },
    Route {
        action: "close",
        definition: RouteDefinition::full(close_tab),
    },
    Route {
        action: "move",
        definition: RouteDefinition::full(move_tab),
    },
    Route {
        action: "split",
        definition: RouteDefinition::full(split_tab),
    },
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

fn close_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<CloseTabParams>(request) {
        Ok(params) => command_response(
            request.id,
            state
                .close_tab(
                    params.workspace_id.map(Into::into),
                    params.pane_id.into(),
                    params.tab_id.into(),
                )
                .is_some(),
            state,
            "tab.not_found",
            "tab does not exist",
        ),
        Err(response) => response,
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

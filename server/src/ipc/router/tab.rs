use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::tab::{
    ActivateTabParams, CloseTabParams, MoveTabParams, OpenTabParams, SetTabKindParams,
    SplitTabParams,
};
use super::{RoutedResponse, command_response, parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> RoutedResponse {
    let response = match request.action.as_str() {
        "open" => open_tab(state, request),
        "activate" => activate_tab(state, request),
        "setKind" => set_tab_kind(state, request),
        "close" => close_tab(state, request),
        "move" => move_tab(state, request),
        "split" => split_tab(state, request),
        _ => return RoutedResponse::none(unsupported_action(request)),
    };

    RoutedResponse::full(response)
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

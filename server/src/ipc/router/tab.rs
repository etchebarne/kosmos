use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::tab::{
    ActivateTabParams, CloseTabParams, OpenTabParams, ReorderTabParams,
};
use super::{command_response, parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match request.action.as_str() {
        "open" => open_tab(state, request),
        "activate" => activate_tab(state, request),
        "close" => close_tab(state, request),
        "reorder" => reorder_tab(state, request),
        _ => unsupported_action(request),
    }
}

fn open_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<OpenTabParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.open_tab(
                params.workspace_id.map(Into::into),
                params.pane_id.map(Into::into),
                params.title.unwrap_or_else(|| "Blank".to_owned()),
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

fn reorder_tab(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ReorderTabParams>(request) {
        Ok(params) => command_response(
            request.id,
            state.reorder_tab(
                params.workspace_id.map(Into::into),
                params.pane_id.into(),
                params.tab_id.into(),
                params.target_index,
            ),
            state,
            "tab.reorder_failed",
            "tab could not be reordered",
        ),
        Err(response) => response,
    }
}

use super::super::messages::EmptyParams;
use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::formatters::{
    FormatterListSnapshot, FormatterParams, FormatterPrioritiesParams, FormatterSnapshot,
};
use super::{Route, RouteDefinition, find_route, parse_params};

pub(super) const ROUTES: &[Route] = &[
    Route::new::<EmptyParams, FormatterListSnapshot>("list", RouteDefinition::snapshot(list)),
    Route::new::<FormatterParams, FormatterSnapshot>("status", RouteDefinition::snapshot(status)),
    Route::new::<FormatterParams, FormatterSnapshot>("install", RouteDefinition::live(install)),
    Route::new::<FormatterParams, FormatterSnapshot>("uninstall", RouteDefinition::live(uninstall)),
    Route::new::<FormatterPrioritiesParams, FormatterListSnapshot>(
        "set-priorities",
        RouteDefinition::live(set_priorities),
    ),
];

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    find_route(ROUTES, action)
}

fn list(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match state.formatters() {
        Ok(statuses) => ServerMessage::ok(request.id, FormatterListSnapshot::new(statuses)),
        Err(error) => formatter_error(request.id, error),
    }
}

fn status(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<FormatterParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.formatter_status(&params.formatter_id) {
        Ok(status) => ServerMessage::ok(request.id, FormatterSnapshot::from_status(status)),
        Err(error) => formatter_error(request.id, error),
    }
}

fn install(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<FormatterParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.install_formatter(&params.formatter_id) {
        Ok(status) => ServerMessage::ok(request.id, FormatterSnapshot::from_status(status)),
        Err(error) => formatter_error(request.id, error),
    }
}

fn uninstall(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<FormatterParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.uninstall_formatter(&params.formatter_id) {
        Ok(status) => ServerMessage::ok(request.id, FormatterSnapshot::from_status(status)),
        Err(error) => formatter_error(request.id, error),
    }
}

fn set_priorities(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let params = match parse_params::<FormatterPrioritiesParams>(request) {
        Ok(params) => params,
        Err(response) => return response,
    };
    match state.set_formatter_priorities(params.formatter_ids) {
        Ok(statuses) => ServerMessage::ok(request.id, FormatterListSnapshot::new(statuses)),
        Err(error) => formatter_error(request.id, error),
    }
}

fn formatter_error(id: u64, error: core::formatters::FormatterError) -> ServerMessage {
    ServerMessage::error(id, error.code(), error.to_string())
}

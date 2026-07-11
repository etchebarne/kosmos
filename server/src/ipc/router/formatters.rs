use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::formatters::{
    FormatterListSnapshot, FormatterParams, FormatterSnapshot,
};
use super::{RouteDefinition, parse_params};

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    match action {
        "list" => Some(RouteDefinition::snapshot(list)),
        "status" => Some(RouteDefinition::snapshot(status)),
        "install" => Some(RouteDefinition::live(install)),
        "uninstall" => Some(RouteDefinition::live(uninstall)),
        _ => None,
    }
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

fn formatter_error(id: u64, error: core::formatters::FormatterError) -> ServerMessage {
    ServerMessage::error(id, error.code(), error.to_string())
}

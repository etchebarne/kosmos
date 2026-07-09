use core::tabs::terminal::TerminalError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::terminal::{
    OpenTerminalParams, ResizeTerminalParams, TerminalOutputSnapshot, TerminalTabParams,
    WriteTerminalInputParams,
};
use super::{RoutedResponse, parse_params, unsupported_action};

pub(super) fn route(state: &mut core::State, request: &RequestEnvelope) -> RoutedResponse {
    let response = match request.action.as_str() {
        "open" => open_terminal(state, request),
        "read" => read_terminal_output(state, request),
        "write" => write_terminal_input(state, request),
        "resize" => resize_terminal(state, request),
        _ => return RoutedResponse::none(unsupported_action(request)),
    };

    RoutedResponse::none(response)
}

fn open_terminal(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<OpenTerminalParams>(request) {
        Ok(params) => match state.open_terminal(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            params.columns,
            params.rows,
        ) {
            Ok(output) => {
                ServerMessage::ok(request.id, TerminalOutputSnapshot::from_output(&output))
            }
            Err(error) => {
                ServerMessage::error(request.id, terminal_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn read_terminal_output(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<TerminalTabParams>(request) {
        Ok(params) => match state
            .read_terminal_output(params.workspace_id.map(Into::into), params.tab_id.into())
        {
            Ok(output) => {
                ServerMessage::ok(request.id, TerminalOutputSnapshot::from_output(&output))
            }
            Err(error) => {
                ServerMessage::error(request.id, terminal_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn write_terminal_input(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<WriteTerminalInputParams>(request) {
        Ok(params) => match state.write_terminal_input(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            &params.data,
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, terminal_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn resize_terminal(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<ResizeTerminalParams>(request) {
        Ok(params) => match state.resize_terminal(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            params.columns,
            params.rows,
        ) {
            Ok(()) => ServerMessage::ok(request.id, true),
            Err(error) => {
                ServerMessage::error(request.id, terminal_error_code(&error), error.to_string())
            }
        },
        Err(response) => response,
    }
}

fn terminal_error_code(error: &TerminalError) -> &'static str {
    match error {
        TerminalError::WorkspaceNotFound => "terminal.workspace_not_found",
        TerminalError::TabNotFound => "terminal.tab_not_found",
        TerminalError::SessionNotFound => "terminal.session_not_found",
        TerminalError::InvalidSize { .. } => "terminal.invalid_size",
        TerminalError::ReadBufferUnavailable => "terminal.output_unavailable",
        TerminalError::Pty(_) => "terminal.process_failed",
        TerminalError::Io(_) => "terminal.io_failed",
    }
}

use core::tabs::terminal::TerminalError;

use super::super::messages::envelope::{RequestEnvelope, ServerMessage};
use super::super::messages::terminal::{
    OpenTerminalParams, ResizeTerminalParams, RestartTerminalParams, TerminalOutputSnapshot,
    TerminalShellSnapshot, TerminalTabParams, WriteTerminalInputParams,
};
use super::{RouteDefinition, parse_params};

pub(super) fn resolve(action: &str) -> Option<RouteDefinition> {
    match action {
        "shells" => Some(RouteDefinition::snapshot(list_shells)),
        "open" => Some(RouteDefinition::live(open_terminal)),
        "read" => Some(RouteDefinition::live(read_terminal_output)),
        "write" => Some(RouteDefinition::live(write_terminal_input)),
        "resize" => Some(RouteDefinition::live(resize_terminal)),
        "restart" => Some(RouteDefinition::live(restart_terminal)),
        _ => None,
    }
}

fn list_shells(_state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    let shells = core::tabs::terminal::available_shells()
        .iter()
        .map(TerminalShellSnapshot::from_shell)
        .collect::<Vec<_>>();

    ServerMessage::ok(request.id, shells)
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

fn restart_terminal(state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
    match parse_params::<RestartTerminalParams>(request) {
        Ok(params) => match state.restart_terminal(
            params.workspace_id.map(Into::into),
            params.tab_id.into(),
            params.columns,
            params.rows,
            &params.shell,
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

fn terminal_error_code(error: &TerminalError) -> &'static str {
    match error {
        TerminalError::InputBufferFull => "terminal.input_buffer_full",
        TerminalError::InputTooLarge { .. } => "terminal.input_too_large",
        TerminalError::WorkspaceNotFound => "terminal.workspace_not_found",
        TerminalError::TabNotFound => "terminal.tab_not_found",
        TerminalError::SessionNotFound => "terminal.session_not_found",
        TerminalError::ShellNotAvailable(_) => "terminal.shell_not_available",
        TerminalError::InvalidSize { .. } => "terminal.invalid_size",
        TerminalError::ReadBufferUnavailable => "terminal.output_unavailable",
        TerminalError::Pty(_) => "terminal.process_failed",
        TerminalError::Io(_) => "terminal.io_failed",
        TerminalError::WriterUnavailable => "terminal.writer_unavailable",
    }
}

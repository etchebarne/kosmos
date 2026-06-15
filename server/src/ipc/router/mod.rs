mod file_tree;
mod pane;
mod tab;
mod workspace;

use serde::de::DeserializeOwned;

use super::messages::envelope::{Domain, RequestEnvelope, ServerMessage};
use super::messages::workspace::WorkspaceListSnapshot;

pub(crate) fn route(state: &mut core::State, request: RequestEnvelope) -> ServerMessage {
    match request.domain {
        Domain::Workspace => workspace::route(state, &request),
        Domain::Pane => pane::route(state, &request),
        Domain::Tab => tab::route(state, &request),
        Domain::FileTree => file_tree::route(state, &request),
    }
}

pub(super) fn parse_params<T>(request: &RequestEnvelope) -> Result<T, ServerMessage>
where
    T: DeserializeOwned,
{
    serde_json::from_value(request.params.clone())
        .map_err(|error| ServerMessage::error(request.id, "ipc.invalid_params", error.to_string()))
}

pub(super) fn command_response(
    id: u64,
    succeeded: bool,
    state: &core::State,
    error_code: &'static str,
    error_message: impl Into<String>,
) -> ServerMessage {
    if succeeded {
        workspace_list_response(id, state)
    } else {
        ServerMessage::error(id, error_code, error_message)
    }
}

pub(super) fn workspace_list_response(id: u64, state: &core::State) -> ServerMessage {
    ServerMessage::ok(id, WorkspaceListSnapshot::from_list(state.workspaces()))
}

pub(super) fn unsupported_action(request: &RequestEnvelope) -> ServerMessage {
    ServerMessage::error(
        request.id,
        "ipc.unsupported_action",
        format!(
            "unsupported {:?}.{} IPC action",
            request.domain, request.action
        ),
    )
}

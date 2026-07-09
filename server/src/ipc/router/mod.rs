mod file_tree;
mod git;
mod pane;
mod tab;
mod terminal;
mod workspace;

use serde::de::DeserializeOwned;

use super::messages::envelope::{Domain, RequestEnvelope, ServerMessage};
use super::messages::workspace::WorkspaceListSnapshot;

pub(crate) fn route(state: &mut core::State, request: &RequestEnvelope) -> RoutedResponse {
    match request.domain {
        Domain::Workspace => workspace::route(state, request),
        Domain::Pane => pane::route(state, request),
        Domain::Tab => tab::route(state, request),
        Domain::FileTree => file_tree::route(state, request),
        Domain::Git => git::route(state, request),
        Domain::Terminal => terminal::route(state, request),
    }
}

pub(crate) struct RoutedResponse {
    response: ServerMessage,
    persistence: PersistenceMode,
}

impl RoutedResponse {
    fn new(response: ServerMessage, persistence: PersistenceMode) -> Self {
        Self {
            response,
            persistence,
        }
    }

    pub(super) fn none(response: ServerMessage) -> Self {
        Self::new(response, PersistenceMode::None)
    }

    pub(super) fn active_workspace(response: ServerMessage) -> Self {
        Self::new(response, PersistenceMode::ActiveWorkspace)
    }

    pub(super) fn full(response: ServerMessage) -> Self {
        Self::new(response, PersistenceMode::Full)
    }

    pub(crate) fn into_parts(self) -> (ServerMessage, PersistenceMode) {
        (self.response, self.persistence)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PersistenceMode {
    None,
    ActiveWorkspace,
    Full,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_is_declared_by_the_routed_operation() {
        let mut state = core::State::new();

        assert_eq!(
            persistence_for(&mut state, Domain::Workspace, "list"),
            PersistenceMode::None
        );
        assert_eq!(
            persistence_for(&mut state, Domain::Workspace, "activate"),
            PersistenceMode::ActiveWorkspace
        );
        assert_eq!(
            persistence_for(&mut state, Domain::FileTree, "createEntry"),
            PersistenceMode::None
        );
        assert_eq!(
            persistence_for(&mut state, Domain::FileTree, "setExpandedPaths"),
            PersistenceMode::Full
        );
        assert_eq!(
            persistence_for(&mut state, Domain::Git, "status"),
            PersistenceMode::None
        );
        assert_eq!(
            persistence_for(&mut state, Domain::Git, "openDiffTab"),
            PersistenceMode::Full
        );
        assert_eq!(
            persistence_for(&mut state, Domain::Terminal, "open"),
            PersistenceMode::None
        );
    }

    fn persistence_for(state: &mut core::State, domain: Domain, action: &str) -> PersistenceMode {
        let request = RequestEnvelope {
            id: 1,
            domain,
            action: action.to_owned(),
            params: serde_json::Value::Null,
        };

        route(state, &request).into_parts().1
    }
}

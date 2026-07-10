mod editor;
mod file_tree;
mod git;
mod pane;
mod settings;
mod tab;
mod terminal;
mod workspace;

use serde::de::DeserializeOwned;

use super::messages::envelope::{Domain, RequestEnvelope, ServerMessage};
use super::messages::workspace::WorkspaceListSnapshot;

type RouteHandler = fn(&mut core::State, &RequestEnvelope) -> ServerMessage;

pub(crate) fn prepare(request: RequestEnvelope) -> Result<PreparedRoute, ServerMessage> {
    let definition = match request.domain {
        Domain::Workspace => workspace::resolve(&request.action),
        Domain::Pane => pane::resolve(&request.action),
        Domain::Tab => tab::resolve(&request.action),
        Domain::FileTree => file_tree::resolve(&request.action),
        Domain::Editor => editor::resolve(&request.action),
        Domain::Git => git::resolve(&request.action),
        Domain::Terminal => terminal::resolve(&request.action),
        Domain::Settings => settings::resolve(&request.action),
    }
    .ok_or_else(|| unsupported_action(&request))?;

    Ok(PreparedRoute {
        request,
        definition,
    })
}

pub(crate) struct PreparedRoute {
    request: RequestEnvelope,
    definition: RouteDefinition,
}

impl PreparedRoute {
    pub(crate) fn request_id(&self) -> u64 {
        self.request.id
    }

    pub(crate) fn mode(&self) -> ExecutionMode {
        self.definition.mode
    }

    pub(crate) fn execute(&self, state: &mut core::State) -> ServerMessage {
        (self.definition.handler)(state, &self.request)
    }

    #[cfg(test)]
    pub(crate) fn for_test(request_id: u64, mode: ExecutionMode, handler: RouteHandler) -> Self {
        Self {
            request: RequestEnvelope {
                id: request_id,
                domain: Domain::Workspace,
                action: "test".to_owned(),
                params: serde_json::Value::Null,
            },
            definition: RouteDefinition::new(handler, mode),
        }
    }
}

pub(super) struct RouteDefinition {
    handler: RouteHandler,
    mode: ExecutionMode,
}

impl RouteDefinition {
    pub(super) const fn snapshot(handler: RouteHandler) -> Self {
        Self::new(handler, ExecutionMode::Snapshot)
    }

    pub(super) const fn external(handler: RouteHandler) -> Self {
        Self::new(handler, ExecutionMode::External)
    }

    pub(super) const fn live(handler: RouteHandler) -> Self {
        Self::new(handler, ExecutionMode::Live)
    }

    pub(super) const fn active_workspace(handler: RouteHandler) -> Self {
        Self::new(
            handler,
            ExecutionMode::Persistent(PersistenceMode::ActiveWorkspace),
        )
    }

    pub(super) const fn full(handler: RouteHandler) -> Self {
        Self::new(handler, ExecutionMode::Persistent(PersistenceMode::Full))
    }

    pub(super) const fn settings(handler: RouteHandler) -> Self {
        Self::new(
            handler,
            ExecutionMode::Persistent(PersistenceMode::Settings),
        )
    }

    pub(super) const fn persistence_barrier(handler: RouteHandler) -> Self {
        Self::new(handler, ExecutionMode::Persistent(PersistenceMode::Barrier))
    }

    const fn new(handler: RouteHandler, mode: ExecutionMode) -> Self {
        Self { handler, mode }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionMode {
    Snapshot,
    External,
    Live,
    Persistent(PersistenceMode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PersistenceMode {
    ActiveWorkspace,
    Barrier,
    Full,
    Settings,
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
    fn execution_modes_are_declared_by_the_routed_operation() {
        assert_modes(Domain::Workspace, &["list"], ExecutionMode::Snapshot);
        assert_modes(
            Domain::Workspace,
            &["flush"],
            ExecutionMode::Persistent(PersistenceMode::Barrier),
        );
        assert_modes(
            Domain::Workspace,
            &["activate"],
            ExecutionMode::Persistent(PersistenceMode::ActiveWorkspace),
        );
        assert_modes(
            Domain::Workspace,
            &["open", "close"],
            ExecutionMode::Persistent(PersistenceMode::Full),
        );
        assert_modes(
            Domain::Pane,
            &["split", "activate", "move", "resize"],
            ExecutionMode::Persistent(PersistenceMode::Full),
        );
        assert_modes(
            Domain::Tab,
            &["open", "activate", "setKind", "close", "move", "split"],
            ExecutionMode::Persistent(PersistenceMode::Full),
        );
        assert_modes(
            Domain::FileTree,
            &[
                "get",
                "getChildren",
                "createEntry",
                "renameEntry",
                "moveEntries",
                "copyEntries",
                "deleteEntries",
                "resolvePath",
            ],
            ExecutionMode::External,
        );
        assert_modes(
            Domain::FileTree,
            &["setExpandedPaths"],
            ExecutionMode::Persistent(PersistenceMode::Full),
        );
        assert_modes(
            Domain::Editor,
            &["openTab"],
            ExecutionMode::Persistent(PersistenceMode::Full),
        );
        assert_modes(
            Domain::Editor,
            &["document", "save"],
            ExecutionMode::External,
        );
        assert_modes(
            Domain::Git,
            &[
                "init",
                "status",
                "diff",
                "stagePaths",
                "unstagePaths",
                "stageAll",
                "unstageAll",
                "commit",
                "switchBranch",
                "trackRemoteBranch",
                "createBranch",
                "deleteBranch",
                "fetch",
                "pull",
                "push",
                "stash",
                "stashStaged",
                "stashes",
                "applyStash",
                "dropStash",
                "remotes",
                "addRemote",
                "removeRemote",
                "tags",
                "createTag",
                "deleteTag",
                "discardAll",
                "discardStaged",
            ],
            ExecutionMode::External,
        );
        assert_modes(
            Domain::Git,
            &["openDiffTab"],
            ExecutionMode::Persistent(PersistenceMode::Full),
        );
        assert_modes(
            Domain::Terminal,
            &["open", "read", "write", "resize", "restart"],
            ExecutionMode::Live,
        );
        assert_modes(Domain::Terminal, &["shells"], ExecutionMode::Snapshot);
        assert_modes(Domain::Settings, &["get"], ExecutionMode::Snapshot);
        assert_modes(
            Domain::Settings,
            &["update"],
            ExecutionMode::Persistent(PersistenceMode::Settings),
        );
    }

    fn assert_modes(domain: Domain, actions: &[&str], expected: ExecutionMode) {
        for action in actions {
            assert_eq!(
                mode_for(domain, action),
                expected,
                "unexpected {action} mode"
            );
        }
    }

    fn mode_for(domain: Domain, action: &str) -> ExecutionMode {
        let request = RequestEnvelope {
            id: 1,
            domain,
            action: action.to_owned(),
            params: serde_json::Value::Null,
        };

        prepare(request).expect("route should exist").mode()
    }
}

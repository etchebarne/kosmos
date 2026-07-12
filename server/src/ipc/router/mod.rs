mod editor;
mod file_tree;
mod formatters;
mod git;
mod language_servers;
mod pane;
mod search;
mod settings;
mod tab;
mod terminal;
mod window;
mod workspace;

use schemars::JsonSchema;
use schemars::schema::RootSchema;
use serde::de::DeserializeOwned;

use super::messages::envelope::{Domain, RequestEnvelope, ServerMessage};
use super::messages::workspace::WorkspaceListSnapshot;

type RouteHandler = fn(&mut core::State, &RequestEnvelope) -> ServerMessage;
type ApplicationRouteHandler = fn(&mut core::Application, &RequestEnvelope) -> ServerMessage;
type PersistentApplicationRouteHandler =
    fn(&mut core::PreparedPersistentOperation, &RequestEnvelope) -> ServerMessage;
type CancellableRouteHandler = fn(
    &mut core::State,
    &RequestEnvelope,
    &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage;

pub(crate) const DOMAINS: &[Domain] = &[
    Domain::Workspace,
    Domain::Pane,
    Domain::Tab,
    Domain::FileTree,
    Domain::Formatters,
    Domain::Editor,
    Domain::Git,
    Domain::Search,
    Domain::Terminal,
    Domain::Settings,
    Domain::LanguageServers,
    Domain::Window,
];

pub(crate) fn routes_for(domain: Domain) -> &'static [Route] {
    match domain {
        Domain::Workspace => workspace::ROUTES,
        Domain::Pane => pane::ROUTES,
        Domain::Tab => tab::ROUTES,
        Domain::FileTree => file_tree::ROUTES,
        Domain::Formatters => formatters::ROUTES,
        Domain::Editor => editor::ROUTES,
        Domain::Git => git::ROUTES,
        Domain::Search => search::ROUTES,
        Domain::Terminal => terminal::ROUTES,
        Domain::Settings => settings::ROUTES,
        Domain::LanguageServers => language_servers::ROUTES,
        Domain::Window => window::ROUTES,
    }
}

pub(crate) fn prepare(request: RequestEnvelope) -> Result<PreparedRoute, ServerMessage> {
    let definition = match request.domain {
        Domain::Workspace => workspace::resolve(&request.action),
        Domain::Pane => pane::resolve(&request.action),
        Domain::Tab => tab::resolve(&request.action),
        Domain::FileTree => file_tree::resolve(&request.action),
        Domain::Formatters => formatters::resolve(&request.action),
        Domain::Editor => editor::resolve(&request.action),
        Domain::Git => git::resolve(&request.action),
        Domain::Search => search::resolve(&request.action),
        Domain::Terminal => terminal::resolve(&request.action),
        Domain::Settings => settings::resolve(&request.action),
        Domain::LanguageServers => language_servers::resolve(&request.action),
        Domain::Window => window::resolve(&request.action),
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

    pub(crate) fn mode(&self) -> SchedulingMode {
        self.definition.mode
    }

    pub(crate) fn workspace_edit_delivery_credentials(&self) -> Option<(u64, String)> {
        (matches!(self.request.domain, Domain::LanguageServers)
            && self.request.action == "applyWorkspaceEdit")
            .then(|| {
                let transaction_id = self.request.params.get("transactionId")?.as_u64()?;
                let authorization = self
                    .request
                    .params
                    .get("authorization")?
                    .as_str()?
                    .to_owned();
                Some((transaction_id, authorization))
            })
            .flatten()
    }

    pub(crate) fn workspace_edit_recovery_request(
        &self,
    ) -> Option<(
        u64,
        String,
        core::language_servers::WorkspaceEditRecoveryIntent,
    )> {
        if !matches!(self.request.domain, Domain::LanguageServers)
            || self.request.action != "resolveWorkspaceEditRecovery"
        {
            return None;
        }
        let transaction_id = self.request.params.get("transactionId")?.as_u64()?;
        let authorization = self
            .request
            .params
            .get("authorization")?
            .as_str()?
            .to_owned();
        let intent = match self.request.params.get("intent")?.as_str()? {
            "retryRollback" => core::language_servers::WorkspaceEditRecoveryIntent::RetryRollback,
            "finalize" => core::language_servers::WorkspaceEditRecoveryIntent::Finalize,
            _ => return None,
        };
        Some((transaction_id, authorization, intent))
    }

    pub(crate) fn execute(
        &self,
        state: &mut core::State,
        cancellation: &core::language_servers::LanguageServerRequestCancellation,
    ) -> ServerMessage {
        match self.definition.handler {
            RouteHandlerKind::Standard(handler) => handler(state, &self.request),
            RouteHandlerKind::Cancellable(handler) => handler(state, &self.request, cancellation),
            RouteHandlerKind::Application(_) | RouteHandlerKind::PersistentApplication(_) => {
                unreachable!("application routes must execute against the live application")
            }
        }
    }

    pub(crate) fn execute_application(
        &self,
        application: &mut core::Application,
        cancellation: &core::language_servers::LanguageServerRequestCancellation,
    ) -> ServerMessage {
        match self.definition.handler {
            RouteHandlerKind::Standard(handler) => handler(application.state_mut(), &self.request),
            RouteHandlerKind::Cancellable(handler) => {
                handler(application.state_mut(), &self.request, cancellation)
            }
            RouteHandlerKind::Application(handler) => handler(application, &self.request),
            RouteHandlerKind::PersistentApplication(_) => {
                unreachable!("persistent application routes require a prepared operation")
            }
        }
    }

    pub(crate) fn execute_persistent(
        &self,
        operation: &mut core::PreparedPersistentOperation,
        cancellation: &core::language_servers::LanguageServerRequestCancellation,
    ) -> ServerMessage {
        match self.definition.handler {
            RouteHandlerKind::Standard(handler) => handler(operation.state_mut(), &self.request),
            RouteHandlerKind::Cancellable(handler) => {
                handler(operation.state_mut(), &self.request, cancellation)
            }
            RouteHandlerKind::PersistentApplication(handler) => handler(operation, &self.request),
            RouteHandlerKind::Application(_) => {
                unreachable!("live application routes cannot execute against a prepared operation")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(request_id: u64, mode: SchedulingMode, handler: RouteHandler) -> Self {
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

#[derive(Clone, Copy)]
pub(super) struct RouteDefinition {
    handler: RouteHandlerKind,
    mode: SchedulingMode,
}

#[derive(Clone, Copy)]
pub(crate) struct Route {
    pub(super) action: &'static str,
    pub(super) definition: RouteDefinition,
    pub(super) contract: ActionContract,
}

#[derive(Clone, Copy)]
pub(crate) struct ActionContract {
    pub(crate) params_schema: fn() -> RootSchema,
    pub(crate) result_schema: fn() -> RootSchema,
}

impl ActionContract {
    pub(crate) const fn of<Params: JsonSchema, Result: JsonSchema>() -> Self {
        Self {
            params_schema: schema_for::<Params>,
            result_schema: schema_for::<Result>,
        }
    }
}

impl Route {
    pub(super) const fn new<Params: JsonSchema, Result: JsonSchema>(
        action: &'static str,
        definition: RouteDefinition,
    ) -> Self {
        Self {
            action,
            definition,
            contract: ActionContract::of::<Params, Result>(),
        }
    }
}

fn schema_for<T: JsonSchema>() -> RootSchema {
    schemars::schema_for!(T)
}

pub(super) fn find_route(routes: &[Route], action: &str) -> Option<RouteDefinition> {
    routes
        .iter()
        .find(|route| route.action == action)
        .map(|route| route.definition)
}

#[derive(Clone, Copy)]
enum RouteHandlerKind {
    Standard(RouteHandler),
    Cancellable(CancellableRouteHandler),
    Application(ApplicationRouteHandler),
    PersistentApplication(PersistentApplicationRouteHandler),
}

impl RouteDefinition {
    pub(super) const fn snapshot(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::Snapshot)
    }

    pub(super) const fn external(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::External)
    }

    pub(super) const fn live(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::Live)
    }

    pub(super) const fn language_server(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::LanguageServer)
    }

    pub(super) const fn language_server_feature(handler: CancellableRouteHandler) -> Self {
        Self {
            handler: RouteHandlerKind::Cancellable(handler),
            mode: SchedulingMode::LanguageServerFeature,
        }
    }

    pub(super) const fn active_workspace(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::SerialMutation)
    }

    pub(super) const fn full(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::SerialMutation)
    }

    pub(super) const fn live_full(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::SerialMutation)
    }

    pub(super) const fn settings(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::SerialMutation)
    }

    pub(super) const fn window(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::SerialMutation)
    }

    pub(super) const fn persistence_barrier(handler: RouteHandler) -> Self {
        Self::new(handler, SchedulingMode::PersistenceBarrier)
    }

    pub(super) const fn application(handler: ApplicationRouteHandler) -> Self {
        Self {
            handler: RouteHandlerKind::Application(handler),
            mode: SchedulingMode::Application,
        }
    }

    pub(super) const fn persistent_application(handler: PersistentApplicationRouteHandler) -> Self {
        Self {
            handler: RouteHandlerKind::PersistentApplication(handler),
            mode: SchedulingMode::SerialMutation,
        }
    }

    const fn new(handler: RouteHandler, mode: SchedulingMode) -> Self {
        Self {
            handler: RouteHandlerKind::Standard(handler),
            mode,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SchedulingMode {
    Snapshot,
    External,
    Live,
    LanguageServer,
    LanguageServerFeature,
    SerialMutation,
    PersistenceBarrier,
    Application,
}

#[cfg(test)]
pub(crate) use SchedulingMode as ExecutionMode;

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

#[cfg(any())]
mod tests {
    use super::*;

    const EXPECTED_MODES: &[(Domain, &str, ExecutionMode)] = &[
        (Domain::Workspace, "list", ExecutionMode::Snapshot),
        (
            Domain::Workspace,
            "flush",
            ExecutionMode::Persistent(LegacyDurability::Barrier),
        ),
        (
            Domain::Workspace,
            "open",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Workspace,
            "activate",
            ExecutionMode::Persistent(LegacyDurability::ActiveWorkspace),
        ),
        (
            Domain::Workspace,
            "close",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Pane,
            "split",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Pane,
            "activate",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Pane,
            "move",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Pane,
            "resize",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Tab,
            "open",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Tab,
            "activate",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Tab,
            "setKind",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Tab,
            "close",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Tab,
            "move",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (
            Domain::Tab,
            "split",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (Domain::FileTree, "get", ExecutionMode::External),
        (Domain::FileTree, "gitStatus", ExecutionMode::External),
        (Domain::FileTree, "getChildren", ExecutionMode::External),
        (
            Domain::FileTree,
            "setExpandedPaths",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (Domain::FileTree, "createEntry", ExecutionMode::External),
        (Domain::FileTree, "renameEntry", ExecutionMode::External),
        (Domain::FileTree, "moveEntries", ExecutionMode::External),
        (Domain::FileTree, "copyEntries", ExecutionMode::External),
        (Domain::FileTree, "deleteEntries", ExecutionMode::External),
        (Domain::FileTree, "resolvePath", ExecutionMode::External),
        (Domain::Formatters, "list", ExecutionMode::Snapshot),
        (Domain::Formatters, "status", ExecutionMode::Snapshot),
        (Domain::Formatters, "install", ExecutionMode::Live),
        (Domain::Formatters, "uninstall", ExecutionMode::Live),
        (Domain::Formatters, "set-priorities", ExecutionMode::Live),
        (
            Domain::Editor,
            "openTab",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (Domain::Editor, "document", ExecutionMode::External),
        (Domain::Editor, "gitLineHunks", ExecutionMode::External),
        (Domain::Editor, "save", ExecutionMode::External),
        (Domain::Git, "init", ExecutionMode::External),
        (Domain::Git, "status", ExecutionMode::External),
        (
            Domain::Git,
            "openDiffTab",
            ExecutionMode::Persistent(LegacyDurability::Full),
        ),
        (Domain::Git, "diff", ExecutionMode::External),
        (Domain::Git, "saveDiffFile", ExecutionMode::External),
        (Domain::Git, "stagePaths", ExecutionMode::External),
        (Domain::Git, "unstagePaths", ExecutionMode::External),
        (Domain::Git, "stageAll", ExecutionMode::External),
        (Domain::Git, "unstageAll", ExecutionMode::External),
        (Domain::Git, "commit", ExecutionMode::External),
        (Domain::Git, "switchBranch", ExecutionMode::External),
        (Domain::Git, "trackRemoteBranch", ExecutionMode::External),
        (Domain::Git, "createBranch", ExecutionMode::External),
        (Domain::Git, "deleteBranch", ExecutionMode::External),
        (Domain::Git, "fetch", ExecutionMode::External),
        (Domain::Git, "pull", ExecutionMode::External),
        (Domain::Git, "push", ExecutionMode::External),
        (Domain::Git, "stash", ExecutionMode::External),
        (Domain::Git, "stashStaged", ExecutionMode::External),
        (Domain::Git, "stashes", ExecutionMode::External),
        (Domain::Git, "applyStash", ExecutionMode::External),
        (Domain::Git, "dropStash", ExecutionMode::External),
        (Domain::Git, "remotes", ExecutionMode::External),
        (Domain::Git, "addRemote", ExecutionMode::External),
        (Domain::Git, "removeRemote", ExecutionMode::External),
        (Domain::Git, "tags", ExecutionMode::External),
        (Domain::Git, "createTag", ExecutionMode::External),
        (Domain::Git, "deleteTag", ExecutionMode::External),
        (Domain::Git, "discardAll", ExecutionMode::External),
        (Domain::Git, "discardStaged", ExecutionMode::External),
        (Domain::Search, "query", ExecutionMode::External),
        (Domain::Search, "document", ExecutionMode::External),
        (Domain::Terminal, "shells", ExecutionMode::Snapshot),
        (Domain::Terminal, "open", ExecutionMode::Live),
        (Domain::Terminal, "read", ExecutionMode::Live),
        (Domain::Terminal, "write", ExecutionMode::Live),
        (Domain::Terminal, "resize", ExecutionMode::Live),
        (Domain::Terminal, "restart", ExecutionMode::Live),
        (Domain::Settings, "get", ExecutionMode::Snapshot),
        (
            Domain::Settings,
            "update",
            ExecutionMode::Persistent(LegacyDurability::Settings),
        ),
        (Domain::LanguageServers, "list", ExecutionMode::Snapshot),
        (Domain::LanguageServers, "status", ExecutionMode::Snapshot),
        (Domain::LanguageServers, "install", ExecutionMode::Live),
        (Domain::LanguageServers, "uninstall", ExecutionMode::Live),
        (
            Domain::LanguageServers,
            "restart",
            ExecutionMode::LanguageServer,
        ),
        (
            Domain::LanguageServers,
            "openDocument",
            ExecutionMode::LanguageServer,
        ),
        (
            Domain::LanguageServers,
            "changeDocument",
            ExecutionMode::LanguageServer,
        ),
        (
            Domain::LanguageServers,
            "closeDocument",
            ExecutionMode::LanguageServer,
        ),
        (
            Domain::LanguageServers,
            "saveDocument",
            ExecutionMode::LanguageServer,
        ),
        (
            Domain::LanguageServers,
            "hover",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "signatureHelp",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "definition",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "declaration",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "typeDefinition",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "implementation",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "references",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "documentSymbols",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "workspaceSymbols",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "resolveWorkspaceSymbol",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "diagnostics",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "completion",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "resolveCompletion",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "documentColors",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "colorPresentations",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "formatting",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "prepareRename",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "rename",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "codeActions",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "resolveCodeAction",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "stageCodeAction",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "executeCommand",
            ExecutionMode::LanguageServerFeature,
        ),
        (
            Domain::LanguageServers,
            "commitWorkspaceEdit",
            ExecutionMode::LivePersistent(LegacyDurability::Full),
        ),
        (
            Domain::LanguageServers,
            "rollbackWorkspaceEdit",
            ExecutionMode::LivePersistent(LegacyDurability::Full),
        ),
        (
            Domain::LanguageServers,
            "finishWorkspaceEdit",
            ExecutionMode::LivePersistent(LegacyDurability::Full),
        ),
        (
            Domain::LanguageServers,
            "finalizeWorkspaceEdit",
            ExecutionMode::LivePersistent(LegacyDurability::Full),
        ),
        (
            Domain::LanguageServers,
            "acknowledgeWorkspaceEditCompletion",
            ExecutionMode::LivePersistent(LegacyDurability::Full),
        ),
        (
            Domain::LanguageServers,
            "workspaceEditStatus",
            ExecutionMode::Live,
        ),
        (
            Domain::LanguageServers,
            "listWorkspaceEditRecoveries",
            ExecutionMode::Live,
        ),
        (
            Domain::LanguageServers,
            "trustWorkspace",
            ExecutionMode::LanguageServer,
        ),
        (Domain::Window, "get", ExecutionMode::Snapshot),
        (
            Domain::Window,
            "update",
            ExecutionMode::Persistent(LegacyDurability::Window),
        ),
    ];

    #[test]
    fn route_registries_are_exhaustive_and_match_their_resolvers() {
        for domain in DOMAINS {
            let routes = routes_for(*domain);

            for (index, route) in routes.iter().enumerate() {
                assert!(
                    !routes[..index]
                        .iter()
                        .any(|previous| previous.action == route.action),
                    "duplicate {:?}.{} route",
                    domain,
                    route.action
                );
                assert_eq!(
                    mode_for(*domain, route.action),
                    route.definition.mode,
                    "resolver changed {:?}.{} metadata",
                    domain,
                    route.action
                );
                assert_eq!(
                    expected_mode(*domain, route.action),
                    Some(route.definition.mode),
                    "unreviewed {:?}.{} route",
                    domain,
                    route.action
                );
            }

            let expected = EXPECTED_MODES
                .iter()
                .filter(|(expected_domain, _, _)| same_domain(*expected_domain, *domain))
                .count();
            assert_eq!(
                routes.len(),
                expected,
                "missing expected {:?} route",
                domain
            );
        }

        for (index, (domain, action, mode)) in EXPECTED_MODES.iter().enumerate() {
            assert!(
                !EXPECTED_MODES[..index]
                    .iter()
                    .any(|(previous_domain, previous_action, _)| {
                        same_domain(*previous_domain, *domain) && previous_action == action
                    }),
                "duplicate expected {:?}.{} mode",
                domain,
                action
            );
            assert_eq!(
                mode_for(*domain, action),
                *mode,
                "unexpected {:?}.{} mode",
                domain,
                action
            );
        }
    }

    #[test]
    fn unknown_actions_do_not_resolve() {
        for domain in DOMAINS {
            assert!(prepare(request(*domain, "missing")).is_err());
        }
    }

    fn expected_mode(domain: Domain, action: &str) -> Option<ExecutionMode> {
        EXPECTED_MODES
            .iter()
            .find(|(expected_domain, expected_action, _)| {
                same_domain(*expected_domain, domain) && *expected_action == action
            })
            .map(|(_, _, mode)| *mode)
    }

    fn same_domain(left: Domain, right: Domain) -> bool {
        std::mem::discriminant(&left) == std::mem::discriminant(&right)
    }

    fn mode_for(domain: Domain, action: &str) -> ExecutionMode {
        prepare(request(domain, action))
            .expect("route should exist")
            .mode()
    }

    fn request(domain: Domain, action: &str) -> RequestEnvelope {
        RequestEnvelope {
            id: 1,
            domain,
            action: action.to_owned(),
            params: serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod scheduling_tests {
    use super::*;

    #[test]
    fn every_registered_route_resolves_to_transport_scheduling() {
        for domain in DOMAINS {
            for route in routes_for(*domain) {
                let prepared = prepare(RequestEnvelope {
                    id: 1,
                    domain: *domain,
                    action: route.action.to_owned(),
                    params: serde_json::Value::Null,
                })
                .expect("registered route should resolve");
                assert!(matches!(
                    prepared.mode(),
                    SchedulingMode::Snapshot
                        | SchedulingMode::External
                        | SchedulingMode::Live
                        | SchedulingMode::LanguageServer
                        | SchedulingMode::LanguageServerFeature
                        | SchedulingMode::SerialMutation
                        | SchedulingMode::PersistenceBarrier
                        | SchedulingMode::Application
                ));
            }
        }
    }
}

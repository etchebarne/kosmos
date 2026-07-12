use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use crate::ipc::messages::envelope::ServerMessage;
use crate::ipc::router::{PreparedRoute, SchedulingMode};

use super::notifications::{NotificationHub, NotificationSubscription};
use super::response::ResponseSender;

const MAX_QUEUED_REQUESTS: usize = 64;
const LANGUAGE_SERVER_FEATURE_WORKERS: usize = 4;

#[derive(Clone)]
pub(crate) struct Dispatcher {
    application: Arc<Mutex<core::Application>>,
    #[cfg(test)]
    state: Arc<Mutex<core::Application>>,
    external_requests: mpsc::SyncSender<ExternalRequest>,
    language_server_requests: mpsc::SyncSender<ExternalRequest>,
    language_server_feature_requests: mpsc::SyncSender<ExternalRequest>,
    persistent_requests: mpsc::SyncSender<PersistentRequest>,
    notifications: NotificationHub,
}

impl Dispatcher {
    pub(crate) fn from_application(application: core::Application) -> io::Result<Self> {
        let initial_workspaces = application.state().workspaces().clone();
        let notifications = NotificationHub::default();
        let (workspace_reconciler, workspace_changes) = match core::WorkspaceChangeWatcher::new() {
            Ok((watcher, changes)) => {
                match WorkspaceWatcherReconciler::new(watcher, notifications.clone()) {
                    Ok(reconciler) => (Some(reconciler), Some(changes)),
                    Err(error) => {
                        eprintln!("workspace watcher worker failed to start: {error}");
                        (None, None)
                    }
                }
            }
            Err(error) => {
                eprintln!("workspace watcher is unavailable: {error}");
                (None, None)
            }
        };
        let application = Arc::new(Mutex::new(application));
        notifications.attach_application(Arc::downgrade(&application));
        application
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .set_event_sink(Arc::new(notifications.clone()));
        let (external_requests, external_receiver) = mpsc::sync_channel(MAX_QUEUED_REQUESTS);
        let (language_server_requests, language_server_receiver) =
            mpsc::sync_channel(MAX_QUEUED_REQUESTS);
        let (language_server_feature_requests, language_server_feature_receiver) =
            mpsc::sync_channel(MAX_QUEUED_REQUESTS);
        let (persistent_requests, persistent_receiver) = mpsc::sync_channel(MAX_QUEUED_REQUESTS);

        spawn_external_worker(external_receiver)?;
        spawn_language_server_worker(language_server_receiver)?;
        spawn_language_server_feature_workers(language_server_feature_receiver)?;
        spawn_persistence_worker(
            Arc::clone(&application),
            persistent_receiver,
            workspace_reconciler.clone(),
        )?;
        if let Some(workspace_changes) = workspace_changes
            && let Err(error) = spawn_notification_worker(
                workspace_changes,
                notifications.clone(),
                workspace_reconciler.clone(),
            )
        {
            eprintln!("workspace notification worker failed to start: {error}");
        }
        if let Some(workspace_reconciler) = workspace_reconciler {
            workspace_reconciler.reconcile(initial_workspaces);
        }

        Ok(Self {
            application: Arc::clone(&application),
            #[cfg(test)]
            state: application,
            external_requests,
            language_server_requests,
            language_server_feature_requests,
            persistent_requests,
            notifications,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(state: core::State, store: core::DurableStore) -> io::Result<Self> {
        Self::from_application(core::Application::new(state, store))
    }

    pub(crate) fn subscribe(&self, responses: ResponseSender) -> NotificationSubscription {
        self.notifications.subscribe(responses)
    }

    pub(crate) fn acknowledge_apply_edit(
        &self,
        renderer_id: u64,
        id: u64,
        token: &str,
        applied: bool,
        failure_reason: Option<String>,
    ) {
        self.notifications
            .acknowledge_apply_edit(renderer_id, id, token, applied, failure_reason);
    }

    #[cfg(test)]
    pub(crate) fn dispatch(
        &self,
        route: PreparedRoute,
        responses: ResponseSender,
    ) -> Option<mpsc::Receiver<()>> {
        self.dispatch_cancellable(
            route,
            responses,
            core::language_servers::LanguageServerRequestCancellation::new(),
            None,
        )
    }

    pub(crate) fn dispatch_cancellable(
        &self,
        route: PreparedRoute,
        responses: ResponseSender,
        cancellation: core::language_servers::LanguageServerRequestCancellation,
        _owner: Option<()>,
    ) -> Option<mpsc::Receiver<()>> {
        if cancellation.is_cancelled() {
            responses.send(request_cancelled(route.request_id()));
            return None;
        }
        if let Some((transaction_id, authorization)) = route.workspace_edit_delivery_credentials() {
            let response = match self
                .notifications
                .request_staged_workspace_edit(transaction_id, &authorization)
            {
                Ok(result) if result.applied => ServerMessage::ok(route.request_id(), true),
                Ok(result) => ServerMessage::error(
                    route.request_id(),
                    "workspace_edit.rejected",
                    result
                        .failure_reason
                        .unwrap_or_else(|| "workspace edit was not applied".to_owned()),
                ),
                Err(error) => workspace_edit_error(route.request_id(), error),
            };
            responses.send(response);
            return None;
        }
        if let Some((transaction_id, authorization, intent)) =
            route.workspace_edit_recovery_request()
        {
            let response = self
                .application
                .lock()
                .map_err(|_| {
                    core::language_servers::WorkspaceEditError::Invalid(
                        "workspace edit application is unavailable".to_owned(),
                    )
                })
                .and_then(|mut application| {
                    application.resolve_workspace_edit_recovery(
                        transaction_id,
                        &authorization,
                        intent,
                    )
                })
                .map(crate::ipc::messages::language_servers::WorkspaceEditTransactionStatusPayload::from_core)
                .map(|status| ServerMessage::ok(route.request_id(), status))
                .unwrap_or_else(|error| workspace_edit_error(route.request_id(), error));
            responses.send(response);
            return None;
        }

        match route.mode() {
            SchedulingMode::Snapshot => {
                responses.send(execute_snapshot(&self.application, &route, &cancellation));
                None
            }
            SchedulingMode::Live => {
                responses.send(execute_live(&self.application, &route, &cancellation));
                None
            }
            SchedulingMode::Application => {
                responses.send(execute_application(
                    &self.application,
                    &route,
                    &cancellation,
                ));
                None
            }
            SchedulingMode::External => {
                let (completion, completed) = mpsc::channel();
                let operation =
                    match prepare_external_operation(&self.application, route.request_id()) {
                        Ok(candidate) => candidate,
                        Err(response) => {
                            responses.send(response);
                            return None;
                        }
                    };

                queue_external_request(
                    &self.external_requests,
                    ExternalRequest {
                        route,
                        responses,
                        operation,
                        completion,
                        cancellation,
                    },
                );
                Some(completed)
            }
            SchedulingMode::LanguageServer => {
                let (completion, completed) = mpsc::channel();
                let operation =
                    match prepare_external_operation(&self.application, route.request_id()) {
                        Ok(candidate) => candidate,
                        Err(response) => {
                            responses.send(response);
                            return None;
                        }
                    };

                queue_external_request(
                    &self.language_server_requests,
                    ExternalRequest {
                        route,
                        responses,
                        operation,
                        completion,
                        cancellation,
                    },
                );
                Some(completed)
            }
            SchedulingMode::LanguageServerFeature => {
                let (completion, completed) = mpsc::channel();
                let operation =
                    match prepare_external_operation(&self.application, route.request_id()) {
                        Ok(candidate) => candidate,
                        Err(response) => {
                            responses.send(response);
                            return None;
                        }
                    };

                queue_external_request(
                    &self.language_server_feature_requests,
                    ExternalRequest {
                        route,
                        responses,
                        operation,
                        completion,
                        cancellation,
                    },
                );
                Some(completed)
            }
            SchedulingMode::SerialMutation | SchedulingMode::PersistenceBarrier => {
                let (completion, completed) = mpsc::channel();
                queue_persistent_request(
                    &self.persistent_requests,
                    PersistentRequest {
                        route,
                        responses,
                        completion,
                        cancellation,
                    },
                );
                Some(completed)
            }
        }
    }
}

struct ExternalRequest {
    route: PreparedRoute,
    responses: ResponseSender,
    operation: core::PreparedExternalOperation,
    completion: mpsc::Sender<()>,
    cancellation: core::language_servers::LanguageServerRequestCancellation,
}

struct PersistentRequest {
    route: PreparedRoute,
    responses: ResponseSender,
    completion: mpsc::Sender<()>,
    cancellation: core::language_servers::LanguageServerRequestCancellation,
}

#[derive(Clone)]
struct WorkspaceWatcherReconciler {
    state: Arc<Mutex<WorkspaceWatcherState>>,
    signal: mpsc::SyncSender<()>,
}

#[derive(Default)]
struct WorkspaceWatcherState {
    notify_after_reconcile: bool,
    workspaces: Option<core::tree::WorkspaceList>,
}

impl WorkspaceWatcherReconciler {
    fn new(
        mut watcher: core::WorkspaceChangeWatcher,
        notifications: NotificationHub,
    ) -> io::Result<Self> {
        let state = Arc::new(Mutex::new(WorkspaceWatcherState::default()));
        let (signal, reconciliations) = mpsc::sync_channel(1);
        let worker_state = Arc::clone(&state);

        thread::Builder::new()
            .name("kosmos-workspace-watcher".to_owned())
            .spawn(move || {
                while reconciliations.recv().is_ok() {
                    let Some((workspaces, notify_after_reconcile)) =
                        worker_state.lock().ok().and_then(|mut state| {
                            let workspaces = state.workspaces.clone()?;
                            let notify_after_reconcile = state.notify_after_reconcile;
                            state.notify_after_reconcile = false;
                            Some((workspaces, notify_after_reconcile))
                        })
                    else {
                        continue;
                    };

                    match watcher.reconcile(&workspaces) {
                        Ok(()) if notify_after_reconcile => notifications.workspace_changed(
                            workspaces
                                .workspaces()
                                .iter()
                                .map(|workspace| workspace.id().value())
                                .collect(),
                        ),
                        Ok(()) => {}
                        Err(error) => {
                            if notify_after_reconcile && let Ok(mut state) = worker_state.lock() {
                                state.notify_after_reconcile = true;
                            }
                            eprintln!("workspace watcher reconciliation failed: {error}");
                        }
                    }
                }
            })?;

        Ok(Self { state, signal })
    }

    fn reconcile(&self, workspaces: core::tree::WorkspaceList) {
        if let Ok(mut state) = self.state.lock() {
            if state.workspaces.as_ref() == Some(&workspaces) {
                return;
            }

            state.workspaces = Some(workspaces);
            state.notify_after_reconcile = true;
            let _ = self.signal.try_send(());
        }
    }

    fn retry(&self) {
        if self
            .state
            .lock()
            .is_ok_and(|state| state.workspaces.is_some())
        {
            let _ = self.signal.try_send(());
        }
    }
}

fn spawn_external_worker(requests: mpsc::Receiver<ExternalRequest>) -> io::Result<()> {
    thread::Builder::new()
        .name("kosmos-external-operations".to_owned())
        .spawn(move || {
            while let Ok(mut request) = requests.recv() {
                if request.cancellation.is_cancelled() {
                    request
                        .responses
                        .send(request_cancelled(request.route.request_id()));
                    let _ = request.completion.send(());
                    continue;
                }

                let response = execute_handler(
                    &request.route,
                    request.operation.state_mut(),
                    &request.cancellation,
                );
                request.responses.send(response);
                let _ = request.completion.send(());
            }
        })
        .map(|_| ())
}

fn spawn_language_server_worker(requests: mpsc::Receiver<ExternalRequest>) -> io::Result<()> {
    thread::Builder::new()
        .name("kosmos-language-server-operations".to_owned())
        .spawn(move || {
            while let Ok(mut request) = requests.recv() {
                if request.cancellation.is_cancelled() {
                    request
                        .responses
                        .send(request_cancelled(request.route.request_id()));
                    let _ = request.completion.send(());
                    continue;
                }

                let response = execute_handler(
                    &request.route,
                    request.operation.state_mut(),
                    &request.cancellation,
                );
                request.responses.send(response);
                let _ = request.completion.send(());
            }
        })
        .map(|_| ())
}

fn spawn_language_server_feature_workers(
    requests: mpsc::Receiver<ExternalRequest>,
) -> io::Result<()> {
    let requests = Arc::new(Mutex::new(requests));
    for index in 0..LANGUAGE_SERVER_FEATURE_WORKERS {
        let requests = Arc::clone(&requests);
        thread::Builder::new()
            .name(format!("kosmos-language-server-features-{index}"))
            .spawn(move || {
                loop {
                    let received = requests
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .recv();
                    let Ok(mut request) = received else {
                        break;
                    };
                    if request.cancellation.is_cancelled() {
                        request
                            .responses
                            .send(request_cancelled(request.route.request_id()));
                        let _ = request.completion.send(());
                        continue;
                    }

                    let response = execute_handler(
                        &request.route,
                        request.operation.state_mut(),
                        &request.cancellation,
                    );
                    request.responses.send(response);
                    let _ = request.completion.send(());
                }
            })?;
    }
    Ok(())
}

fn spawn_persistence_worker(
    application: Arc<Mutex<core::Application>>,
    requests: mpsc::Receiver<PersistentRequest>,
    workspace_reconciler: Option<WorkspaceWatcherReconciler>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("kosmos-persistent-operations".to_owned())
        .spawn(move || {
            while let Ok(request) = requests.recv() {
                if request.cancellation.is_cancelled() {
                    request
                        .responses
                        .send(request_cancelled(request.route.request_id()));
                    let _ = request.completion.send(());
                    continue;
                }

                let response = execute_persistent(
                    &application,
                    &request.route,
                    workspace_reconciler.as_ref(),
                    &request.cancellation,
                );

                request.responses.send(response);
                let _ = request.completion.send(());
            }
        })
        .map(|_| ())
}

fn spawn_notification_worker(
    workspace_changes: mpsc::Receiver<Vec<core::tree::WorkspaceId>>,
    notifications: NotificationHub,
    workspace_reconciler: Option<WorkspaceWatcherReconciler>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("kosmos-workspace-changes".to_owned())
        .spawn(move || {
            while let Ok(workspace_ids) = workspace_changes.recv() {
                notifications.workspace_changed(
                    workspace_ids
                        .into_iter()
                        .map(core::tree::WorkspaceId::value)
                        .collect(),
                );
                if let Some(workspace_reconciler) = &workspace_reconciler {
                    workspace_reconciler.retry();
                }
            }
        })
        .map(|_| ())
}

fn execute_snapshot(
    application: &Mutex<core::Application>,
    route: &PreparedRoute,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let mut operation = match prepare_external_operation(application, route.request_id()) {
        Ok(operation) => operation,
        Err(_) => return state_unavailable(route.request_id()),
    };

    execute_handler(route, operation.state_mut(), cancellation)
}

fn execute_live(
    application: &Mutex<core::Application>,
    route: &PreparedRoute,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let mut application = match application.lock() {
        Ok(application) => application,
        Err(_) => return state_unavailable(route.request_id()),
    };

    execute_handler(route, application.state_mut(), cancellation)
}

fn execute_application(
    application: &Mutex<core::Application>,
    route: &PreparedRoute,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    let mut application = match application.lock() {
        Ok(application) => application,
        Err(_) => return state_unavailable(route.request_id()),
    };
    catch_unwind(AssertUnwindSafe(|| {
        route.execute_application(&mut application, cancellation)
    }))
    .unwrap_or_else(|_| handler_panicked(route.request_id()))
}

fn execute_persistent(
    application: &Mutex<core::Application>,
    route: &PreparedRoute,
    workspace_reconciler: Option<&WorkspaceWatcherReconciler>,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    if matches!(route.mode(), SchedulingMode::PersistenceBarrier) {
        return execute_live(application, route, cancellation);
    }
    let mut operation = match application.lock() {
        Ok(mut application) => match application.prepare_persistent_operation() {
            Ok(operation) => operation,
            Err(core::ApplicationError::DurabilityInFlight) => {
                return state_conflict(route.request_id());
            }
            Err(error) => return persistence_error(route.request_id(), error),
        },
        Err(_) => return state_unavailable(route.request_id()),
    };
    let response = execute_persistent_handler(route, &mut operation, cancellation);

    if !response.is_ok() {
        abandon_persistent_operation(application);
        return response;
    }

    if let Err(error) = operation.persist() {
        abandon_persistent_operation(application);
        return persistence_error(route.request_id(), error);
    }

    let mut application = match application.lock() {
        Ok(application) => application,
        Err(_) => return state_unavailable(route.request_id()),
    };
    if let Err(error) = application.complete_persistent_operation(operation) {
        return persistence_error(route.request_id(), error);
    }

    let workspaces = application.state().workspaces().clone();
    drop(application);

    if let Some(workspace_reconciler) = workspace_reconciler {
        workspace_reconciler.reconcile(workspaces);
    }

    response
}

fn prepare_external_operation(
    application: &Mutex<core::Application>,
    request_id: u64,
) -> Result<core::PreparedExternalOperation, ServerMessage> {
    application
        .lock()
        .map(|application| application.prepare_external_operation())
        .map_err(|_| state_unavailable(request_id))
}

fn abandon_persistent_operation(application: &Mutex<core::Application>) {
    if let Ok(mut application) = application.lock() {
        application.abandon_persistent_operation();
    }
}

fn queue_external_request(requests: &mpsc::SyncSender<ExternalRequest>, request: ExternalRequest) {
    let request_id = request.route.request_id();

    match requests.try_send(request) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(request)) => {
            request.responses.send(ServerMessage::error(
                request_id,
                "ipc.worker_busy",
                "external worker queue is full",
            ));
            let _ = request.completion.send(());
        }
        Err(mpsc::TrySendError::Disconnected(request)) => {
            request.responses.send(ServerMessage::error(
                request_id,
                "ipc.worker_unavailable",
                "external worker is unavailable",
            ));
            let _ = request.completion.send(());
        }
    }
}

fn queue_persistent_request(
    requests: &mpsc::SyncSender<PersistentRequest>,
    request: PersistentRequest,
) {
    let request_id = request.route.request_id();

    match requests.try_send(request) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(request)) => {
            request.responses.send(ServerMessage::error(
                request_id,
                "ipc.worker_busy",
                "persistence worker queue is full",
            ));
            let _ = request.completion.send(());
        }
        Err(mpsc::TrySendError::Disconnected(request)) => {
            request.responses.send(ServerMessage::error(
                request_id,
                "ipc.worker_unavailable",
                "persistence worker is unavailable",
            ));
            let _ = request.completion.send(());
        }
    }
}

fn execute_handler(
    route: &PreparedRoute,
    state: &mut core::State,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    catch_unwind(AssertUnwindSafe(|| route.execute(state, cancellation)))
        .unwrap_or_else(|_| handler_panicked(route.request_id()))
}

fn execute_persistent_handler(
    route: &PreparedRoute,
    operation: &mut core::PreparedPersistentOperation,
    cancellation: &core::language_servers::LanguageServerRequestCancellation,
) -> ServerMessage {
    catch_unwind(AssertUnwindSafe(|| {
        route.execute_persistent(operation, cancellation)
    }))
    .unwrap_or_else(|_| handler_panicked(route.request_id()))
}

fn request_cancelled(request_id: u64) -> ServerMessage {
    ServerMessage::error(
        request_id,
        core::language_servers::LanguageServerError::RequestCancelled.code(),
        core::language_servers::LanguageServerError::RequestCancelled.to_string(),
    )
}

fn state_unavailable(request_id: u64) -> ServerMessage {
    ServerMessage::error(
        request_id,
        "ipc.state_unavailable",
        "IPC state mutex was poisoned",
    )
}

fn workspace_edit_error(
    request_id: u64,
    error: core::language_servers::WorkspaceEditError,
) -> ServerMessage {
    let code = match &error {
        core::language_servers::WorkspaceEditError::Recovery(_) => {
            "workspace_edit.recovery_required"
        }
        core::language_servers::WorkspaceEditError::Expired => "workspace_edit.expired",
        _ => "workspace_edit.invalid",
    };
    ServerMessage::error(request_id, code, error.to_string())
}

fn handler_panicked(request_id: u64) -> ServerMessage {
    ServerMessage::error(
        request_id,
        "ipc.handler_panicked",
        "IPC request handler panicked",
    )
}

fn state_conflict(request_id: u64) -> ServerMessage {
    ServerMessage::error(
        request_id,
        "persistence.state_conflict",
        "persistent state changed before the candidate could be saved",
    )
}

fn persistence_error(request_id: u64, error: core::ApplicationError) -> ServerMessage {
    let code = match error {
        core::ApplicationError::StalePreparedOperation => "persistence.state_conflict",
        core::ApplicationError::DurabilityInFlight => "persistence.state_conflict",
        core::ApplicationError::Persistence(_) => "persistence.save_failed",
        core::ApplicationError::Editor(_)
        | core::ApplicationError::EditorSession(_)
        | core::ApplicationError::CloseNotFound
        | core::ApplicationError::InvalidCloseDecision => "persistence.operation_failed",
    };
    ServerMessage::error(request_id, code, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Condvar, OnceLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::ipc::messages::envelope::{Domain, RequestEnvelope};
    use crate::ipc::router::{self, ExecutionMode};
    use crate::ipc::transport::response;

    #[derive(Default)]
    struct ExternalGate {
        release: bool,
        started: bool,
    }

    static EXTERNAL_GATE: OnceLock<(Mutex<ExternalGate>, Condvar)> = OnceLock::new();
    static LANGUAGE_SERVER_FEATURE_GATE: OnceLock<(Mutex<FeatureGate>, Condvar)> = OnceLock::new();
    static LANGUAGE_SERVER_FEATURE_TEST_LOCK: Mutex<()> = Mutex::new(());
    static CANCELLED_ROUTE_INVOCATIONS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Default)]
    struct FeatureGate {
        release: bool,
        started: usize,
    }

    #[test]
    fn persistence_failures_do_not_mutate_live_state() {
        let (store, path) = test_store("save-failure");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        std::fs::remove_file(&path).expect("database should be removed");
        std::fs::create_dir(&path).expect("database path should become a directory");
        let (responses, receiver) = test_response_channel();

        let _completion = dispatcher
            .dispatch(workspace_open_route(1, "/workspaces/main"), responses)
            .expect("persistent request should have a completion barrier");
        let response = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("persistence failure should respond");

        assert!(!response.is_ok());
        assert!(
            dispatcher
                .state
                .lock()
                .expect("state should lock")
                .workspaces()
                .is_empty()
        );

        let _ = std::fs::remove_dir(path);
    }

    #[test]
    fn persistence_failures_preserve_live_terminal_sessions() {
        let root = test_workspace("terminal-save-failure");
        let mut state = core::State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            core::tree::PaneId::new(1),
            core::tree::TabId::new(1),
            core::tree::TabKind::Terminal,
        ));
        state
            .open_terminal(Some(workspace_id), core::tree::TabId::new(1), 80, 24)
            .expect("terminal should open");
        let (store, path) = test_store("terminal-save-failure");
        let dispatcher = Dispatcher::new(state, store).expect("dispatcher should open");
        std::fs::remove_file(&path).expect("database should be removed");
        std::fs::create_dir(&path).expect("database path should become a directory");
        let (responses, receiver) = test_response_channel();
        let route = router::prepare(RequestEnvelope {
            id: 1,
            domain: Domain::Tab,
            action: "setKind".to_owned(),
            params: serde_json::json!({
                "workspaceId": workspace_id.value(),
                "paneId": 1,
                "tabId": 1,
                "kind": "search"
            }),
        })
        .expect("tab route should prepare");

        let _completion = dispatcher
            .dispatch(route, responses)
            .expect("persistent request should have a completion barrier");
        let response = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("persistence failure should respond");

        assert!(!response.is_ok());
        assert!(
            dispatcher
                .state
                .lock()
                .expect("state should lock")
                .read_terminal_output(Some(workspace_id), core::tree::TabId::new(1))
                .is_ok()
        );

        let _ = std::fs::remove_dir(path);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_requests_commit_in_queue_order() {
        let (store, path) = test_store("ordered-persistence");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        let (responses, receiver) = test_response_channel();

        let completions = [
            dispatcher
                .dispatch(
                    workspace_open_route(1, "/workspaces/first"),
                    responses.clone(),
                )
                .expect("persistent request should have a completion barrier"),
            dispatcher
                .dispatch(workspace_open_route(2, "/workspaces/second"), responses)
                .expect("persistent request should have a completion barrier"),
        ];

        for _ in 0..2 {
            assert!(
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .expect("persistent request should respond")
                    .is_ok()
            );
        }

        for completion in completions {
            completion
                .recv_timeout(Duration::from_secs(2))
                .expect("persistent completion should be signaled");
        }

        let live_workspace_count = dispatcher
            .state
            .lock()
            .expect("state should lock")
            .workspaces()
            .workspaces()
            .len();
        let persisted_workspace_count = core::DurableStore::open(&path)
            .expect("store should reopen")
            .load()
            .expect("state should load")
            .workspaces()
            .workspaces()
            .len();

        assert_eq!(live_workspace_count, 2);
        assert_eq!(persisted_workspace_count, 2);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn finished_workspace_edit_route_cannot_be_overwritten_by_a_later_full_state_save() {
        let root = test_workspace("workspace-edit-live-finish");
        let other = root.join("other-workspace");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        let (store, database) = test_store("workspace-edit-live-finish");
        let paths = core::language_servers::LanguageServerPaths::new(
            root.join("language-servers"),
            root.join("language-server-cache"),
        );
        let manager =
            core::language_servers::LanguageServerManager::open(paths.clone(), store.clone())
                .unwrap();
        let mut state = core::State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            core::tree::PaneId::new(1),
            core::tree::TabId::new(1),
            core::tree::TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), core::tree::TabId::new(1), "src/main.rs")
            .unwrap();
        let staged = manager
            .stage_workspace_edit(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "rename",
                    "oldUri": format!("file://{}", root.join("src").display()),
                    "newUri": format!("file://{}", root.join("renamed").display())
                }]}),
                &[core::language_servers::WorkspaceEditRoot {
                    workspace_id,
                    path: root.clone(),
                }],
            )
            .unwrap();
        state.attach_language_server_manager(manager);
        let dispatcher = Dispatcher::new(state, store.clone()).unwrap();
        let (responses, _receiver) = test_response_channel();

        for (id, action) in [(1, "commitWorkspaceEdit"), (2, "finishWorkspaceEdit")] {
            let route = router::prepare(RequestEnvelope {
                id,
                domain: Domain::LanguageServers,
                action: action.to_owned(),
                params: serde_json::json!({
                    "transactionId": staged.transaction_id,
                    "authorization": staged.authorization,
                }),
            })
            .unwrap();
            dispatcher
                .dispatch(route, responses.clone())
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
        }
        dispatcher
            .dispatch(workspace_open_route(3, other.to_str().unwrap()), responses)
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();

        let restarted =
            core::language_servers::LanguageServerManager::open(paths, store.clone()).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.editor_view_states()[0].path(), "renamed/main.rs");
        let recovery = restarted
            .workspace_edit_recoveries()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            recovery.status.phase,
            core::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn external_requests_release_state_and_do_not_block_snapshot_responses() {
        let (store, path) = test_store("external-locking");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        let (responses, receiver) = test_response_channel();
        reset_external_gate();

        let _external_completion = dispatcher
            .dispatch(
                router::PreparedRoute::for_test(
                    1,
                    ExecutionMode::External,
                    blocking_external_route,
                ),
                responses.clone(),
            )
            .expect("external request should have a completion barrier");
        wait_for_external_route();

        assert!(dispatcher.state.try_lock().is_ok());

        assert!(
            dispatcher
                .dispatch(
                    router::PreparedRoute::for_test(2, ExecutionMode::Snapshot, successful_route,),
                    responses.clone(),
                )
                .is_none()
        );
        assert!(
            dispatcher
                .dispatch(
                    router::PreparedRoute::for_test(3, ExecutionMode::Live, successful_route),
                    responses,
                )
                .is_none()
        );

        for _ in 0..2 {
            assert!(
                receiver
                    .recv_timeout(Duration::from_millis(250))
                    .expect("live and snapshot responses should bypass external work")
                    .is_ok()
            );
        }

        release_external_route();
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("external request should finish")
                .is_ok()
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn handler_panics_do_not_stop_external_worker() {
        let (store, path) = test_store("handler-panic");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        let (responses, receiver) = test_response_channel();

        let _completions = [
            dispatcher
                .dispatch(
                    router::PreparedRoute::for_test(1, ExecutionMode::External, panicking_route),
                    responses.clone(),
                )
                .expect("external request should have a completion barrier"),
            dispatcher
                .dispatch(
                    router::PreparedRoute::for_test(2, ExecutionMode::External, successful_route),
                    responses,
                )
                .expect("external request should have a completion barrier"),
        ];

        assert!(
            !receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("panic should return an error")
                .is_ok()
        );
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("worker should process the next request")
                .is_ok()
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn slow_language_server_feature_does_not_block_other_feature_responses() {
        let _test_lock = LANGUAGE_SERVER_FEATURE_TEST_LOCK.lock().unwrap();
        let (store, path) = test_store("language-server-feature-concurrency");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        let (responses, receiver) = test_response_channel();
        reset_language_server_feature_gate();

        let _slow_completion = dispatcher
            .dispatch(
                router::PreparedRoute::for_test(
                    1,
                    ExecutionMode::LanguageServerFeature,
                    blocking_language_server_feature_route,
                ),
                responses.clone(),
            )
            .expect("language server feature should have a completion signal");
        wait_for_language_server_feature_route();

        let _fast_completion = dispatcher
            .dispatch(
                router::PreparedRoute::for_test(
                    2,
                    ExecutionMode::LanguageServerFeature,
                    successful_route,
                ),
                responses,
            )
            .expect("language server feature should have a completion signal");
        assert!(
            receiver
                .recv_timeout(Duration::from_millis(250))
                .expect("independent feature request should bypass slow server work")
                .is_ok()
        );

        release_language_server_feature_route();
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("slow feature request should finish")
                .is_ok()
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn queued_cancelled_language_server_feature_does_not_execute() {
        let _test_lock = LANGUAGE_SERVER_FEATURE_TEST_LOCK.lock().unwrap();
        let (store, path) = test_store("language-server-feature-queued-cancellation");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        let (responses, receiver) = test_response_channel();
        reset_language_server_feature_gate();
        CANCELLED_ROUTE_INVOCATIONS.store(0, Ordering::Relaxed);

        let mut completions = Vec::new();
        for request_id in 1..=LANGUAGE_SERVER_FEATURE_WORKERS as u64 {
            completions.push(
                dispatcher
                    .dispatch(
                        router::PreparedRoute::for_test(
                            request_id,
                            ExecutionMode::LanguageServerFeature,
                            blocking_language_server_feature_route,
                        ),
                        responses.clone(),
                    )
                    .expect("blocking feature should have a completion signal"),
            );
        }
        wait_for_language_server_feature_routes(LANGUAGE_SERVER_FEATURE_WORKERS);

        let cancellation = core::language_servers::LanguageServerRequestCancellation::new();
        completions.push(
            dispatcher
                .dispatch_cancellable(
                    router::PreparedRoute::for_test(
                        99,
                        ExecutionMode::LanguageServerFeature,
                        cancelled_route,
                    ),
                    responses,
                    cancellation.clone(),
                    None,
                )
                .expect("queued feature should have a completion signal"),
        );
        cancellation.cancel();
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());

        release_language_server_feature_route();
        for completion in completions {
            completion
                .recv_timeout(Duration::from_secs(2))
                .expect("feature should complete");
        }
        let responses = (0..=LANGUAGE_SERVER_FEATURE_WORKERS)
            .map(|_| {
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .expect("feature should respond")
            })
            .collect::<Vec<_>>();
        assert_eq!(CANCELLED_ROUTE_INVOCATIONS.load(Ordering::Relaxed), 0);
        assert!(responses.iter().any(|response| {
            serde_json::to_value(response).is_ok_and(|response| {
                response["id"] == 99
                    && response["error"]["code"] == "language_servers.request_cancelled"
            })
        }));

        let _ = std::fs::remove_file(path);
    }

    fn workspace_open_route(request_id: u64, path: &str) -> router::PreparedRoute {
        router::prepare(RequestEnvelope {
            id: request_id,
            domain: Domain::Workspace,
            action: "open".to_owned(),
            params: serde_json::json!({ "path": path }),
        })
        .expect("workspace route should prepare")
    }

    fn blocking_external_route(
        _state: &mut core::State,
        request: &RequestEnvelope,
    ) -> ServerMessage {
        let (gate, condition) = external_gate();
        let mut gate = gate.lock().expect("gate should lock");
        gate.started = true;
        condition.notify_all();

        while !gate.release {
            gate = condition.wait(gate).expect("gate should wait");
        }

        ServerMessage::ok(request.id, true)
    }

    fn successful_route(_state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
        ServerMessage::ok(request.id, true)
    }

    fn panicking_route(_state: &mut core::State, _request: &RequestEnvelope) -> ServerMessage {
        panic!("test handler panic")
    }

    fn cancelled_route(_state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
        CANCELLED_ROUTE_INVOCATIONS.fetch_add(1, Ordering::Relaxed);
        ServerMessage::ok(request.id, true)
    }

    fn blocking_language_server_feature_route(
        _state: &mut core::State,
        request: &RequestEnvelope,
    ) -> ServerMessage {
        let (gate, condition) = language_server_feature_gate();
        let mut gate = gate.lock().expect("gate should lock");
        gate.started += 1;
        condition.notify_all();

        while !gate.release {
            gate = condition.wait(gate).expect("gate should wait");
        }

        ServerMessage::ok(request.id, true)
    }

    fn external_gate() -> &'static (Mutex<ExternalGate>, Condvar) {
        EXTERNAL_GATE.get_or_init(|| (Mutex::new(ExternalGate::default()), Condvar::new()))
    }

    fn reset_external_gate() {
        *external_gate().0.lock().expect("gate should lock") = ExternalGate::default();
    }

    fn wait_for_external_route() {
        let (gate, condition) = external_gate();
        let gate = gate.lock().expect("gate should lock");
        let (gate, timeout) = condition
            .wait_timeout_while(gate, Duration::from_secs(2), |gate| !gate.started)
            .expect("gate should wait");

        assert!(gate.started, "external route did not start");
        assert!(!timeout.timed_out(), "external route start timed out");
    }

    fn release_external_route() {
        let (gate, condition) = external_gate();
        let mut gate = gate.lock().expect("gate should lock");
        gate.release = true;
        condition.notify_all();
    }

    fn language_server_feature_gate() -> &'static (Mutex<FeatureGate>, Condvar) {
        LANGUAGE_SERVER_FEATURE_GATE
            .get_or_init(|| (Mutex::new(FeatureGate::default()), Condvar::new()))
    }

    fn reset_language_server_feature_gate() {
        *language_server_feature_gate()
            .0
            .lock()
            .expect("gate should lock") = FeatureGate::default();
    }

    fn wait_for_language_server_feature_route() {
        wait_for_language_server_feature_routes(1);
    }

    fn wait_for_language_server_feature_routes(expected: usize) {
        let (gate, condition) = language_server_feature_gate();
        let gate = gate.lock().expect("gate should lock");
        let (gate, timeout) = condition
            .wait_timeout_while(gate, Duration::from_secs(2), |gate| gate.started < expected)
            .expect("gate should wait");

        assert_eq!(
            gate.started, expected,
            "language server feature routes did not start"
        );
        assert!(
            !timeout.timed_out(),
            "language server feature route start timed out"
        );
    }

    fn release_language_server_feature_route() {
        let (gate, condition) = language_server_feature_gate();
        let mut gate = gate.lock().expect("gate should lock");
        gate.release = true;
        condition.notify_all();
    }

    fn test_store(name: &str) -> (core::DurableStore, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kosmos-server-dispatcher-{}-{name}-{nanos}.sqlite3",
            std::process::id()
        ));
        let store = core::DurableStore::open(&path).expect("store should open");

        (store, path)
    }

    fn test_response_channel() -> (ResponseSender, response::ResponseReceiver) {
        let (stream, _peer) =
            std::os::unix::net::UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) =
            response::channel(&stream).expect("response channel should open");

        (responses, receiver)
    }

    fn test_workspace(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kosmos-server-workspace-{}-{name}-{nanos}",
            std::process::id()
        ));

        std::fs::create_dir_all(&path).expect("workspace should be created");
        path
    }
}

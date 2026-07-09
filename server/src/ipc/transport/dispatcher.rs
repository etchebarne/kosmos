use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use crate::ipc::messages::envelope::ServerMessage;
use crate::ipc::router::{ExecutionMode, PersistenceMode, PreparedRoute};

use super::response::ResponseSender;

const MAX_QUEUED_REQUESTS: usize = 64;

#[derive(Clone)]
pub(crate) struct Dispatcher {
    state: Arc<Mutex<core::State>>,
    external_requests: mpsc::SyncSender<ExternalRequest>,
    persistent_requests: mpsc::SyncSender<PersistentRequest>,
}

impl Dispatcher {
    pub(crate) fn new(
        state: core::State,
        store: core::persistence::StateStore,
    ) -> io::Result<Self> {
        let state = Arc::new(Mutex::new(state));
        let (external_requests, external_receiver) = mpsc::sync_channel(MAX_QUEUED_REQUESTS);
        let (persistent_requests, persistent_receiver) = mpsc::sync_channel(MAX_QUEUED_REQUESTS);

        spawn_external_worker(external_receiver)?;
        spawn_persistence_worker(Arc::clone(&state), store, persistent_receiver)?;

        Ok(Self {
            state,
            external_requests,
            persistent_requests,
        })
    }

    #[cfg(test)]
    pub(crate) fn dispatch(
        &self,
        route: PreparedRoute,
        responses: ResponseSender,
    ) -> Option<mpsc::Receiver<()>> {
        self.dispatch_cancellable(route, responses, Arc::new(AtomicBool::new(false)))
    }

    pub(crate) fn dispatch_cancellable(
        &self,
        route: PreparedRoute,
        responses: ResponseSender,
        cancelled: Arc<AtomicBool>,
    ) -> Option<mpsc::Receiver<()>> {
        if cancelled.load(Ordering::Acquire) {
            return None;
        }

        match route.mode() {
            ExecutionMode::Snapshot => {
                responses.send(execute_snapshot(&self.state, &route));
                None
            }
            ExecutionMode::Live => {
                responses.send(execute_live(&self.state, &route));
                None
            }
            ExecutionMode::External => {
                let (completion, completed) = mpsc::channel();
                let candidate = match persistent_candidate(&self.state, route.request_id()) {
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
                        candidate,
                        completion,
                        cancelled,
                    },
                );
                Some(completed)
            }
            ExecutionMode::Persistent(_) => {
                let (completion, completed) = mpsc::channel();
                queue_persistent_request(
                    &self.persistent_requests,
                    PersistentRequest {
                        route,
                        responses,
                        completion,
                        cancelled,
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
    candidate: core::PersistentStateCandidate,
    completion: mpsc::Sender<()>,
    cancelled: Arc<AtomicBool>,
}

struct PersistentRequest {
    route: PreparedRoute,
    responses: ResponseSender,
    completion: mpsc::Sender<()>,
    cancelled: Arc<AtomicBool>,
}

fn spawn_external_worker(requests: mpsc::Receiver<ExternalRequest>) -> io::Result<()> {
    thread::Builder::new()
        .name("kosmos-external-operations".to_owned())
        .spawn(move || {
            while let Ok(mut request) = requests.recv() {
                if request.cancelled.load(Ordering::Acquire) {
                    let _ = request.completion.send(());
                    continue;
                }

                let response = execute_handler(&request.route, request.candidate.state_mut());
                request.responses.send(response);
                let _ = request.completion.send(());
            }
        })
        .map(|_| ())
}

fn spawn_persistence_worker(
    state: Arc<Mutex<core::State>>,
    store: core::persistence::StateStore,
    requests: mpsc::Receiver<PersistentRequest>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("kosmos-persistent-operations".to_owned())
        .spawn(move || {
            while let Ok(request) = requests.recv() {
                if request.cancelled.load(Ordering::Acquire) {
                    let _ = request.completion.send(());
                    continue;
                }

                let ExecutionMode::Persistent(persistence) = request.route.mode() else {
                    request.responses.send(ServerMessage::error(
                        request.route.request_id(),
                        "ipc.invalid_execution_mode",
                        "non-persistent request reached the persistence worker",
                    ));
                    let _ = request.completion.send(());
                    continue;
                };
                let response = execute_persistent(&state, &store, &request.route, persistence);

                request.responses.send(response);
                let _ = request.completion.send(());
            }
        })
        .map(|_| ())
}

fn execute_snapshot(state: &Mutex<core::State>, route: &PreparedRoute) -> ServerMessage {
    let mut candidate = match persistent_candidate(state, route.request_id()) {
        Ok(candidate) => candidate,
        Err(response) => return response,
    };

    execute_handler(route, candidate.state_mut())
}

fn execute_live(state: &Mutex<core::State>, route: &PreparedRoute) -> ServerMessage {
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return state_unavailable(route.request_id()),
    };

    execute_handler(route, &mut state)
}

fn execute_persistent(
    state: &Mutex<core::State>,
    store: &core::persistence::StateStore,
    route: &PreparedRoute,
    persistence: PersistenceMode,
) -> ServerMessage {
    let mut candidate = match persistent_candidate(state, route.request_id()) {
        Ok(candidate) => candidate,
        Err(response) => return response,
    };
    let response = execute_handler(route, candidate.state_mut());

    if !response.is_ok() {
        return response;
    }

    if persistence == PersistenceMode::Barrier {
        return response;
    }

    let candidate_is_current = match state.lock() {
        Ok(state) => state.accepts_persistent_candidate(&candidate),
        Err(_) => return state_unavailable(route.request_id()),
    };

    if !candidate_is_current {
        return state_conflict(route.request_id());
    }

    let save_result = match persistence {
        PersistenceMode::ActiveWorkspace => store.save_active_workspace(candidate.state()),
        PersistenceMode::Barrier => unreachable!("barriers return before persistence"),
        PersistenceMode::Full => store.save(candidate.state()),
    };

    if let Err(error) = save_result {
        return ServerMessage::error(
            route.request_id(),
            "persistence.save_failed",
            error.to_string(),
        );
    }

    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return state_unavailable(route.request_id()),
    };
    if !state.commit_persistent_candidate(candidate) {
        let rollback_error = store.save(&state).err();
        let message = match rollback_error {
            Some(error) => {
                format!("persistent state changed before commit; database rollback failed: {error}")
            }
            None => "persistent state changed before the saved candidate could commit".to_owned(),
        };

        return ServerMessage::error(route.request_id(), "persistence.state_conflict", message);
    }

    response
}

fn persistent_candidate(
    state: &Mutex<core::State>,
    request_id: u64,
) -> Result<core::PersistentStateCandidate, ServerMessage> {
    state
        .lock()
        .map(|state| state.persistent_candidate())
        .map_err(|_| state_unavailable(request_id))
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

fn execute_handler(route: &PreparedRoute, state: &mut core::State) -> ServerMessage {
    catch_unwind(AssertUnwindSafe(|| route.execute(state)))
        .unwrap_or_else(|_| handler_panicked(route.request_id()))
}

fn state_unavailable(request_id: u64) -> ServerMessage {
    ServerMessage::error(
        request_id,
        "ipc.state_unavailable",
        "IPC state mutex was poisoned",
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Condvar, OnceLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::ipc::messages::envelope::{Domain, RequestEnvelope};
    use crate::ipc::router;
    use crate::ipc::transport::response;

    #[derive(Default)]
    struct ExternalGate {
        release: bool,
        started: bool,
    }

    static EXTERNAL_GATE: OnceLock<(Mutex<ExternalGate>, Condvar)> = OnceLock::new();

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
        let persisted_workspace_count = core::persistence::StateStore::open(&path)
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

    fn test_store(name: &str) -> (core::persistence::StateStore, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kosmos-server-dispatcher-{}-{name}-{nanos}.sqlite3",
            std::process::id()
        ));
        let store = core::persistence::StateStore::open(&path).expect("store should open");

        (store, path)
    }

    fn test_response_channel() -> (ResponseSender, mpsc::Receiver<ServerMessage>) {
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

use std::collections::HashMap;
use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use crate::ipc::messages::envelope::{ClientMessage, ServerMessage};
use crate::ipc::router::{self, PreparedRoute, SchedulingMode};

use super::dispatcher::Dispatcher;
use super::response::ResponseSender;
use super::{codec, response};

const MAX_PENDING_REQUESTS: usize = 64;
const RESPONSE_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn handle(stream: UnixStream, dispatcher: Dispatcher) -> io::Result<()> {
    let lifecycle = ConnectionLifecycle::new();
    stream.set_write_timeout(Some(RESPONSE_WRITE_TIMEOUT))?;
    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);
    let (responses, response_receiver, shutdown) = response::channel(&stream)?;
    let (requests, request_receiver) = mpsc::sync_channel(MAX_PENDING_REQUESTS);
    let notification_subscription = dispatcher.subscribe(responses.clone());
    let renderer_id = notification_subscription.id();

    thread::spawn(move || write_responses(stream, response_receiver, shutdown));
    thread::spawn({
        let closed = lifecycle.closed();
        let responses = responses.clone();
        let dispatcher = dispatcher.clone();
        let active = lifecycle.active();
        move || {
            dispatch_requests(
                dispatcher,
                request_receiver,
                responses,
                closed,
                active,
                None,
            )
        }
    });

    while let Some(frame) = codec::read_frame(&mut reader)? {
        if frame.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<ClientMessage>(&frame) {
            Ok(ClientMessage::Request(request)) => match router::prepare(request) {
                Ok(route) => queue_request(&requests, route, &responses, &lifecycle),
                Err(response) => responses.send(response),
            },
            Ok(ClientMessage::Cancel { id }) => lifecycle.cancel(id),
            Ok(ClientMessage::ApplyEditAck {
                id,
                token,
                applied,
                failure_reason,
            }) => {
                let failure_reason = failure_reason.map(|mut reason| {
                    reason.truncate(reason.floor_char_boundary(4 * 1024));
                    reason
                });
                dispatcher.acknowledge_apply_edit(renderer_id, id, &token, applied, failure_reason);
            }
            Err(error) => responses.send(ServerMessage::error(
                0,
                "ipc.invalid_message",
                error.to_string(),
            )),
        }
    }

    Ok(())
}

fn dispatch_requests(
    dispatcher: Dispatcher,
    requests: mpsc::Receiver<PendingRoute>,
    responses: ResponseSender,
    closed: Arc<AtomicBool>,
    active: ActiveRequests,
    _workspace_edit_owner: Option<()>,
) {
    let mut external_completions = Vec::new();

    while let Ok(pending) = requests.recv() {
        discard_completed(&mut external_completions, &active);

        if closed.load(Ordering::Acquire) {
            break;
        }

        let PendingRoute {
            route,
            cancellation,
        } = pending;
        let request_id = route.request_id();
        match route.mode() {
            SchedulingMode::External => {
                if let Some(completed) =
                    dispatcher.dispatch_cancellable(route, responses.clone(), cancellation, None)
                {
                    external_completions.push(PendingCompletion {
                        request_id,
                        completed,
                    });
                } else {
                    active.finish(request_id);
                }
            }
            SchedulingMode::LanguageServer | SchedulingMode::LanguageServerFeature => {
                if let Some(completed) =
                    dispatcher.dispatch_cancellable(route, responses.clone(), cancellation, None)
                {
                    track_completion(active.clone(), request_id, completed);
                } else {
                    active.finish(request_id);
                }
            }
            SchedulingMode::SerialMutation | SchedulingMode::PersistenceBarrier => {
                wait_for_completions(&mut external_completions, &active);

                if let Some(completed) =
                    dispatcher.dispatch_cancellable(route, responses.clone(), cancellation, None)
                {
                    let _ = completed.recv();
                }
                active.finish(request_id);
            }
            SchedulingMode::Live | SchedulingMode::Snapshot => {
                let _ =
                    dispatcher.dispatch_cancellable(route, responses.clone(), cancellation, None);
                active.finish(request_id);
            }
        }
    }
}

struct ConnectionLifecycle {
    closed: Arc<AtomicBool>,
    active: ActiveRequests,
}

impl ConnectionLifecycle {
    fn new() -> Self {
        Self {
            closed: Arc::new(AtomicBool::new(false)),
            active: ActiveRequests::default(),
        }
    }

    fn closed(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.closed)
    }

    fn active(&self) -> ActiveRequests {
        self.active.clone()
    }

    fn begin(
        &self,
        request_id: u64,
    ) -> Option<core::language_servers::LanguageServerRequestCancellation> {
        self.active.begin(request_id)
    }

    fn cancel(&self, request_id: u64) {
        self.active.cancel(request_id);
    }
}

impl Drop for ConnectionLifecycle {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.active.cancel_all();
    }
}

#[derive(Clone, Default)]
struct ActiveRequests {
    requests: Arc<Mutex<HashMap<u64, core::language_servers::LanguageServerRequestCancellation>>>,
}

impl ActiveRequests {
    fn begin(
        &self,
        request_id: u64,
    ) -> Option<core::language_servers::LanguageServerRequestCancellation> {
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if requests.contains_key(&request_id) {
            return None;
        }
        let cancellation = core::language_servers::LanguageServerRequestCancellation::new();
        requests.insert(request_id, cancellation.clone());
        Some(cancellation)
    }

    fn finish(&self, request_id: u64) {
        self.requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&request_id);
    }

    fn cancel(&self, request_id: u64) {
        let cancellation = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&request_id)
            .cloned();
        if let Some(cancellation) = cancellation {
            cancellation.cancel();
        }
    }

    fn cancel_all(&self) {
        let cancellations = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for cancellation in cancellations {
            cancellation.cancel();
        }
    }
}

struct PendingRoute {
    route: PreparedRoute,
    cancellation: core::language_servers::LanguageServerRequestCancellation,
}

struct PendingCompletion {
    request_id: u64,
    completed: mpsc::Receiver<()>,
}

fn wait_for_completions(completions: &mut Vec<PendingCompletion>, active: &ActiveRequests) {
    for completion in completions.drain(..) {
        let _ = completion.completed.recv();
        active.finish(completion.request_id);
    }
}

fn discard_completed(completions: &mut Vec<PendingCompletion>, active: &ActiveRequests) {
    completions.retain(|completion| {
        if matches!(
            completion.completed.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ) {
            true
        } else {
            active.finish(completion.request_id);
            false
        }
    });
}

fn track_completion(active: ActiveRequests, request_id: u64, completed: mpsc::Receiver<()>) {
    thread::spawn(move || {
        let _ = completed.recv();
        active.finish(request_id);
    });
}

fn queue_request(
    requests: &mpsc::SyncSender<PendingRoute>,
    route: PreparedRoute,
    responses: &ResponseSender,
    lifecycle: &ConnectionLifecycle,
) {
    let request_id = route.request_id();
    let Some(cancellation) = lifecycle.begin(request_id) else {
        responses.send(ServerMessage::error(
            request_id,
            "ipc.duplicate_request_id",
            "IPC request ID is already active",
        ));
        return;
    };

    match requests.try_send(PendingRoute {
        route,
        cancellation,
    }) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(_)) => {
            lifecycle.active.finish(request_id);
            responses.send(ServerMessage::error(
                request_id,
                "ipc.connection_busy",
                "IPC connection request queue is full",
            ));
        }
        Err(mpsc::TrySendError::Disconnected(_)) => {
            lifecycle.active.finish(request_id);
            responses.send(ServerMessage::error(
                request_id,
                "ipc.connection_closed",
                "IPC connection request dispatcher is unavailable",
            ));
        }
    }
}

fn write_responses(
    mut stream: UnixStream,
    responses: response::ResponseReceiver,
    shutdown: Arc<UnixStream>,
) {
    while let Ok(response) = responses.recv() {
        if let Err(error) = codec::write_message(&mut stream, &response) {
            eprintln!("IPC response write failed: {error}");
            let _ = shutdown.shutdown(std::net::Shutdown::Both);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufWriter, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::ipc::messages::envelope::{RequestEnvelope, ServerMessage};
    use crate::ipc::router::ExecutionMode;

    #[derive(Default)]
    struct PersistentGate {
        release: bool,
        started: bool,
    }

    static PERSISTENT_GATE: OnceLock<(Mutex<PersistentGate>, Condvar)> = OnceLock::new();
    static ORDERING_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn cancellation_targets_one_request_and_connection_close_cancels_the_rest() {
        let lifecycle = ConnectionLifecycle::new();
        let first = lifecycle.begin(1).expect("first request should start");
        let second = lifecycle.begin(2).expect("second request should start");
        assert!(lifecycle.begin(1).is_none());

        lifecycle.cancel(1);
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());

        drop(lifecycle);
        assert!(second.is_cancelled());
    }

    #[test]
    fn language_server_install_event_does_not_block_install_or_status_responses() {
        let root = test_directory("language-server-install-event");
        let store =
            core::DurableStore::open(root.join("state.sqlite3")).expect("store should open");
        let paths = core::language_servers::LanguageServerPaths::new(
            root.join("language-servers"),
            root.join("language-server-cache"),
        );
        write_test_rust_analyzer_installation(&paths);
        let manager = core::language_servers::LanguageServerManager::open(paths, store.clone())
            .expect("language server manager should open");
        let mut state = core::State::new();
        state.attach_language_server_manager(manager);
        let dispatcher = Dispatcher::new(state, store).expect("dispatcher should open");
        let (server, client) = UnixStream::pair().expect("socket pair should open");
        let connection = thread::spawn(move || handle(server, dispatcher));
        let mut writer = BufWriter::new(client.try_clone().expect("client should clone"));
        let mut reader = BufReader::new(client.try_clone().expect("client should clone"));
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should be set");

        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request",
                "id": 1,
                "domain": "languageServers",
                "action": "install",
                "params": { "serverId": "rust-analyzer" }
            }),
        );
        let mut install_response = false;
        let mut status_event = false;
        while !install_response || !status_event {
            let message = read_message(&mut reader);
            install_response |=
                message["type"] == "response" && message["id"] == 1 && message["ok"] == true;
            status_event |= message["type"] == "notification"
                && message["event"] == "languageServerStatusChanged"
                && message["serverId"] == "rust-analyzer";
        }

        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request",
                "id": 2,
                "domain": "languageServers",
                "action": "status",
                "params": { "serverId": "rust-analyzer" }
            }),
        );
        loop {
            let message = read_message(&mut reader);
            if message["type"] == "response" && message["id"] == 2 {
                assert_eq!(message["ok"], true);
                assert_eq!(message["result"]["id"], "rust-analyzer");
                break;
            }
        }

        drop(reader);
        drop(writer);
        drop(client);
        connection
            .join()
            .expect("connection should not panic")
            .expect("connection should close cleanly");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn restarted_server_lists_freshly_authorized_workspace_edit_recoveries() {
        let root = test_directory("workspace-edit-restart");
        let database = root.join("state.sqlite3");
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).expect("workspace should be created");
        std::fs::write(workspace.join("retry.txt"), "before-retry").unwrap();
        std::fs::write(workspace.join("finalize.txt"), "before-finalize").unwrap();
        std::fs::write(workspace.join("retried.txt"), "old-retry-target").unwrap();
        std::fs::write(workspace.join("finalized.txt"), "old-finalize-target").unwrap();
        let store = core::DurableStore::open(&database).expect("store should open");
        let paths = core::language_servers::LanguageServerPaths::new(
            root.join("language-servers"),
            root.join("language-server-cache"),
        );
        let manager = core::language_servers::LanguageServerManager::open(paths.clone(), store)
            .expect("language server manager should open");
        let mut state = core::State::new();
        let workspace_id = state.open_workspace(&workspace);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            core::tree::PaneId::new(1),
            core::tree::TabId::new(1),
            core::tree::TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), core::tree::TabId::new(1), "retry.txt")
            .unwrap();
        let retry = manager
            .stage_workspace_edit(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "rename",
                    "oldUri": format!("file://{}", workspace.join("retry.txt").display()),
                    "newUri": format!("file://{}", workspace.join("retried.txt").display()),
                    "options": { "overwrite": true }
                }]}),
                &[core::language_servers::WorkspaceEditRoot {
                    workspace_id,
                    path: workspace.clone(),
                }],
            )
            .expect("workspace edit should stage");
        let finalize = manager
            .stage_workspace_edit(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "rename",
                    "oldUri": format!("file://{}", workspace.join("finalize.txt").display()),
                    "newUri": format!("file://{}", workspace.join("finalized.txt").display()),
                    "options": { "overwrite": true }
                }]}),
                &[core::language_servers::WorkspaceEditRoot {
                    workspace_id,
                    path: workspace.clone(),
                }],
            )
            .expect("second workspace edit should stage");
        state.attach_language_server_manager(manager);
        state
            .commit_workspace_edit(retry.transaction_id, &retry.authorization)
            .unwrap();
        state
            .commit_workspace_edit(finalize.transaction_id, &finalize.authorization)
            .unwrap();
        core::DurableStore::open(&database)
            .unwrap()
            .save(&state)
            .unwrap();
        let retry_recovery =
            workspace.join(format!(".kosmos-workspace-edit-{}", retry.authorization));
        let finalize_recovery =
            workspace.join(format!(".kosmos-workspace-edit-{}", finalize.authorization));
        std::fs::set_permissions(&retry_recovery, std::fs::Permissions::from_mode(0o0)).unwrap();
        std::fs::set_permissions(&finalize_recovery, std::fs::Permissions::from_mode(0o0)).unwrap();
        assert!(
            state
                .finish_workspace_edit(finalize.transaction_id, &finalize.authorization)
                .is_err()
        );
        drop(state);

        let restarted_store = core::DurableStore::open(&database).expect("store should reopen");
        let restarted_manager =
            core::language_servers::LanguageServerManager::open(paths, restarted_store.clone())
                .expect("manager should recover the journal");
        let mut state = restarted_store.load().expect("state should load");
        state.attach_language_server_manager(restarted_manager);
        let dispatcher =
            Dispatcher::new(state, restarted_store.clone()).expect("dispatcher should open");
        let (server, client) = UnixStream::pair().expect("socket pair should open");
        let connection = thread::spawn(move || handle(server, dispatcher));
        let mut writer = BufWriter::new(client.try_clone().expect("client should clone"));
        let mut reader = BufReader::new(client.try_clone().expect("client should clone"));
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should be set");

        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 1, "domain": "languageServers",
                "action": "listWorkspaceEditRecoveries", "params": {}
            }),
        );
        let listed = read_response(&mut reader, 1);
        assert_eq!(listed["ok"], true, "unexpected recovery list: {listed}");
        let listed = listed["result"].as_array().unwrap();
        assert_eq!(listed.len(), 2);
        let credentials = |transaction_id: u64, phase: &str, retry_rollback: bool| {
            let recovery = listed
                .iter()
                .find(|recovery| recovery["transactionId"] == transaction_id)
                .unwrap();
            assert_eq!(recovery["phase"], phase);
            assert_eq!(recovery["retryRollback"], retry_rollback);
            assert_eq!(recovery["canFinalize"], true);
            serde_json::json!({
                "transactionId": transaction_id,
                "authorization": recovery["authorization"].as_str().unwrap(),
            })
        };
        let retry_params = credentials(retry.transaction_id, "recoveryRequired", true);
        let finalize_params =
            credentials(finalize.transaction_id, "committedCleanupRequired", false);
        assert_ne!(retry_params["authorization"], retry.authorization);
        assert_ne!(finalize_params["authorization"], finalize.authorization);

        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 7, "domain": "languageServers",
                "action": "workspaceEditStatus", "params": {
                    "transactionId": retry.transaction_id,
                    "authorization": retry.authorization,
                }
            }),
        );
        let rejected = read_response(&mut reader, 7);
        assert_eq!(rejected["ok"], false);
        assert_eq!(rejected["error"]["code"], "workspace_edit.invalid");

        std::fs::set_permissions(&retry_recovery, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&finalize_recovery, std::fs::Permissions::from_mode(0o700))
            .unwrap();

        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 2, "domain": "languageServers",
                "action": "rollbackWorkspaceEdit", "params": retry_params
            }),
        );
        let rolled_back = read_response(&mut reader, 2);
        assert_eq!(
            rolled_back["ok"], true,
            "unexpected rollback response: {rolled_back}"
        );
        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 3, "domain": "languageServers",
                "action": "finishWorkspaceEdit", "params": retry_params
            }),
        );
        assert_eq!(read_response(&mut reader, 3)["ok"], true);
        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 8, "domain": "languageServers",
                "action": "acknowledgeWorkspaceEditCompletion", "params": retry_params
            }),
        );
        assert_eq!(read_response(&mut reader, 8)["ok"], true);
        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 4, "domain": "languageServers",
                "action": "finalizeWorkspaceEdit", "params": finalize_params
            }),
        );
        let finalized = read_response(&mut reader, 4);
        assert_eq!(
            finalized["ok"], true,
            "unexpected finalize response: {finalized}"
        );
        assert_eq!(finalized["result"]["phase"], "finishedCommitted");
        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 5, "domain": "languageServers",
                "action": "acknowledgeWorkspaceEditCompletion", "params": finalize_params
            }),
        );
        assert_eq!(read_response(&mut reader, 5)["ok"], true);
        write_request(
            &mut writer,
            serde_json::json!({
                "type": "request", "id": 6, "domain": "languageServers",
                "action": "listWorkspaceEditRecoveries", "params": {}
            }),
        );
        assert_eq!(
            read_response(&mut reader, 6)["result"],
            serde_json::json!([])
        );

        assert!(workspace.join("retry.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(workspace.join("retried.txt")).unwrap(),
            "old-retry-target"
        );
        assert!(workspace.join("finalized.txt").is_file());
        assert!(!workspace.join("finalize.txt").exists());
        assert_eq!(
            restarted_store.load().unwrap().editor_view_states()[0].path(),
            "retry.txt"
        );

        drop(reader);
        drop(writer);
        drop(client);
        connection
            .join()
            .expect("connection should not panic")
            .expect("connection should close cleanly");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn live_requests_wait_for_prior_persistent_requests() {
        let _test_lock = ORDERING_TEST_LOCK.lock().expect("test lock should lock");
        let (store, path) = test_store("connection-ordering");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        let (stream, _peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) = response::channel(&stream).expect("responses should open");
        let (requests, request_receiver) = mpsc::sync_channel(2);
        reset_persistent_gate();
        thread::spawn({
            let responses = responses.clone();
            move || {
                dispatch_requests(
                    dispatcher,
                    request_receiver,
                    responses,
                    Arc::new(AtomicBool::new(false)),
                    ActiveRequests::default(),
                    None,
                )
            }
        });

        requests
            .send(pending_route(router::PreparedRoute::for_test(
                1,
                ExecutionMode::PersistenceBarrier,
                blocking_persistent_route,
            )))
            .expect("persistent request should queue");
        requests
            .send(pending_route(router::PreparedRoute::for_test(
                2,
                ExecutionMode::Live,
                successful_route,
            )))
            .expect("live request should queue");
        wait_for_persistent_route();

        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());

        release_persistent_route();
        for _ in 0..2 {
            assert!(
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .expect("ordered request should respond")
                    .is_ok()
            );
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn live_requests_bypass_external_work_while_persistence_waits() {
        let _test_lock = ORDERING_TEST_LOCK.lock().expect("test lock should lock");
        let (store, path) = test_store("external-ordering");
        let dispatcher =
            Dispatcher::new(core::State::new(), store).expect("dispatcher should open");
        let (stream, _peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) = response::channel(&stream).expect("responses should open");
        let (requests, request_receiver) = mpsc::sync_channel(3);
        reset_persistent_gate();
        thread::spawn({
            let responses = responses.clone();
            move || {
                dispatch_requests(
                    dispatcher,
                    request_receiver,
                    responses,
                    Arc::new(AtomicBool::new(false)),
                    ActiveRequests::default(),
                    None,
                )
            }
        });

        requests
            .send(pending_route(router::PreparedRoute::for_test(
                1,
                ExecutionMode::External,
                blocking_persistent_route,
            )))
            .expect("external request should queue");
        requests
            .send(pending_route(router::PreparedRoute::for_test(
                2,
                ExecutionMode::Live,
                successful_route,
            )))
            .expect("live request should queue");
        requests
            .send(pending_route(router::PreparedRoute::for_test(
                3,
                ExecutionMode::PersistenceBarrier,
                successful_route,
            )))
            .expect("persistent request should queue");
        wait_for_persistent_route();

        assert!(
            receiver
                .recv_timeout(Duration::from_millis(250))
                .expect("live request should bypass external work")
                .is_ok()
        );
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());

        release_persistent_route();
        for _ in 0..2 {
            assert!(
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .expect("ordered request should respond")
                    .is_ok()
            );
        }

        let _ = std::fs::remove_file(path);
    }

    fn blocking_persistent_route(
        _state: &mut core::State,
        request: &RequestEnvelope,
    ) -> ServerMessage {
        let (gate, condition) = persistent_gate();
        let mut gate = gate.lock().expect("gate should lock");
        gate.started = true;
        condition.notify_all();

        while !gate.release {
            gate = condition.wait(gate).expect("gate should wait");
        }

        ServerMessage::ok(request.id, true)
    }

    fn pending_route(route: PreparedRoute) -> PendingRoute {
        PendingRoute {
            route,
            cancellation: core::language_servers::LanguageServerRequestCancellation::new(),
        }
    }

    fn write_request(writer: &mut BufWriter<UnixStream>, request: serde_json::Value) {
        serde_json::to_writer(&mut *writer, &request).expect("request should serialize");
        writer.write_all(b"\n").expect("request should terminate");
        writer.flush().expect("request should flush");
    }

    fn read_message(reader: &mut BufReader<UnixStream>) -> serde_json::Value {
        let mut frame = String::new();
        reader
            .read_line(&mut frame)
            .expect("server should respond before the timeout");
        assert!(!frame.is_empty(), "server closed before responding");
        serde_json::from_str(&frame).expect("server message should be valid JSON")
    }

    fn read_response(reader: &mut BufReader<UnixStream>, id: u64) -> serde_json::Value {
        loop {
            let message = read_message(reader);
            if message["type"] == "response" && message["id"] == id {
                return message;
            }
        }
    }

    fn write_test_rust_analyzer_installation(paths: &core::language_servers::LanguageServerPaths) {
        let definition = core::language_servers::language_server_catalog()
            .iter()
            .find(|definition| definition.id() == "rust-analyzer")
            .expect("rust-analyzer should be in the catalog");
        let (source_url, sha256) = match std::env::consts::ARCH {
            "x86_64" => (
                "https://github.com/rust-lang/rust-analyzer/releases/download/2026-07-06/rust-analyzer-x86_64-unknown-linux-gnu.gz",
                "2fb596e12676e512de5dbf1c322dd591127ee089a1cca47995605593f2fc8850",
            ),
            "aarch64" => (
                "https://github.com/rust-lang/rust-analyzer/releases/download/2026-07-06/rust-analyzer-aarch64-unknown-linux-gnu.gz",
                "7e2627d96c6f1614115d212b61fd5f8dc9279853054b800f2b023c883e3ae056",
            ),
            architecture => panic!("unsupported test architecture: {architecture}"),
        };
        let directory = paths
            .data_directory()
            .join(definition.id())
            .join(definition.version());
        let executable = directory.join("rust-analyzer");
        std::fs::create_dir_all(&directory).expect("installation directory should be created");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n")
            .expect("test executable should be written");
        let mut permissions = std::fs::metadata(&executable)
            .expect("test executable metadata should load")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions)
            .expect("test executable should become executable");
        std::fs::write(
            directory.join("installation.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "serverId": definition.id(),
                "version": definition.version(),
                "operatingSystem": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
                "sourceUrl": source_url,
                "sha256": sha256,
                "executable": "rust-analyzer"
            }))
            .expect("manifest should serialize"),
        )
        .expect("manifest should be written");
    }

    fn successful_route(_state: &mut core::State, request: &RequestEnvelope) -> ServerMessage {
        ServerMessage::ok(request.id, true)
    }

    fn persistent_gate() -> &'static (Mutex<PersistentGate>, Condvar) {
        PERSISTENT_GATE.get_or_init(|| (Mutex::new(PersistentGate::default()), Condvar::new()))
    }

    fn reset_persistent_gate() {
        *persistent_gate().0.lock().expect("gate should lock") = PersistentGate::default();
    }

    fn wait_for_persistent_route() {
        let (gate, condition) = persistent_gate();
        let gate = gate.lock().expect("gate should lock");
        let (gate, timeout) = condition
            .wait_timeout_while(gate, Duration::from_secs(2), |gate| !gate.started)
            .expect("gate should wait");

        assert!(gate.started, "persistent route did not start");
        assert!(!timeout.timed_out(), "persistent route start timed out");
    }

    fn release_persistent_route() {
        let (gate, condition) = persistent_gate();
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
            "kosmos-server-connection-{}-{name}-{nanos}.sqlite3",
            std::process::id()
        ));
        let store = core::DurableStore::open(&path).expect("store should open");

        (store, path)
    }

    fn test_directory(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kosmos-server-connection-{}-{name}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("test directory should be created");
        path
    }
}

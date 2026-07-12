use std::collections::HashMap;
use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use crate::ipc::messages::envelope::{ClientMessage, ServerMessage};
use crate::ipc::router::{self, ExecutionMode, PreparedRoute};

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
                renderer_id,
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
    renderer_id: u64,
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
            ExecutionMode::External => {
                if let Some(completed) = dispatcher.dispatch_cancellable(
                    route,
                    responses.clone(),
                    cancellation,
                    renderer_id,
                ) {
                    external_completions.push(PendingCompletion {
                        request_id,
                        completed,
                    });
                } else {
                    active.finish(request_id);
                }
            }
            ExecutionMode::LanguageServer | ExecutionMode::LanguageServerFeature => {
                if let Some(completed) = dispatcher.dispatch_cancellable(
                    route,
                    responses.clone(),
                    cancellation,
                    renderer_id,
                ) {
                    track_completion(active.clone(), request_id, completed);
                } else {
                    active.finish(request_id);
                }
            }
            ExecutionMode::Persistent(_) => {
                wait_for_completions(&mut external_completions, &active);

                if let Some(completed) = dispatcher.dispatch_cancellable(
                    route,
                    responses.clone(),
                    cancellation,
                    renderer_id,
                ) {
                    let _ = completed.recv();
                }
                active.finish(request_id);
            }
            ExecutionMode::Live | ExecutionMode::Snapshot => {
                let _ = dispatcher.dispatch_cancellable(
                    route,
                    responses.clone(),
                    cancellation,
                    renderer_id,
                );
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
    use crate::ipc::router::{ExecutionMode, PersistenceMode};

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
        let store = core::persistence::StateStore::open(root.join("state.sqlite3"))
            .expect("store should open");
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
                    0,
                )
            }
        });

        requests
            .send(pending_route(router::PreparedRoute::for_test(
                1,
                ExecutionMode::Persistent(PersistenceMode::Barrier),
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
                    0,
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
                ExecutionMode::Persistent(PersistenceMode::Barrier),
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

    fn test_store(name: &str) -> (core::persistence::StateStore, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kosmos-server-connection-{}-{name}-{nanos}.sqlite3",
            std::process::id()
        ));
        let store = core::persistence::StateStore::open(&path).expect("store should open");

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

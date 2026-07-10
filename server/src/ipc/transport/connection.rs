use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
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
    let _notification_subscription = dispatcher.subscribe(responses.clone());

    thread::spawn(move || write_responses(stream, response_receiver, shutdown));
    thread::spawn({
        let closed = lifecycle.closed();
        let responses = responses.clone();
        let dispatcher = dispatcher.clone();
        move || dispatch_requests(dispatcher, request_receiver, responses, closed)
    });

    while let Some(frame) = codec::read_frame(&mut reader)? {
        if frame.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<ClientMessage>(&frame) {
            Ok(ClientMessage::Request(request)) => match router::prepare(request) {
                Ok(route) => queue_request(&requests, route, &responses),
                Err(response) => responses.send(response),
            },
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
    requests: mpsc::Receiver<PreparedRoute>,
    responses: ResponseSender,
    closed: Arc<AtomicBool>,
) {
    let mut external_completions = Vec::new();

    while let Ok(route) = requests.recv() {
        discard_completed(&mut external_completions);

        if closed.load(Ordering::Acquire) {
            break;
        }

        match route.mode() {
            ExecutionMode::External => {
                if let Some(completed) =
                    dispatcher.dispatch_cancellable(route, responses.clone(), Arc::clone(&closed))
                {
                    external_completions.push(completed);
                }
            }
            ExecutionMode::Persistent(_) => {
                wait_for_completions(&mut external_completions);

                if let Some(completed) =
                    dispatcher.dispatch_cancellable(route, responses.clone(), Arc::clone(&closed))
                {
                    let _ = completed.recv();
                }
            }
            ExecutionMode::Live | ExecutionMode::Snapshot => {
                let _ =
                    dispatcher.dispatch_cancellable(route, responses.clone(), Arc::clone(&closed));
            }
        }
    }
}

struct ConnectionLifecycle {
    closed: Arc<AtomicBool>,
}

impl ConnectionLifecycle {
    fn new() -> Self {
        Self {
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    fn closed(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.closed)
    }
}

impl Drop for ConnectionLifecycle {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}

fn wait_for_completions(completions: &mut Vec<mpsc::Receiver<()>>) {
    for completion in completions.drain(..) {
        let _ = completion.recv();
    }
}

fn discard_completed(completions: &mut Vec<mpsc::Receiver<()>>) {
    completions
        .retain(|completion| matches!(completion.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

fn queue_request(
    requests: &mpsc::SyncSender<PreparedRoute>,
    route: PreparedRoute,
    responses: &ResponseSender,
) {
    let request_id = route.request_id();

    match requests.try_send(route) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(_)) => responses.send(ServerMessage::error(
            request_id,
            "ipc.connection_busy",
            "IPC connection request queue is full",
        )),
        Err(mpsc::TrySendError::Disconnected(_)) => responses.send(ServerMessage::error(
            request_id,
            "ipc.connection_closed",
            "IPC connection request dispatcher is unavailable",
        )),
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
                )
            }
        });

        requests
            .send(router::PreparedRoute::for_test(
                1,
                ExecutionMode::Persistent(PersistenceMode::Barrier),
                blocking_persistent_route,
            ))
            .expect("persistent request should queue");
        requests
            .send(router::PreparedRoute::for_test(
                2,
                ExecutionMode::Live,
                successful_route,
            ))
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
                )
            }
        });

        requests
            .send(router::PreparedRoute::for_test(
                1,
                ExecutionMode::External,
                blocking_persistent_route,
            ))
            .expect("external request should queue");
        requests
            .send(router::PreparedRoute::for_test(
                2,
                ExecutionMode::Live,
                successful_route,
            ))
            .expect("live request should queue");
        requests
            .send(router::PreparedRoute::for_test(
                3,
                ExecutionMode::Persistent(PersistenceMode::Barrier),
                successful_route,
            ))
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
}

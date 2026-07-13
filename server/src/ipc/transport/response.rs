use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Condvar, Mutex, mpsc};

#[cfg(test)]
use std::time::{Duration, Instant};

use crate::ipc::messages::envelope::ServerMessage;

const MAX_PENDING_RESPONSES: usize = 32;
const MAX_PENDING_LSP_DIAGNOSTICS: usize = 256;
const MAX_PENDING_LSP_SERVERS: usize = 64;

#[derive(Clone)]
pub(crate) struct ResponseSender {
    queue: Arc<ResponseQueue>,
    _lifecycle: Arc<SenderLifecycle>,
    shutdown: Arc<UnixStream>,
}

impl ResponseSender {
    pub(crate) fn send(&self, response: ServerMessage) {
        self.try_send(response);
    }

    pub(crate) fn try_send(&self, response: ServerMessage) -> bool {
        if !self.queue.push_response(response) {
            self.shutdown();
            return false;
        }

        true
    }

    pub(crate) fn notify_workspace_changed(&self, workspace_ids: &[u64]) -> bool {
        self.queue.push_workspace_change(workspace_ids)
    }

    pub(crate) fn notify_core_event(&self, event: core::events::CoreEvent) -> bool {
        if !self.queue.push_core_event(event) {
            self.shutdown();
            return false;
        }
        true
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.shutdown.shutdown(Shutdown::Both);
    }
}

pub(crate) struct ResponseReceiver {
    queue: Arc<ResponseQueue>,
}

impl ResponseReceiver {
    pub(crate) fn recv(&self) -> Result<ServerMessage, mpsc::RecvError> {
        let mut state = self.queue.state.lock().map_err(|_| mpsc::RecvError)?;

        loop {
            if let Some(message) = next_message(&mut state) {
                return Ok(message);
            }
            if state.senders_closed {
                return Err(mpsc::RecvError);
            }

            state = self
                .queue
                .available
                .wait(state)
                .map_err(|_| mpsc::RecvError)?;
        }
    }

    #[cfg(test)]
    pub(crate) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<ServerMessage, mpsc::RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .queue
            .state
            .lock()
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;

        loop {
            if let Some(message) = next_message(&mut state) {
                return Ok(message);
            }
            if state.senders_closed {
                return Err(mpsc::RecvTimeoutError::Disconnected);
            }

            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(mpsc::RecvTimeoutError::Timeout);
            };
            let (next_state, wait_result) = self
                .queue
                .available
                .wait_timeout(state, remaining)
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;
            state = next_state;
            if wait_result.timed_out() {
                if let Some(message) = next_message(&mut state) {
                    return Ok(message);
                }
                return Err(mpsc::RecvTimeoutError::Timeout);
            }
        }
    }
}

impl Drop for ResponseReceiver {
    fn drop(&mut self) {
        if let Ok(mut state) = self.queue.state.lock() {
            state.receiver_open = false;
        }
        self.queue.available.notify_all();
    }
}

struct SenderLifecycle {
    queue: Arc<ResponseQueue>,
}

impl Drop for SenderLifecycle {
    fn drop(&mut self) {
        if let Ok(mut state) = self.queue.state.lock() {
            state.senders_closed = true;
        }
        self.queue.available.notify_all();
    }
}

#[derive(Default)]
struct ResponseQueue {
    state: Mutex<ResponseQueueState>,
    available: Condvar,
}

impl ResponseQueue {
    fn push_response(&self, response: ServerMessage) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state.receiver_open || state.responses.len() >= MAX_PENDING_RESPONSES {
            return false;
        }

        state.responses.push_back(response);
        drop(state);
        self.available.notify_one();
        true
    }

    fn push_workspace_change(&self, workspace_ids: &[u64]) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state.receiver_open {
            return false;
        }

        state
            .workspace_changes
            .extend(workspace_ids.iter().copied());
        drop(state);
        self.available.notify_one();
        true
    }

    fn push_core_event(&self, event: core::events::CoreEvent) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state.receiver_open {
            return false;
        }
        match event {
            core::events::CoreEvent::LanguageServerDiagnosticsChanged(diagnostics) => {
                if state.language_server_diagnostics_resync {
                    return true;
                }
                let key = DiagnosticKey {
                    workspace_id: diagnostics.workspace_id.value(),
                    path: diagnostics.path.clone(),
                    server_id: diagnostics.server_id.clone(),
                };
                if !state.language_server_diagnostics.contains_key(&key) {
                    if state.language_server_diagnostic_order.len() >= MAX_PENDING_LSP_DIAGNOSTICS {
                        state.language_server_diagnostics.clear();
                        state.language_server_diagnostic_order.clear();
                        state.language_server_diagnostics_resync = true;
                        drop(state);
                        self.available.notify_one();
                        return true;
                    }
                    state
                        .language_server_diagnostic_order
                        .push_back(key.clone());
                }
                state.language_server_diagnostics.insert(key, diagnostics);
            }
            core::events::CoreEvent::LanguageServerStatusChanged { server_id } => {
                let ResponseQueueState {
                    language_server_statuses,
                    language_server_status_order,
                    ..
                } = &mut *state;
                insert_server_signal(
                    language_server_statuses,
                    language_server_status_order,
                    server_id,
                );
            }
            core::events::CoreEvent::LanguageServerLogAvailable { server_id } => {
                let ResponseQueueState {
                    language_server_logs,
                    language_server_log_order,
                    ..
                } = &mut *state;
                insert_server_signal(language_server_logs, language_server_log_order, server_id);
            }
            core::events::CoreEvent::ToolingCapabilitiesChanged { revision } => {
                state.tooling_capability_revision = Some(
                    state
                        .tooling_capability_revision
                        .map_or(revision, |current| current.max(revision)),
                );
            }
        }
        drop(state);
        self.available.notify_one();
        true
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DiagnosticKey {
    workspace_id: u64,
    path: String,
    server_id: String,
}

struct ResponseQueueState {
    receiver_open: bool,
    responses: VecDeque<ServerMessage>,
    senders_closed: bool,
    workspace_changes: HashSet<u64>,
    language_server_diagnostics:
        HashMap<DiagnosticKey, core::events::LanguageServerDiagnosticsChanged>,
    language_server_diagnostic_order: VecDeque<DiagnosticKey>,
    language_server_diagnostics_resync: bool,
    language_server_statuses: HashSet<String>,
    language_server_status_order: VecDeque<String>,
    language_server_logs: HashSet<String>,
    language_server_log_order: VecDeque<String>,
    tooling_capability_revision: Option<u64>,
}

impl Default for ResponseQueueState {
    fn default() -> Self {
        Self {
            receiver_open: true,
            responses: VecDeque::new(),
            senders_closed: false,
            workspace_changes: HashSet::new(),
            language_server_diagnostics: HashMap::new(),
            language_server_diagnostic_order: VecDeque::new(),
            language_server_diagnostics_resync: false,
            language_server_statuses: HashSet::new(),
            language_server_status_order: VecDeque::new(),
            language_server_logs: HashSet::new(),
            language_server_log_order: VecDeque::new(),
            tooling_capability_revision: None,
        }
    }
}

fn next_message(state: &mut ResponseQueueState) -> Option<ServerMessage> {
    if let Some(response) = state.responses.pop_front() {
        return Some(response);
    }
    if std::mem::take(&mut state.language_server_diagnostics_resync) {
        return Some(ServerMessage::language_server_diagnostics_resync());
    }
    while let Some(key) = state.language_server_diagnostic_order.pop_front() {
        if let Some(diagnostics) = state.language_server_diagnostics.remove(&key) {
            return Some(ServerMessage::language_server_diagnostics_changed(
                diagnostics,
            ));
        }
    }
    while let Some(server_id) = state.language_server_status_order.pop_front() {
        if state.language_server_statuses.remove(&server_id) {
            return Some(ServerMessage::language_server_status_changed(server_id));
        }
    }
    while let Some(server_id) = state.language_server_log_order.pop_front() {
        if state.language_server_logs.remove(&server_id) {
            return Some(ServerMessage::language_server_log_available(server_id));
        }
    }
    if let Some(revision) = state.tooling_capability_revision.take() {
        return Some(ServerMessage::tooling_capabilities_changed(revision));
    }
    if state.workspace_changes.is_empty() {
        return None;
    }

    let mut workspace_ids = state.workspace_changes.drain().collect::<Vec<_>>();
    workspace_ids.sort_unstable();
    Some(ServerMessage::workspace_changed(workspace_ids))
}

fn insert_server_signal(
    pending: &mut HashSet<String>,
    order: &mut VecDeque<String>,
    server_id: String,
) {
    if !pending.insert(server_id.clone()) {
        return;
    }
    if order.len() >= MAX_PENDING_LSP_SERVERS
        && let Some(oldest) = order.pop_front()
    {
        pending.remove(&oldest);
    }
    order.push_back(server_id);
}

pub(crate) fn channel(
    stream: &UnixStream,
) -> io::Result<(ResponseSender, ResponseReceiver, Arc<UnixStream>)> {
    let queue = Arc::new(ResponseQueue::default());
    let lifecycle = Arc::new(SenderLifecycle {
        queue: Arc::clone(&queue),
    });
    let shutdown = Arc::new(stream.try_clone()?);

    Ok((
        ResponseSender {
            queue: Arc::clone(&queue),
            _lifecycle: lifecycle,
            shutdown: Arc::clone(&shutdown),
        },
        ResponseReceiver { queue },
        shutdown,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn response_backpressure_closes_the_connection() {
        let (stream, mut peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, _receiver, _) = channel(&stream).expect("response channel should open");

        for id in 0..=MAX_PENDING_RESPONSES {
            responses.send(ServerMessage::ok(id as u64, true));
        }

        peer.set_read_timeout(Some(Duration::from_secs(1)))
            .expect("read timeout should be set");
        let mut byte = [0];

        assert_eq!(
            peer.read(&mut byte).expect("peer should observe shutdown"),
            0
        );
    }

    #[test]
    fn workspace_changes_coalesce_without_using_response_capacity() {
        let (stream, mut peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) = channel(&stream).expect("response channel should open");

        for id in 0..MAX_PENDING_RESPONSES {
            assert!(responses.try_send(ServerMessage::ok(id as u64, true)));
        }
        for workspace_id in 1..=MAX_PENDING_RESPONSES as u64 {
            assert!(responses.notify_workspace_changed(&[workspace_id]));
        }

        for _ in 0..MAX_PENDING_RESPONSES {
            assert!(matches!(receiver.recv(), Ok(ServerMessage::Response(_))));
        }
        let notification = receiver.recv().expect("notification should remain queued");
        assert!(matches!(notification, ServerMessage::Notification(_)));

        peer.set_nonblocking(true)
            .expect("peer should become nonblocking");
        let mut byte = [0];
        assert_eq!(
            peer.read(&mut byte)
                .expect_err("open idle connection should not be readable")
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn language_server_events_coalesce_by_document_and_server() {
        let (stream, _peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) = channel(&stream).expect("response channel should open");
        responses.notify_core_event(diagnostics_event(1));
        responses.notify_core_event(diagnostics_event(2));
        for _ in 0..3 {
            responses.notify_core_event(core::events::CoreEvent::LanguageServerStatusChanged {
                server_id: "rust-analyzer".to_owned(),
            });
        }

        let diagnostics = serde_json::to_value(receiver.recv().unwrap()).unwrap();
        let status = serde_json::to_value(receiver.recv().unwrap()).unwrap();
        assert_eq!(diagnostics["version"], 2);
        assert_eq!(diagnostics["diagnostics"][0]["message"], "version 2");
        assert_eq!(status["event"], "languageServerStatusChanged");
        assert!(receiver.recv_timeout(Duration::from_millis(25)).is_err());
    }

    #[test]
    fn diagnostic_overflow_coalesces_to_a_bounded_resync_signal() {
        let (stream, _peer) = UnixStream::pair().expect("socket pair should open");
        let (responses, receiver, _) = channel(&stream).expect("response channel should open");

        for workspace_id in 0..MAX_PENDING_LSP_DIAGNOSTICS {
            let mut event = diagnostics_event(1);
            let core::events::CoreEvent::LanguageServerDiagnosticsChanged(diagnostics) = &mut event
            else {
                unreachable!();
            };
            diagnostics.workspace_id = core::tree::WorkspaceId::new(workspace_id as u64 + 1);
            assert!(responses.notify_core_event(event));
        }
        let mut overflow = diagnostics_event(2);
        let core::events::CoreEvent::LanguageServerDiagnosticsChanged(diagnostics) = &mut overflow
        else {
            unreachable!();
        };
        diagnostics.workspace_id = core::tree::WorkspaceId::new(10_000);
        assert!(responses.notify_core_event(overflow));

        let notification = serde_json::to_value(receiver.recv().unwrap()).unwrap();
        assert_eq!(notification["event"], "languageServerDiagnosticsResync");
        assert!(receiver.recv_timeout(Duration::from_millis(25)).is_err());
    }

    fn diagnostics_event(version: i64) -> core::events::CoreEvent {
        core::events::CoreEvent::LanguageServerDiagnosticsChanged(
            core::events::LanguageServerDiagnosticsChanged {
                workspace_id: core::tree::WorkspaceId::new(1),
                path: "src/lib.rs".to_owned(),
                server_id: "rust-analyzer".to_owned(),
                generation: 4,
                version,
                diagnostics: vec![core::language_servers::LanguageServerDiagnostic {
                    range: core::language_servers::LanguageServerRange {
                        start: core::language_servers::LanguageServerPosition {
                            line: 0,
                            character: 0,
                        },
                        end: core::language_servers::LanguageServerPosition {
                            line: 0,
                            character: 1,
                        },
                    },
                    severity: None,
                    message: format!("version {version}"),
                    source: None,
                    code: None,
                }],
            },
        )
    }
}

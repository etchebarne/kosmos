use std::collections::{HashSet, VecDeque};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Condvar, Mutex, mpsc};

#[cfg(test)]
use std::time::{Duration, Instant};

use crate::ipc::messages::envelope::ServerMessage;

const MAX_PENDING_RESPONSES: usize = 32;

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
}

struct ResponseQueueState {
    receiver_open: bool,
    responses: VecDeque<ServerMessage>,
    senders_closed: bool,
    workspace_changes: HashSet<u64>,
}

impl Default for ResponseQueueState {
    fn default() -> Self {
        Self {
            receiver_open: true,
            responses: VecDeque::new(),
            senders_closed: false,
            workspace_changes: HashSet::new(),
        }
    }
}

fn next_message(state: &mut ResponseQueueState) -> Option<ServerMessage> {
    if let Some(response) = state.responses.pop_front() {
        return Some(response);
    }
    if state.workspace_changes.is_empty() {
        return None;
    }

    let mut workspace_ids = state.workspace_changes.drain().collect::<Vec<_>>();
    workspace_ids.sort_unstable();
    Some(ServerMessage::workspace_changed(workspace_ids))
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
}

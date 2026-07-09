use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, mpsc};

use crate::ipc::messages::envelope::ServerMessage;

const MAX_PENDING_RESPONSES: usize = 32;

#[derive(Clone)]
pub(crate) struct ResponseSender {
    responses: mpsc::SyncSender<ServerMessage>,
    shutdown: Arc<UnixStream>,
}

impl ResponseSender {
    pub(crate) fn send(&self, response: ServerMessage) {
        if self.responses.try_send(response).is_err() {
            self.shutdown();
        }
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.shutdown.shutdown(Shutdown::Both);
    }
}

pub(crate) fn channel(
    stream: &UnixStream,
) -> io::Result<(
    ResponseSender,
    mpsc::Receiver<ServerMessage>,
    Arc<UnixStream>,
)> {
    let (responses, receiver) = mpsc::sync_channel(MAX_PENDING_RESPONSES);
    let shutdown = Arc::new(stream.try_clone()?);

    Ok((
        ResponseSender {
            responses,
            shutdown: Arc::clone(&shutdown),
        },
        receiver,
        shutdown,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::Duration;

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
}

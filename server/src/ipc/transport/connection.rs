use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use crate::ipc::messages::envelope::{ClientMessage, ServerMessage};
use crate::ipc::router;

use super::codec;

pub(crate) fn handle(stream: UnixStream, state: Arc<Mutex<core::State>>) -> io::Result<()> {
    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    while let Some(frame) = codec::read_frame(&mut reader)? {
        if frame.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<ClientMessage>(&frame) {
            Ok(ClientMessage::Request(request)) => {
                let mut state = state
                    .lock()
                    .map_err(|_| io::Error::other("IPC state mutex was poisoned"))?;

                router::route(&mut state, request)
            }
            Err(error) => ServerMessage::error(0, "ipc.invalid_message", error.to_string()),
        };

        codec::write_message(&mut writer, &response)?;
    }

    Ok(())
}

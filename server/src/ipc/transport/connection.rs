use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use crate::ipc::messages::envelope::{ClientMessage, ServerMessage};
use crate::ipc::router;

use super::codec;

pub(crate) fn handle(
    stream: UnixStream,
    state: Arc<Mutex<core::State>>,
    store: Arc<core::persistence::StateStore>,
) -> io::Result<()> {
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

                let request_id = request.id;
                let response = router::route(&mut state, request);

                if response.is_ok() {
                    if let Err(error) = store.save(&state) {
                        ServerMessage::error(
                            request_id,
                            "persistence.save_failed",
                            error.to_string(),
                        )
                    } else {
                        response
                    }
                } else {
                    response
                }
            }
            Err(error) => ServerMessage::error(0, "ipc.invalid_message", error.to_string()),
        };

        codec::write_message(&mut writer, &response)?;
    }

    Ok(())
}

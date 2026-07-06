use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use crate::ipc::messages::envelope::{ClientMessage, Domain, RequestEnvelope, ServerMessage};
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

                let response = router::route(&mut state, &request);

                persist_successful_request(&request, response, &state, &store)
            }
            Err(error) => ServerMessage::error(0, "ipc.invalid_message", error.to_string()),
        };

        codec::write_message(&mut writer, &response)?;
    }

    Ok(())
}

fn persist_successful_request(
    request: &RequestEnvelope,
    response: ServerMessage,
    state: &core::State,
    store: &core::persistence::StateStore,
) -> ServerMessage {
    if !response.is_ok() {
        return response;
    }

    let result = match persistence_mode(request) {
        PersistenceMode::None => Ok(()),
        PersistenceMode::ActiveWorkspace => store.save_active_workspace(state),
        PersistenceMode::Full => store.save(state),
    };

    match result {
        Ok(()) => response,
        Err(error) => {
            ServerMessage::error(request.id, "persistence.save_failed", error.to_string())
        }
    }
}

fn persistence_mode(request: &RequestEnvelope) -> PersistenceMode {
    match (request.domain, request.action.as_str()) {
        (Domain::Workspace, "list") => PersistenceMode::None,
        (Domain::Workspace, "activate") => PersistenceMode::ActiveWorkspace,
        (Domain::FileTree, "get") => PersistenceMode::None,
        _ => PersistenceMode::Full,
    }
}

enum PersistenceMode {
    None,
    ActiveWorkspace,
    Full,
}

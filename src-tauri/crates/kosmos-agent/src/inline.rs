use std::io;
use std::io::Stdout;
use std::sync::Arc;
use std::sync::Mutex;

use kosmos_core::EventSink;
use kosmos_protocol::events::Event;
use kosmos_protocol::framing;
use kosmos_protocol::requests::{Request, RequestMessage, ResponseMessage};

use crate::dispatch::run_dispatch;
use crate::{agent_data_dir, ensure_node_runtime, AgentState};

struct StdoutEventSink {
    writer: Arc<Mutex<Stdout>>,
}

impl EventSink for StdoutEventSink {
    fn emit(&self, event: Event) {
        if let Ok(json) = serde_json::to_string(&event) {
            if let Ok(mut w) = self.writer.lock() {
                let _ = framing::write_message(&mut *w, &json);
            }
        }
    }
}

fn send_response(writer: &Arc<Mutex<Stdout>>, response: &ResponseMessage) {
    if let Ok(json) = serde_json::to_string(response) {
        if let Ok(mut w) = writer.lock() {
            let _ = framing::write_message(&mut *w, &json);
        }
    }
}

#[cfg(not(unix))]
pub(crate) async fn inline_main() {
    let data_dir = agent_data_dir();
    ensure_node_runtime(&data_dir);
    let servers_dir = data_dir.join("servers");
    std::fs::create_dir_all(&servers_dir).ok();

    let stdout_writer: Arc<Mutex<Stdout>> = Arc::new(Mutex::new(io::stdout()));

    let events: Arc<dyn EventSink> = Arc::new(StdoutEventSink {
        writer: stdout_writer.clone(),
    });

    let fff = kosmos_core::fff_picker::FffPicker::new(data_dir.join("fff-frecency.lmdb"))
        .unwrap_or_else(|e| {
            panic!("Failed to initialize fff frecency database: {e}");
        });

    let state = Arc::new(AgentState {
        watcher: kosmos_core::watcher::WatcherManager::new(events.clone()),
        terminals: kosmos_core::terminal::TerminalManager::new(events.clone()),
        lsp: kosmos_core::lsp::LspManager::new(events, servers_dir, None),
        fff,
    });

    let writer = stdout_writer.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Request)>();

    tokio::task::spawn_blocking(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            let msg = match framing::read_message(&mut reader) {
                Ok(msg) => msg,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    tracing::warn!("Read error: {e}");
                    break;
                }
            };
            let req_msg: RequestMessage = match serde_json::from_str(&msg) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Parse error: {e}");
                    continue;
                }
            };
            if tx.send((req_msg.id, req_msg.request)).is_err() {
                break;
            }
        }
    });

    while let Some((id, request)) = rx.recv().await {
        let state = state.clone();
        let writer = writer.clone();
        tokio::spawn(async move {
            let response = run_dispatch(state, id, request).await;
            send_response(&writer, &response);
        });
    }
}

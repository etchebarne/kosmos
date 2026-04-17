use std::path::{Path, PathBuf};
use std::sync::Arc;

use kosmos_core::EventSink;
use kosmos_protocol::events::Event;
use kosmos_protocol::framing::{async_read_message, async_write_message};
use kosmos_protocol::requests::RequestMessage;
use tokio::sync::broadcast;

use crate::dispatch::run_dispatch;
use crate::{agent_data_dir, ensure_node_runtime, AgentState};

struct BroadcastEventSink {
    tx: broadcast::Sender<String>,
}

impl EventSink for BroadcastEventSink {
    fn emit(&self, event: Event) {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = self.tx.send(json);
        }
    }
}

async fn handle_client(
    stream: tokio::net::UnixStream,
    state: Arc<AgentState>,
    event_tx: broadcast::Sender<String>,
) {
    let (read_half, write_half) = stream.into_split();
    let write = Arc::new(tokio::sync::Mutex::new(write_half));

    // Forward broadcast events to this client's socket
    let mut event_rx = event_tx.subscribe();
    let write_for_events = write.clone();
    let event_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(json) => {
                    let mut w = write_for_events.lock().await;
                    if async_write_message(&mut *w, &json).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Read requests from this client
    let mut reader = tokio::io::BufReader::new(read_half);
    loop {
        let msg = match async_read_message(&mut reader).await {
            Ok(m) => m,
            Err(_) => break, // Client disconnected
        };

        let req_msg: RequestMessage = match serde_json::from_str(&msg) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Parse error: {e}");
                continue;
            }
        };

        let state = state.clone();
        let write = write.clone();
        tokio::spawn(async move {
            let response = run_dispatch(state, req_msg.id, req_msg.request).await;
            if let Ok(json) = serde_json::to_string(&response) {
                let mut w = write.lock().await;
                let _ = async_write_message(&mut *w, &json).await;
            }
        });
    }

    event_task.abort();
    tracing::info!("Client disconnected");
}

pub(crate) async fn daemon_main() {
    // Detach from the controlling terminal so the daemon survives when
    // the SSH/WSL session ends.
    #[cfg(unix)]
    unsafe {
        libc::setsid();
    }

    let data_dir = agent_data_dir();
    let sock_path = data_dir.join("agent.sock");

    // Write PID + binary identity so clients can detect stale daemons
    let _ = std::fs::write(data_dir.join("daemon.pid"), std::process::id().to_string());
    let _ = std::fs::write(data_dir.join("daemon.identity"), binary_identity());

    // Remove stale socket from a previous daemon
    let _ = std::fs::remove_file(&sock_path);

    // Bind the socket FIRST so the relay client can connect immediately
    // while we finish the heavier initialization below. Connections queue
    // in the kernel until we call accept().
    let listener = match tokio::net::UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind socket: {e}");
            return;
        }
    };

    // Now do the heavier setup — relay is already able to connect.
    ensure_node_runtime(&data_dir);
    let servers_dir = data_dir.join("servers");
    std::fs::create_dir_all(&servers_dir).ok();

    let (event_tx, _) = broadcast::channel::<String>(8192);

    let events: Arc<dyn EventSink> = Arc::new(BroadcastEventSink {
        tx: event_tx.clone(),
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

    tracing::info!("Listening on {}", sock_path.display());

    // Clean up socket + metadata on exit
    struct DaemonCleanup(PathBuf);
    impl Drop for DaemonCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("agent.sock"));
            let _ = std::fs::remove_file(self.0.join("daemon.pid"));
            let _ = std::fs::remove_file(self.0.join("daemon.identity"));
        }
    }
    let _guard = DaemonCleanup(data_dir.clone());

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tracing::info!("Client connected");
                let state = state.clone();
                let event_tx = event_tx.clone();
                tokio::spawn(async move {
                    handle_client(stream, state, event_tx).await;
                });
            }
            Err(e) => {
                tracing::warn!("Accept error: {e}");
            }
        }
    }
}

pub(crate) fn is_daemon_running(sock_path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(sock_path).is_ok()
}

/// Returns a fingerprint of the current binary (size + mtime).
/// Used to detect when the daemon is running from a stale binary.
fn binary_identity() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return String::new();
    };
    let Ok(meta) = std::fs::metadata(&exe) else {
        return String::new();
    };
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{size}:{mtime}")
}

/// Kill a running daemon using its PID file.
fn kill_stale_daemon(data_dir: &Path) {
    if let Ok(pid_str) = std::fs::read_to_string(data_dir.join("daemon.pid")) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            tracing::info!("Killing stale daemon (pid {pid})");
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            // Brief wait for the daemon to clean up its socket
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    let _ = std::fs::remove_file(data_dir.join("agent.sock"));
    let _ = std::fs::remove_file(data_dir.join("daemon.pid"));
    let _ = std::fs::remove_file(data_dir.join("daemon.identity"));
}

pub(crate) fn ensure_daemon(data_dir: &Path) {
    let sock_path = data_dir.join("agent.sock");

    if is_daemon_running(&sock_path) {
        // Check if the running daemon matches the current binary.
        // Old daemons won't have an identity file — treat as stale.
        let current = binary_identity();
        let matches = std::fs::read_to_string(data_dir.join("daemon.identity"))
            .map(|s| !current.is_empty() && s.trim() == current)
            .unwrap_or(false);

        if matches {
            return;
        }

        tracing::info!("Daemon binary outdated, restarting");
        kill_stale_daemon(data_dir);
    }

    // Remove stale socket
    let _ = std::fs::remove_file(&sock_path);

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("Failed to get current exe: {e}");
            std::process::exit(1);
        }
    };

    // Open a log file for the daemon's stderr
    let log_path = data_dir.join("daemon.log");
    let stderr_target = std::fs::File::create(&log_path)
        .map(std::process::Stdio::from)
        .unwrap_or_else(|_| std::process::Stdio::null());

    if let Err(e) = std::process::Command::new(exe)
        .arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr_target)
        .spawn()
    {
        tracing::error!("Failed to start daemon: {e}");
        std::process::exit(1);
    }

    // Wait for the daemon socket to appear. Use fast polling initially
    // (the socket binds early in daemon startup), then back off.
    for i in 0..100 {
        let delay = if i < 20 { 10 } else { 50 };
        std::thread::sleep(std::time::Duration::from_millis(delay));
        if is_daemon_running(&sock_path) {
            return;
        }
    }

    tracing::error!("Daemon did not start within 5s");
    std::process::exit(1);
}

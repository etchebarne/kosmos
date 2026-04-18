use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kosmos_core::EventSink;
use kosmos_protocol::events::Event;
use kosmos_protocol::requests::{Request, RequestMessage, ResponseMessage};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, Mutex};

use super::connection::ConnectionType;

use kosmos_core::configure_child_process;

const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// A connection to a remote kosmos-agent process.
pub struct RemoteAgent {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<ResponseMessage>>>>,
    alive: Arc<AtomicBool>,
    pub connection_type: ConnectionType,
}

impl Drop for RemoteAgent {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

impl RemoteAgent {
    pub async fn spawn(
        conn: ConnectionType,
        on_event: Arc<dyn EventSink>,
    ) -> Result<Self, String> {
        let mut child = match &conn {
            ConnectionType::Local => {
                return Err("Cannot spawn remote agent for local connection".into());
            }
            ConnectionType::Wsl { distro } => {
                let mut cmd = tokio::process::Command::new("wsl.exe");
                // bash -lic so nvm/fnm/volta setup behind the interactive guard is sourced;
                // exec so no bash wrapper lingers after the agent.
                let remote_dir = super::deploy::REMOTE_DIR;
                cmd.args([
                    "-d",
                    distro,
                    "--",
                    "bash",
                    "-lc",
                    &format!("exec ~/{remote_dir}/kosmos-agent"),
                ]);
                cmd.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                configure_child_process(&mut cmd);
                cmd.spawn()
                    .map_err(|e| format!("Failed to spawn agent: {e}"))?
            }
            ConnectionType::Ssh { host, user } => {
                let target = match user {
                    Some(u) => format!("{u}@{host}"),
                    None => host.clone(),
                };
                let remote_dir = super::deploy::REMOTE_DIR;
                let mut cmd = tokio::process::Command::new("ssh");
                cmd.args([
                    "-o", "ServerAliveInterval=15",
                    "-o", "ServerAliveCountMax=3",
                    &target,
                    &format!("~/{remote_dir}/kosmos-agent"),
                ]);
                cmd.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                cmd.spawn()
                    .map_err(|e| format!("Failed to spawn agent: {e}"))?
            }
        };

        let stdin = child.stdin.take().ok_or("Failed to take agent stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to take agent stdout")?;

        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                tracing::debug!(target: "kosmos::agent", "{trimmed}");
                            }
                        }
                    }
                }
            });
        }

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<ResponseMessage>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));


        let pending_clone = pending.clone();
        let alive_clone = alive.clone();
        tokio::spawn(async move {
            read_agent_stdout(stdout, pending_clone.clone(), on_event).await;

            // Agent exited: fail pending requests; frontend handles terminal reconnection.
            alive_clone.store(false, Ordering::SeqCst);
            tracing::warn!("Agent process exited");
            let mut p = pending_clone.lock().await;
            for (_, tx) in p.drain() {
                let _ = tx.send(ResponseMessage::err(0, "Agent connection lost".into()));
            }
        });

        let agent = Self {
            child: Mutex::new(child),
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: AtomicU64::new(1),
            pending,
            alive: alive.clone(),
            connection_type: conn,
        };

        // Periodic Ping prevents idle pipe closure. id=0 → no receiver, response discarded.
        let keepalive_stdin = agent.stdin.clone();
        let keepalive_alive = alive;
        tokio::spawn(async move {
            let msg = RequestMessage {
                id: 0,
                request: Request::Ping,
            };
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(_) => return,
            };
            let framed = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);
            let bytes = framed.as_bytes();

            loop {
                tokio::time::sleep(KEEPALIVE_INTERVAL).await;
                if !keepalive_alive.load(Ordering::SeqCst) {
                    break;
                }
                let mut stdin = keepalive_stdin.lock().await;
                if stdin.write_all(bytes).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        Ok(agent)
    }

    /// Check if the agent process is still alive.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Send a request and wait for the response with the default timeout.
    pub async fn request(&self, request: Request) -> Result<serde_json::Value, String> {
        self.request_with_timeout(request, REQUEST_TIMEOUT).await
    }

    /// Send a request without waiting for a response (fire-and-forget).
    /// Used for operations like lsp_send where the response is meaningless.
    pub async fn notify(&self, request: Request) -> Result<(), String> {
        if !self.is_alive() {
            return Err("Agent connection is dead".into());
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = RequestMessage { id, request };
        let json = serde_json::to_string(&msg).map_err(|e| e.to_string())?;

        let mut stdin = self.stdin.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", json.len());
        stdin
            .write_all(header.as_bytes())
            .await
            .map_err(|e| format!("Agent write failed: {e}"))?;
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| format!("Agent write failed: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Agent flush failed: {e}"))?;
        Ok(())
    }

    /// Send a request and wait for the response with a custom timeout.
    pub async fn request_with_timeout(
        &self,
        request: Request,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        if !self.is_alive() {
            return Err("Agent connection is dead".into());
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = RequestMessage { id, request };
        let json = serde_json::to_string(&msg).map_err(|e| e.to_string())?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        {
            let mut stdin = self.stdin.lock().await;
            let header = format!("Content-Length: {}\r\n\r\n", json.len());
            if let Err(e) = stdin.write_all(header.as_bytes()).await {
                self.pending.lock().await.remove(&id);
                return Err(format!("Agent write failed: {e}"));
            }
            if let Err(e) = stdin.write_all(json.as_bytes()).await {
                self.pending.lock().await.remove(&id);
                return Err(format!("Agent write failed: {e}"));
            }
            if let Err(e) = stdin.flush().await {
                self.pending.lock().await.remove(&id);
                return Err(format!("Agent flush failed: {e}"));
            }
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => {
                if let Some(err) = response.error {
                    Err(err)
                } else {
                    Ok(response.result.unwrap_or(serde_json::Value::Null))
                }
            }
            Ok(Err(_)) => Err("Agent connection lost".into()),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err("Agent request timed out".into())
            }
        }
    }
}

async fn read_agent_stdout(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<ResponseMessage>>>>,
    on_event: Arc<dyn EventSink>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => return,
                Err(_) => return,
                Ok(_) => {}
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(val) = trimmed.strip_prefix("Content-Length:") {
                content_length = val.trim().parse().ok();
            }
        }

        let length = match content_length {
            Some(l) if l <= MAX_MESSAGE_SIZE => l,
            _ => continue,
        };

        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).await.is_err() {
            return;
        }

        let text = match String::from_utf8(body) {
            Ok(t) => t,
            Err(_) => continue,
        };

        if let Ok(resp) = serde_json::from_str::<ResponseMessage>(&text) {
            let mut p = pending.lock().await;
            if let Some(tx) = p.remove(&resp.id) {
                let _ = tx.send(resp);
            }
            continue;
        }

        if let Ok(event) = serde_json::from_str::<Event>(&text) {
            on_event.emit(event);
        }
    }
}

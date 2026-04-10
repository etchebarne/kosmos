pub mod detection;
pub mod framing;
#[cfg(feature = "installer")]
pub mod installer;
pub mod registry;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;

use kosmos_protocol::events::Event;
use kosmos_protocol::types::{
    DetectedProject, InstalledServer, LspStartResult, RegistryEntry, ServerAvailability,
};

use crate::{CoreError, EventSink};

use detection::{
    check_availability, find_project_root, resolve_command, resolve_server_for_language,
    scan_workspace_projects, server_language_group,
};

#[cfg(target_os = "windows")]
use crate::CREATE_NO_WINDOW;

struct LspServer {
    #[allow(dead_code)]
    child: Child,
    stdin: ChildStdin,
    #[allow(dead_code)]
    language_id: String,
}

pub struct LspManager {
    servers: Arc<Mutex<HashMap<String, Arc<Mutex<LspServer>>>>>,
    events: Arc<dyn EventSink>,
    servers_dir: PathBuf,
    custom_registry_path: Option<PathBuf>,
}

fn make_server_id(workspace_path: &str, language_id: &str) -> String {
    let safe_path = workspace_path.replace('\\', "/");
    format!("{language_id}:{safe_path}")
}

fn server_id_workspace(server_id: &str) -> Option<&str> {
    server_id.split_once(':').map(|(_, path)| path)
}

fn spawn_server(
    command: &str,
    args: &[String],
    working_dir: &str,
) -> std::io::Result<(Child, Option<tokio::process::ChildStderr>)> {
    #[cfg(target_os = "windows")]
    let mut cmd = if command.ends_with(".cmd") || command.ends_with(".bat") {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        tokio::process::Command::new(command)
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = tokio::process::Command::new(command);

    cmd.args(args)
        .current_dir(working_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(target_os = "linux")]
    crate::sanitize_child_env(&mut cmd);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd.spawn()?;
    let stderr = child.stderr.take();
    Ok((child, stderr))
}

impl LspManager {
    pub fn new(
        events: Arc<dyn EventSink>,
        servers_dir: PathBuf,
        custom_registry_path: Option<PathBuf>,
    ) -> Self {
        Self {
            servers: Arc::new(Mutex::new(HashMap::new())),
            events,
            servers_dir,
            custom_registry_path,
        }
    }

    #[tracing::instrument(skip(self), fields(server_id))]
    pub async fn start(
        &self,
        workspace_path: &str,
        language_id: &str,
    ) -> Result<LspStartResult, CoreError> {
        let group = server_language_group(language_id);
        let server_id = make_server_id(workspace_path, group);

        // Check if already running
        {
            let servers = self.servers.lock().await;
            if servers.contains_key(&server_id) {
                let server_name = detection::server_name_for_language(language_id)
                    .unwrap_or("unknown")
                    .to_string();
                return Ok(LspStartResult {
                    server_id,
                    server_name,
                    server_language: group.to_string(),
                });
            }
        }

        let config = resolve_server_for_language(language_id)
            .ok_or_else(|| CoreError::Lsp(format!("No language server configured for {language_id}")))?;

        let resolved_command = resolve_command(&self.servers_dir, &config.command);

        let (mut child, stderr) =
            spawn_server(&resolved_command, &config.args, workspace_path)
                .map_err(|e| CoreError::Lsp(format!("Failed to start {}: {e}", config.command)))?;

        let stdin = child.stdin.take().ok_or_else(|| CoreError::Lsp("Failed to take stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| CoreError::Lsp("Failed to take stdout".into()))?;

        // Buffer stderr so early-exit errors can be reported to the frontend
        let stderr_buffer = Arc::new(std::sync::Mutex::new(String::new()));

        // Log stderr AND buffer it
        if let Some(stderr) = stderr {
            let server_name = config.server_name.clone();
            let buffer = stderr_buffer.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                tracing::warn!(server = %server_name, "{trimmed}");
                                if let Ok(mut buf) = buffer.lock() {
                                    if buf.len() < 2048 {
                                        if !buf.is_empty() {
                                            buf.push('\n');
                                        }
                                        buf.push_str(trimmed);
                                    }
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        // Read stdout and emit messages
        let events = self.events.clone();
        let sid = server_id.clone();
        let servers_ref = self.servers.clone();
        let stderr_buf = stderr_buffer;
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match framing::read_message(&mut reader).await {
                    Ok(msg) => {
                        events.emit(Event::LspMessage {
                            server_id: sid.clone(),
                            message: msg,
                        });
                    }
                    Err(e) => {
                        servers_ref.lock().await.remove(&sid);
                        let error = if e == "EOF" {
                            // Give stderr a moment to flush before reading the buffer
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            match stderr_buf.lock() {
                                Ok(buf) if !buf.is_empty() => Some(buf.clone()),
                                _ => None,
                            }
                        } else {
                            Some(e)
                        };
                        events.emit(Event::LspStopped {
                            server_id: sid.clone(),
                            error,
                        });
                        break;
                    }
                }
            }
        });

        let server = Arc::new(Mutex::new(LspServer {
            child,
            stdin,
            language_id: group.to_string(),
        }));

        self.servers
            .lock()
            .await
            .insert(server_id.clone(), server);

        Ok(LspStartResult {
            server_id,
            server_name: config.server_name,
            server_language: group.to_string(),
        })
    }

    #[tracing::instrument(skip(self, message))]
    pub async fn send(&self, server_id: &str, message: &str) -> Result<(), CoreError> {
        let server_arc = {
            let servers = self.servers.lock().await;
            servers
                .get(server_id)
                .cloned()
                .ok_or_else(|| CoreError::NotFound(format!("Server {server_id}")))?
        };

        let result = {
            let mut server = server_arc.lock().await;
            framing::write_message(&mut server.stdin, message)
                .await
                .map_err(|e| CoreError::Lsp(e))
        };

        if let Err(ref e) = result {
            self.servers.lock().await.remove(server_id);
            self.events.emit(Event::LspStopped {
                server_id: server_id.to_string(),
                error: Some(e.to_string()),
            });
        }

        result
    }

    #[tracing::instrument(skip(self))]
    pub async fn stop(&self, server_id: &str) -> Result<(), CoreError> {
        let server_arc = {
            let mut servers = self.servers.lock().await;
            servers.remove(server_id)
        };
        if let Some(arc) = server_arc {
            let mut server = arc.lock().await;
            let _ = server.child.kill().await;
        }
        Ok(())
    }

    pub async fn stop_workspace(&self, workspace_path: &str) -> Result<(), CoreError> {
        let safe_path = workspace_path.replace('\\', "/");

        let removed: Vec<Arc<Mutex<LspServer>>> = {
            let mut servers = self.servers.lock().await;
            let keys_to_remove: Vec<String> = servers
                .keys()
                .filter(|k| server_id_workspace(k) == Some(&safe_path))
                .cloned()
                .collect();
            keys_to_remove
                .into_iter()
                .filter_map(|key| servers.remove(&key))
                .collect()
        };

        for arc in removed {
            let mut server = arc.lock().await;
            let _ = server.child.kill().await;
        }
        Ok(())
    }

    // ── Detection / scanning (delegates to detection module) ──

    pub fn check_availability(&self, workspace_path: &str) -> Vec<ServerAvailability> {
        check_availability(&self.servers_dir, workspace_path)
    }

    pub fn scan_projects(&self, workspace_path: &str) -> Vec<DetectedProject> {
        scan_workspace_projects(&self.servers_dir, workspace_path)
    }

    pub fn resolve_root(
        file_path: &str,
        language_id: &str,
        workspace_path: &str,
    ) -> String {
        find_project_root(file_path, language_id, workspace_path)
    }

    pub fn language_groups() -> HashMap<String, String> {
        detection::language_groups()
    }

    pub fn companion_servers() -> HashMap<String, Vec<String>> {
        detection::companion_servers()
    }

    // ── Registry / installer ──

    fn load_full_registry(&self) -> Vec<RegistryEntry> {
        let base = registry::load_registry();
        match &self.custom_registry_path {
            Some(path) => {
                let custom = registry::load_custom_entries(path);
                registry::merge_registries(base, custom)
            }
            None => base,
        }
    }

    pub fn registry_list(&self) -> Vec<RegistryEntry> {
        self.load_full_registry()
    }

    pub fn registry_search(&self, query: &str) -> Vec<RegistryEntry> {
        registry::search_in(self.load_full_registry(), query)
    }

    #[cfg(feature = "installer")]
    pub fn installed_list(&self) -> Vec<InstalledServer> {
        installer::list_installed(&self.servers_dir)
    }

    #[cfg(feature = "installer")]
    pub async fn install_server(&self, name: &str) -> Result<InstalledServer, String> {
        let entries = self.load_full_registry();
        let entry = entries
            .iter()
            .find(|e| e.name == name)
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.bin.as_deref() == Some(name))
            })
            .cloned()
            .ok_or_else(|| format!("Server '{name}' not found in registry"))?;

        installer::install_server(&self.servers_dir, &entry).await
    }

    #[cfg(feature = "installer")]
    pub fn uninstall_server(&self, name: &str) -> Result<(), String> {
        installer::uninstall_server(&self.servers_dir, name)
    }
}

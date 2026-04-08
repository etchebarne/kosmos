use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
use kosmos_core::CREATE_NO_WINDOW;

#[derive(Serialize)]
pub struct PluginEntry {
    manifest: serde_json::Value,
    path: String,
}

/// Returns the plugins directory inside the app data folder.
fn plugins_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("plugins");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// List all installed plugins by scanning the plugins directory for manifest.json files.
#[tauri::command]
pub async fn plugin_list(app: AppHandle) -> Result<Vec<PluginEntry>, String> {
    let dir = plugins_dir(&app)?;
    let mut entries = Vec::new();

    let read_dir = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        match std::fs::read_to_string(&manifest_path) {
            Ok(contents) => match serde_json::from_str::<serde_json::Value>(&contents) {
                Ok(manifest) => {
                    entries.push(PluginEntry {
                        manifest,
                        path: path.to_string_lossy().to_string(),
                    });
                }
                Err(e) => {
                    tracing::warn!("Invalid manifest at {}: {}", manifest_path.display(), e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read {}: {}", manifest_path.display(), e);
            }
        }
    }

    Ok(entries)
}

/// Install a plugin from a URL.
/// Supports .zip and .tar.gz archives.
/// If `plugin_id` is provided, the plugin is extracted into `plugins/<plugin_id>/`.
/// Otherwise the archive's root directory name is used.
#[tauri::command]
pub async fn plugin_install(
    app: AppHandle,
    url: String,
    plugin_id: Option<String>,
) -> Result<(), String> {
    let dir = plugins_dir(&app)?;

    // Download
    let bytes = reqwest::get(&url)
        .await
        .map_err(|e| format!("Download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;

    // Determine target directory
    let target = match &plugin_id {
        Some(id) => dir.join(id),
        None => {
            // Try to derive name from URL (last path segment without extension)
            let name = url
                .rsplit('/')
                .next()
                .unwrap_or("plugin")
                .trim_end_matches(".tar.gz")
                .trim_end_matches(".tgz")
                .trim_end_matches(".zip");
            dir.join(name)
        }
    };

    // Clean existing install
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    // Extract based on content
    if url.ends_with(".zip") {
        extract_zip(&bytes, &target)?;
    } else {
        // Assume tar.gz for everything else
        extract_tar_gz(&bytes, &target)?;
    }

    // If the archive extracted a single top-level directory, hoist its contents up
    hoist_single_child(&target)?;

    // Verify manifest exists
    if !target.join("manifest.json").exists() {
        std::fs::remove_dir_all(&target).ok();
        return Err("Plugin archive does not contain a manifest.json".into());
    }

    Ok(())
}

/// Uninstall a plugin by removing its directory.
#[tauri::command]
pub async fn plugin_uninstall(app: AppHandle, plugin_id: String) -> Result<(), String> {
    let dir = plugins_dir(&app)?.join(&plugin_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Fetch the plugin registry JSON from a URL.
#[tauri::command]
pub async fn plugin_fetch_registry(url: String) -> Result<String, String> {
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to fetch registry: {e}"))?;
    resp.text()
        .await
        .map_err(|e| format!("Failed to read registry: {e}"))
}

// ── Shell / process API for plugins ──

/// Tracks spawned child processes so plugins can write to stdin and kill them.
#[derive(Clone, Default)]
pub struct PluginProcessManager {
    processes: Arc<Mutex<HashMap<String, Arc<Mutex<Child>>>>>,
    stdin_handles: Arc<Mutex<HashMap<String, Arc<Mutex<tokio::process::ChildStdin>>>>>,
}

#[derive(Serialize, Clone)]
pub struct ShellOutput {
    stdout: String,
    stderr: String,
    code: i32,
}

/// Execute a command synchronously (wait for it to finish, return output).
#[tauri::command]
pub async fn plugin_shell_execute(
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
) -> Result<ShellOutput, String> {
    let mut cmd = Command::new(&command);
    cmd.args(&args);
    #[cfg(target_os = "linux")]
    kosmos_core::sanitize_child_env(&mut cmd);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Some(ref dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().await.map_err(|e| format!("Failed to execute: {e}"))?;

    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    })
}

/// Spawn a long-running child process.
/// Returns a process ID. stdout/stderr are streamed back as Tauri events.
#[tauri::command]
pub async fn plugin_shell_spawn(
    app: AppHandle,
    state: tauri::State<'_, PluginProcessManager>,
    pid: String,
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
) -> Result<(), String> {
    let mut cmd = Command::new(&command);
    cmd.args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "linux")]
    kosmos_core::sanitize_child_env(&mut cmd);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Some(ref dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn: {e}"))?;

    // Extract stdin before moving child into the map
    if let Some(stdin) = child.stdin.take() {
        state
            .stdin_handles
            .lock()
            .await
            .insert(pid.clone(), Arc::new(Mutex::new(stdin)));
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let child = Arc::new(Mutex::new(child));
    state
        .processes
        .lock()
        .await
        .insert(pid.clone(), child.clone());

    // Stream stdout
    if let Some(stdout) = stdout {
        let app_clone = app.clone();
        let pid_clone = pid.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = app_clone.emit(
                    &format!("plugin-process-stdout-{pid_clone}"),
                    line,
                );
            }
        });
    }

    // Stream stderr
    if let Some(stderr) = stderr {
        let app_clone = app.clone();
        let pid_clone = pid.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = app_clone.emit(
                    &format!("plugin-process-stderr-{pid_clone}"),
                    line,
                );
            }
        });
    }

    // Wait for exit in background
    {
        let app_clone = app.clone();
        let pid_clone = pid.clone();
        let state_processes = state.processes.lock().await;
        let child_ref = state_processes.get(&pid).cloned();
        drop(state_processes);

        if let Some(child_ref) = child_ref {
            let stdin_handles = state.stdin_handles.clone();
            let processes = state.processes.clone();
            tokio::spawn(async move {
                let code = {
                    let mut child = child_ref.lock().await;
                    match child.wait().await {
                        Ok(status) => status.code().unwrap_or(-1),
                        Err(_) => -1,
                    }
                };
                let _ = app_clone.emit(&format!("plugin-process-exit-{pid_clone}"), code);
                processes.lock().await.remove(&pid_clone);
                stdin_handles.lock().await.remove(&pid_clone);
            });
        }
    }

    Ok(())
}

/// Write data to a spawned process's stdin.
#[tauri::command]
pub async fn plugin_shell_write(
    state: tauri::State<'_, PluginProcessManager>,
    pid: String,
    data: String,
) -> Result<(), String> {
    let handles = state.stdin_handles.lock().await;
    let stdin = handles
        .get(&pid)
        .ok_or_else(|| format!("No process with pid {pid}"))?
        .clone();
    drop(handles);

    let mut stdin = stdin.lock().await;
    stdin
        .write_all(data.as_bytes())
        .await
        .map_err(|e| format!("Failed to write to stdin: {e}"))?;
    stdin.flush().await.map_err(|e| format!("Flush failed: {e}"))?;
    Ok(())
}

/// Kill a spawned process.
#[tauri::command]
pub async fn plugin_shell_kill(
    state: tauri::State<'_, PluginProcessManager>,
    pid: String,
) -> Result<(), String> {
    let processes = state.processes.lock().await;
    if let Some(child) = processes.get(&pid) {
        let mut child = child.lock().await;
        child.kill().await.map_err(|e| format!("Failed to kill: {e}"))?;
    }
    Ok(())
}

// ── Archive extraction helpers ──

fn extract_zip(data: &[u8], target: &std::path::Path) -> Result<(), String> {
    use std::io::{Cursor, Read};

    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("Invalid zip: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = file.enclosed_name() else {
            continue;
        };
        let out_path = target.join(name);

        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            std::fs::write(&out_path, &buf).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn extract_tar_gz(data: &[u8], target: &std::path::Path) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let decoder = GzDecoder::new(Cursor::new(data));
    let mut archive = Archive::new(decoder);

    archive.unpack(target).map_err(|e| format!("Failed to extract tar.gz: {e}"))?;
    Ok(())
}

/// If the target directory contains exactly one child directory and no files,
/// move its contents up to the target level. This handles archives that wrap
/// everything in a single root folder (common with GitHub release tarballs).
fn hoist_single_child(target: &std::path::Path) -> Result<(), String> {
    let entries: Vec<_> = std::fs::read_dir(target)
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    if entries.len() == 1 && entries[0].path().is_dir() {
        let child = entries[0].path();
        let temp = target.with_extension("__hoist_tmp");
        std::fs::rename(&child, &temp).map_err(|e| e.to_string())?;

        // Remove the now-empty target (might still have the temp dir as sibling)
        // Move temp contents into target
        for entry in std::fs::read_dir(&temp).map_err(|e| e.to_string())?.flatten() {
            let dest = target.join(entry.file_name());
            std::fs::rename(entry.path(), &dest).map_err(|e| e.to_string())?;
        }
        std::fs::remove_dir_all(&temp).ok();
    }

    Ok(())
}

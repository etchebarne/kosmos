use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use kosmos_protocol::events::Event;
use kosmos_protocol::types::ShellInfo;

use crate::{CoreError, EventSink};

struct TerminalInstance {
    writer: Box<dyn Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

pub struct TerminalManager {
    events: Arc<dyn EventSink>,
    terminals: Mutex<HashMap<String, TerminalInstance>>,
}

impl TerminalManager {
    pub fn new(events: Arc<dyn EventSink>) -> Self {
        Self {
            events,
            terminals: Mutex::new(HashMap::new()),
        }
    }

    /// List IDs of terminals whose child process is still running.
    pub fn list(&self) -> Vec<String> {
        let mut terminals = match self.terminals.lock() {
            Ok(t) => t,
            Err(_) => return vec![],
        };
        terminals.retain(|_, inst| matches!(inst.child.try_wait(), Ok(None)));
        terminals.keys().cloned().collect()
    }

    #[tracing::instrument(skip(self))]
    pub fn spawn(
        &self,
        id: String,
        program: &str,
        args: &[String],
        cwd: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), CoreError> {
        // If this terminal already exists and is alive, return success (idempotent).
        // This enables seamless reconnection: the daemon keeps the PTY alive and
        // the reconnecting client reuses it.
        {
            let mut terminals = self
                .terminals
                .lock()
                .map_err(|e| CoreError::Terminal(e.to_string()))?;
            if let Some(inst) = terminals.get_mut(&id) {
                if matches!(inst.child.try_wait(), Ok(None)) {
                    tracing::info!(id = %id, "Reattaching to existing terminal");
                    return Ok(()); // Still alive — reattach
                }
                // Child exited, remove stale entry and re-spawn below
                terminals.remove(&id);
            }
        }

        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| CoreError::Terminal(e.to_string()))?;

        let mut cmd = CommandBuilder::new(program);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        #[cfg(target_os = "linux")]
        {
            if crate::is_appimage() {
                cmd.env_remove("LD_LIBRARY_PATH");
            }
            // Deploy clipboard shims so TUI apps can read images pasted via
            // Ctrl+V. The shims serve the kosmos clipboard image for image
            // reads and fall back to the real tools for everything else.
            let shim_dir = ensure_clipboard_shims();
            if let Ok(path) = std::env::var("PATH") {
                cmd.env("PATH", format!("{shim_dir}:{path}"));
            }
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| CoreError::Terminal(e.to_string()))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| CoreError::Terminal(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| CoreError::Terminal(e.to_string()))?;

        {
            let mut terminals = self
                .terminals
                .lock()
                .map_err(|e| CoreError::Terminal(e.to_string()))?;
            terminals.insert(
                id.clone(),
                TerminalInstance {
                    writer,
                    master: pair.master,
                    child,
                },
            );
        }

        // Background reader thread
        let events = self.events.clone();
        let event_id = id.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        events.emit(Event::TerminalData {
                            id: event_id.clone(),
                            data,
                        });
                    }
                    Err(_) => break,
                }
            }
            tracing::debug!(id = %event_id, "Terminal reader loop ended");
            events.emit(Event::TerminalExit { id: event_id });
        });

        Ok(())
    }

    #[tracing::instrument(skip(self, data))]
    pub fn write(&self, id: &str, data: &str) -> Result<(), CoreError> {
        let mut terminals = self
            .terminals
            .lock()
            .map_err(|e| CoreError::Terminal(e.to_string()))?;
        let terminal = terminals
            .get_mut(id)
            .ok_or_else(|| CoreError::NotFound(format!("Terminal {id}")))?;
        terminal.writer.write_all(data.as_bytes())?;
        terminal.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), CoreError> {
        let terminals = self
            .terminals
            .lock()
            .map_err(|e| CoreError::Terminal(e.to_string()))?;
        let terminal = terminals
            .get(id)
            .ok_or_else(|| CoreError::NotFound(format!("Terminal {id}")))?;
        terminal
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| CoreError::Terminal(e.to_string()))?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn close(&self, id: &str) -> Result<(), CoreError> {
        let mut terminals = self
            .terminals
            .lock()
            .map_err(|e| CoreError::Terminal(e.to_string()))?;
        if let Some(mut terminal) = terminals.remove(id) {
            let _ = terminal.child.kill();
        }
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn default_login_shell() -> Option<String> {
    // /etc/passwd is the source of truth for the login shell. $SHELL is unreliable
    // because a parent process (terminal emulator, IDE, launcher) may override it.
    let user = std::env::var("USER").ok()?;
    let prefix = format!("{user}:");
    if let Ok(content) = std::fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix(&prefix) {
                if let Some(shell) = rest.rsplit(':').next() {
                    let shell = shell.trim();
                    if !shell.is_empty() {
                        return Some(shell.to_string());
                    }
                }
            }
        }
    }
    std::env::var("SHELL").ok()
}

pub fn list_shells() -> Vec<ShellInfo> {
    let mut shells = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // PowerShell 7+
        if which::which("pwsh").is_ok() {
            shells.push(ShellInfo {
                name: "PowerShell".into(),
                program: "pwsh.exe".into(),
                args: vec![],
            });
        }

        // Windows PowerShell 5.1
        shells.push(ShellInfo {
            name: "Windows PowerShell".into(),
            program: "powershell.exe".into(),
            args: vec![],
        });

        // Command Prompt
        shells.push(ShellInfo {
            name: "Command Prompt".into(),
            program: "cmd.exe".into(),
            args: vec![],
        });

        // Git Bash
        let git_bash_paths = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ];
        for path in &git_bash_paths {
            if std::path::Path::new(path).exists() {
                shells.push(ShellInfo {
                    name: "Git Bash".into(),
                    program: path.to_string(),
                    args: vec!["--login".into()],
                });
                break;
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut seen_names = std::collections::HashSet::new();
        if let Ok(content) = std::fs::read_to_string("/etc/shells") {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let name = std::path::Path::new(line)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| line.to_string());
                // Skip non-interactive or minimal shells
                if matches!(name.as_str(), "sh" | "dash" | "rbash" | "git-shell" | "nologin" | "false")
                    || name.contains("fallback")
                {
                    continue;
                }
                if !seen_names.insert(name.clone()) {
                    continue;
                }
                shells.push(ShellInfo {
                    name,
                    program: line.to_string(),
                    args: vec!["--login".into()],
                });
            }
        }

        if shells.is_empty() {
            for (name, path) in [("bash", "/bin/bash"), ("sh", "/bin/sh")] {
                if std::path::Path::new(path).exists() {
                    shells.push(ShellInfo {
                        name: name.to_string(),
                        program: path.to_string(),
                        args: vec![],
                    });
                }
            }
        }

        if let Some(default_shell) = default_login_shell() {
            if let Some(pos) = shells.iter().position(|s| s.program == default_shell) {
                if pos != 0 {
                    let default = shells.remove(pos);
                    shells.insert(0, default);
                }
            }
        }
    }

    shells
}

/// Deploy lightweight clipboard shim scripts so TUI apps inside local Linux
/// terminals can read image data written by `terminal_forward_clipboard_image`.
///
/// Returns the shim directory path (prepended to PATH for spawned terminals).
/// The shims serve the kosmos clipboard image for image reads and find the
/// real tool dynamically in PATH (skipping the shim directory) for everything
/// else.
#[cfg(target_os = "linux")]
fn ensure_clipboard_shims() -> String {
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let dir = format!("{base}/kosmos/clipboard-shims");
    let _ = std::fs::create_dir_all(&dir);

    // Helper fragment: resolve the real binary by removing our shim dir from PATH.
    // Shared by both shims — keeps them from recursing into themselves.
    const FIND_REAL: &str = r#"SHIM_DIR=$(dirname "$(readlink -f "$0")")
REAL=$(PATH=$(printf '%s' "$PATH" | tr ':' '\n' | grep -Fxv "$SHIM_DIR" | tr '\n' ':')"#;

    let xclip_shim = format!(
        r#"#!/bin/sh
KOSMOS_IMG="${{XDG_RUNTIME_DIR:-/tmp}}/kosmos/clipboard.png"
reading=false; image=false; targets=false
for arg in "$@"; do case "$arg" in -o) reading=true ;; image/*) image=true ;; TARGETS) targets=true ;; esac; done
if $reading && $targets && [ -f "$KOSMOS_IMG" ]; then printf 'image/png\n'; exit 0; fi
if $reading && $image && [ -f "$KOSMOS_IMG" ]; then cat "$KOSMOS_IMG"; exit 0; fi
{FIND_REAL} command -v xclip 2>/dev/null)
[ -x "$REAL" ] && exec "$REAL" "$@"
"#
    );

    let wl_paste_shim = format!(
        r#"#!/bin/sh
KOSMOS_IMG="${{XDG_RUNTIME_DIR:-/tmp}}/kosmos/clipboard.png"
listing=false; image=false
for arg in "$@"; do case "$arg" in -l|--list-types) listing=true ;; image/*|--type=image/*) image=true ;; esac; done
if $listing && [ -f "$KOSMOS_IMG" ]; then printf 'image/png\n'; exit 0; fi
if $image && [ -f "$KOSMOS_IMG" ]; then cat "$KOSMOS_IMG"; exit 0; fi
{FIND_REAL} command -v wl-paste 2>/dev/null)
[ -x "$REAL" ] && exec "$REAL" "$@"
"#
    );

    let _ = std::fs::write(format!("{dir}/xclip"), xclip_shim);
    let _ = std::fs::write(format!("{dir}/wl-paste"), wl_paste_shim);

    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o755);
    let _ = std::fs::set_permissions(format!("{dir}/xclip"), perms.clone());
    let _ = std::fs::set_permissions(format!("{dir}/wl-paste"), perms);

    dir
}

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use kosmos_core::configure_child_process;

/// Dev builds use a separate directory so pkill/overwrite doesn't touch prod.
pub const REMOTE_DIR: &str = if cfg!(debug_assertions) {
    ".kosmos-agent-dev"
} else {
    ".kosmos-agent"
};

/// Locate the bundled agent binary, falling back to the dev path under src-tauri/resources.
fn bundled_agent_path(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource dir: {e}"))?;

    let path = resource_dir.join("resources").join("kosmos-agent");
    if path.exists() {
        return Ok(path);
    }

    // Dev: resource_dir is target/debug; resources live at src-tauri/resources.
    let dev_path = resource_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("resources").join("kosmos-agent"));

    if let Some(p) = dev_path {
        if p.exists() {
            return Ok(p);
        }
    }

    Err("Agent binary not found. Run: cargo build -p kosmos-agent --target x86_64-unknown-linux-musl, then copy to src-tauri/resources/".into())
}

/// Check the version of the installed agent in a WSL distro.
pub async fn check_remote_version(distro: &str) -> Option<String> {
    let bin = format!("~/{REMOTE_DIR}/kosmos-agent");
    let output = run_wsl(distro, &[&bin, "--version"])
        .await
        .ok()?;
    let trimmed = output.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Ensure a WSL distro is running. Starts it if stopped.
pub async fn ensure_wsl_running(distro: &str) -> Result<(), String> {
    // `wsl -d <distro> -- true` starts a stopped distro as a side effect.
    run_wsl(distro, &["true"]).await?;
    Ok(())
}

async fn binaries_match(distro: &str, local_wsl_path: &str, remote_bin: &str) -> bool {
    let local_args = ["sha256sum", local_wsl_path];
    let remote_args = ["sha256sum", remote_bin];
    let (local_result, remote_result) = tokio::join!(
        run_wsl(distro, &local_args),
        run_wsl(distro, &remote_args),
    );

    let extract_hash = |r: Result<String, String>| -> Option<String> {
        r.ok()
            .and_then(|s| s.split_whitespace().next().map(|h| h.to_string()))
    };

    match (extract_hash(local_result), extract_hash(remote_result)) {
        (Some(local), Some(remote)) => local == remote,
        _ => false,
    }
}

/// Copy the bundled agent into a WSL distro; skip the copy if SHA256 matches.
pub async fn deploy_to_wsl(app: &AppHandle, distro: &str) -> Result<(), String> {
    let dir = format!("~/{REMOTE_DIR}");
    let bin = format!("~/{REMOTE_DIR}/kosmos-agent");

    run_wsl(distro, &["mkdir", "-p", &dir]).await?;

    let agent_src = bundled_agent_path(app)?;
    let wsl_path = windows_to_wsl_path(&agent_src.to_string_lossy());

    if binaries_match(distro, &wsl_path, &bin).await {
        // Shims are always redeployed; the write is idempotent.
        deploy_clipboard_shims(distro).await;
        return Ok(());
    }

    // Match on the bare dir (not ~/REMOTE_DIR) so pkill works whether tilde was expanded.
    let kill_pattern = format!("{REMOTE_DIR}/kosmos-agent");
    let _ = run_wsl(distro, &["pkill", "-f", &kill_pattern]).await;
    // Drop the daemon socket so ensure_daemon() spawns a fresh instance.
    let sock = format!("~/{REMOTE_DIR}/agent.sock");
    let _ = run_wsl(distro, &["rm", "-f", &sock]).await;
    run_wsl(distro, &["cp", &wsl_path, &bin]).await?;
    run_wsl(distro, &["chmod", "+x", &bin]).await?;

    deploy_clipboard_shims(distro).await;

    Ok(())
}

/// Install xclip/wl-paste shims in ~/.local/bin so WSL TUIs can read host
/// clipboard images from /tmp/kosmos-clipboard.png, falling back to PowerShell
/// for text. Must run before the agent so .profile adds ~/.local/bin to PATH.
async fn deploy_clipboard_shims(distro: &str) {
    let home = match wsl_resolve_home(distro).await {
        Ok(h) => h,
        Err(_) => return,
    };

    let bin_dir = format!(r"\\wsl.localhost\{distro}{home}/.local/bin");
    if std::fs::create_dir_all(&bin_dir).is_err() {
        return;
    }

    let xclip_shim = r#"#!/bin/sh
KOSMOS_IMG=/tmp/kosmos-clipboard.png
REAL=/usr/bin/xclip
reading=false; image=false; targets=false
for arg in "$@"; do case "$arg" in -o) reading=true ;; image/*) image=true ;; TARGETS) targets=true ;; esac; done
if $reading && $targets && [ -f "$KOSMOS_IMG" ]; then printf 'image/png\n'; exit 0; fi
if $reading && $image && [ -f "$KOSMOS_IMG" ]; then cat "$KOSMOS_IMG"; exit 0; fi
[ -x "$REAL" ] && exec "$REAL" "$@"
if $reading; then powershell.exe -NoProfile -Command 'Get-Clipboard' 2>/dev/null | tr -d '\r'
else clip.exe 2>/dev/null; fi
"#;

    let wl_paste_shim = r#"#!/bin/sh
KOSMOS_IMG=/tmp/kosmos-clipboard.png
REAL=/usr/bin/wl-paste
listing=false; image=false
for arg in "$@"; do case "$arg" in -l|--list-types) listing=true ;; image/*|--type=image/*) image=true ;; esac; done
if $listing && [ -f "$KOSMOS_IMG" ]; then printf 'image/png\n'; exit 0; fi
if $image && [ -f "$KOSMOS_IMG" ]; then cat "$KOSMOS_IMG"; exit 0; fi
[ -x "$REAL" ] && exec "$REAL" "$@"
powershell.exe -NoProfile -Command 'Get-Clipboard' 2>/dev/null | tr -d '\r'
"#;

    // Strip CR: Windows source edits may be CRLF, but shell scripts need LF-only.
    let _ = std::fs::write(format!(r"{bin_dir}\xclip"), xclip_shim.replace('\r', ""));
    let _ = std::fs::write(format!(r"{bin_dir}\wl-paste"), wl_paste_shim.replace('\r', ""));
    let _ = run_wsl(distro, &["chmod", "+x",
        &format!("{home}/.local/bin/xclip"),
        &format!("{home}/.local/bin/wl-paste"),
    ]).await;
}

/// Copy the bundled agent to an SSH host via scp.
pub async fn deploy_to_ssh(
    app: &tauri::AppHandle,
    host: &str,
    user: Option<&str>,
) -> Result<(), String> {
    let target = match user {
        Some(u) => format!("{u}@{host}"),
        None => host.to_string(),
    };

    let dir = format!("~/{REMOTE_DIR}");
    let bin = format!("~/{REMOTE_DIR}/kosmos-agent");

    let output = tokio::process::Command::new("ssh")
        .args([&target, "mkdir", "-p", &dir])
        .output()
        .await
        .map_err(|e| format!("SSH failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to create agent directory: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let agent_src = bundled_agent_path(app)?;
    let scp_dest = format!("{target}:{bin}");

    let output = tokio::process::Command::new("scp")
        .args([&agent_src.to_string_lossy().to_string(), &scp_dest])
        .output()
        .await
        .map_err(|e| format!("SCP failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to copy agent binary: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = tokio::process::Command::new("ssh")
        .args([&target, "chmod", "+x", &bin])
        .output()
        .await
        .map_err(|e| format!("SSH chmod failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to make agent executable: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Convert a Windows path to a WSL-accessible /mnt/ path.
fn windows_to_wsl_path(win_path: &str) -> String {
    // Strip the \\?\ extended-length prefix Windows APIs sometimes add.
    let clean = win_path.strip_prefix(r"\\?\").unwrap_or(win_path);
    let normalized = clean.replace('\\', "/");
    if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        let drive = (normalized.as_bytes()[0] as char).to_ascii_lowercase();
        format!("/mnt/{}/{}", drive, &normalized[3..])
    } else {
        normalized
    }
}

/// Run a command inside a WSL distro and return stdout.
async fn run_wsl(distro: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("wsl.exe");
    cmd.args(["-d", distro, "--"]);
    cmd.args(args);
    configure_child_process(&mut cmd);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("WSL exec failed: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Resolve the home directory inside a WSL distro.
pub async fn wsl_resolve_home(distro: &str) -> Result<String, String> {
    let output = run_wsl(distro, &["sh", "-c", "echo $HOME"]).await?;
    let home = output.trim().to_string();
    if home.is_empty() {
        Ok("/root".to_string())
    } else {
        Ok(home)
    }
}

/// List directories inside a WSL distro path.
pub async fn wsl_list_dir(distro: &str, path: &str) -> Result<Vec<(String, bool)>, String> {
    let output = run_wsl(distro, &["ls", "-1ApL", path]).await?;
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.ends_with('/') {
            let name = line.trim_end_matches('/').to_string();
            if !name.is_empty() {
                dirs.push(name);
            }
        } else {
            files.push(line.to_string());
        }
    }

    dirs.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    files.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    let mut result: Vec<(String, bool)> = dirs.into_iter().map(|n| (n, true)).collect();
    result.extend(files.into_iter().map(|n| (n, false)));
    Ok(result)
}

/// List available WSL distributions.
pub async fn list_wsl_distros() -> Result<Vec<String>, String> {
    #[cfg(not(target_os = "windows"))]
    return Ok(vec![]);

    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("wsl.exe");
        cmd.args(["--list", "--quiet"]);
        configure_child_process(&mut cmd);

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("WSL list failed: {e}"))?;

        if !output.status.success() {
            return Ok(vec![]);
        }

        let u16s: Vec<u16> = output
            .stdout
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&u16s);

        let distros: Vec<String> = text
            .lines()
            .map(|l| l.trim().trim_matches('\0').to_string())
            .filter(|l| !l.is_empty())
            .collect();

        Ok(distros)
    }
}

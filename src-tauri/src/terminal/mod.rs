use kosmos_core::terminal::TerminalManager;
use kosmos_protocol::requests::Request;
use kosmos_protocol::types::ShellInfo;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::remote::connection::ConnectionType;
use crate::remote::router::BackendRouter;

#[tauri::command]
pub async fn terminal_list_shells(
    router: State<'_, BackendRouter>,
    workspace_path: Option<String>,
) -> Result<Vec<ShellInfo>, String> {
    if let Some(ref path) = workspace_path {
        if let Some((agent, _)) = router.resolve(path).await {
            let val = agent.request(Request::TerminalListShells).await?;
            return serde_json::from_value(val).map_err(|e| e.to_string());
        }
    }
    Ok(kosmos_core::terminal::list_shells())
}

#[tauri::command]
pub async fn terminal_spawn(
    router: State<'_, BackendRouter>,
    state: State<'_, TerminalManager>,
    id: String,
    program: String,
    args: Vec<String>,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    if let Some((agent, remote_cwd)) = router.resolve(&cwd).await {
        // Register before spawn so an immediate terminal_write finds the route.
        router
            .register_remote_terminal(id.clone(), agent.clone())
            .await;
        if let Err(e) = agent
            .request(Request::TerminalSpawn {
                id: id.clone(),
                program,
                args,
                cwd: remote_cwd,
                cols,
                rows,
            })
            .await
        {
            router.remove_remote_terminal(&id).await;
            return Err(e);
        }
        Ok(())
    } else if BackendRouter::is_remote_path(&cwd) {
        Err(format!("Remote agent not connected for path: {cwd}"))
    } else {
        state.spawn(id, &program, &args, &cwd, cols, rows).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn terminal_write(
    router: State<'_, BackendRouter>,
    state: State<'_, TerminalManager>,
    id: String,
    data: String,
) -> Result<(), String> {
    if let Some(agent) = router.get_remote_terminal(&id).await {
        agent
            .request(Request::TerminalWrite {
                id,
                data,
            })
            .await?;
        Ok(())
    } else {
        state.write(&id, &data).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn terminal_resize(
    router: State<'_, BackendRouter>,
    state: State<'_, TerminalManager>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    if let Some(agent) = router.get_remote_terminal(&id).await {
        agent
            .request(Request::TerminalResize {
                id,
                cols,
                rows,
            })
            .await?;
        Ok(())
    } else {
        state.resize(&id, cols, rows).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn terminal_close(
    router: State<'_, BackendRouter>,
    state: State<'_, TerminalManager>,
    id: String,
) -> Result<(), String> {
    if let Some(agent) = router.get_remote_terminal(&id).await {
        agent
            .request(Request::TerminalClose { id: id.clone() })
            .await?;
        router.remove_remote_terminal(&id).await;
        Ok(())
    } else {
        state.close(&id).map_err(|e| e.to_string())
    }
}

/// Forward the host clipboard image so TUI apps inside the terminal can
/// read it.
///
/// - **WSL terminals**: writes the PNG to the WSL filesystem via
///   `\\wsl.localhost\…\tmp\kosmos-clipboard.png`. The deployed
///   xclip/wl-paste shims serve this file transparently.
/// - **Local Linux terminals**: reads the image via system clipboard tools
///   (from the Tauri process, which has display access) and writes it to
///   `$XDG_RUNTIME_DIR/kosmos/clipboard.png`. Shims deployed at terminal
///   spawn serve this file to TUI apps that call xclip/wl-paste.
/// - **Local Windows terminals**: no-op — TUI apps read the Win32 clipboard
///   directly.
#[tauri::command]
pub async fn terminal_forward_clipboard_image(
    app: AppHandle,
    router: State<'_, BackendRouter>,
    id: String,
) -> Result<(), String> {
    if let Some(agent) = router.get_remote_terminal(&id).await {
        let distro = match &agent.connection_type {
            ConnectionType::Wsl { distro } => distro.clone(),
            _ => return Ok(()),
        };

        let image = app
            .clipboard()
            .read_image()
            .map_err(|e| format!("clipboard read_image: {e}"))?;

        let rgba = image.rgba();
        let png_bytes = encode_rgba_to_png(&rgba, image.width(), image.height())?;
        let wsl_host_path = format!(r"\\wsl.localhost\{distro}\tmp\kosmos-clipboard.png");
        std::fs::write(&wsl_host_path, &png_bytes)
            .map_err(|e| format!("write to WSL fs: {e}"))?;

        return Ok(());
    }

    // Local Linux: write the host clipboard image where shims will find it.
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let img_dir = format!("{base}/kosmos");
        let _ = std::fs::create_dir_all(&img_dir);
        let png_bytes = read_clipboard_image_linux(&app)?;
        std::fs::write(format!("{img_dir}/clipboard.png"), &png_bytes)
            .map_err(|e| format!("write clipboard image: {e}"))?;
    }

    Ok(())
}

/// Read a clipboard image on Linux, trying Tauri's API first then falling
/// back to system tools (wl-paste, xclip) which work from the Tauri
/// process since it has display access.
#[cfg(target_os = "linux")]
fn read_clipboard_image_linux(app: &AppHandle) -> Result<Vec<u8>, String> {
    if let Ok(image) = app.clipboard().read_image() {
        let rgba = image.rgba();
        if !rgba.is_empty() {
            return encode_rgba_to_png(&rgba, image.width(), image.height());
        }
    }

    // Fall back to wl-paste/xclip — these run from Tauri, which owns the clipboard.
    use std::process::Command;

    if let Ok(output) = Command::new("wl-paste").args(["--type", "image/png"]).output() {
        if output.status.success() && !output.stdout.is_empty() {
            return Ok(output.stdout);
        }
    }

    if let Ok(output) = Command::new("xclip")
        .args(["-selection", "clipboard", "-o", "-t", "image/png"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return Ok(output.stdout);
        }
    }

    Err("No image found in clipboard".to_string())
}

fn encode_rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("png header: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("png data: {e}"))?;
    }
    Ok(buf)
}

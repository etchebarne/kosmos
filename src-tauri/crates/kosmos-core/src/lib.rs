pub mod editor;
pub mod error;
pub mod fff_picker;
pub mod file_tree;
pub mod fuzzy;
pub mod git;
pub mod git_stash;
pub mod lsp;
pub mod search;
pub mod terminal;
pub mod watcher;

use std::path::Path;

pub use error::CoreError;
pub use kosmos_protocol;

/// Validate that a path doesn't contain traversal components (`..`).
/// This prevents escaping workspace boundaries on the remote agent.
pub fn validate_no_traversal(path: &str) -> Result<(), CoreError> {
    for component in Path::new(path).components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(CoreError::PathTraversal(path.to_string()));
        }
    }
    Ok(())
}

/// Windows process creation flag to suppress console windows for background processes.
#[cfg(target_os = "windows")]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Returns true when running inside an AppImage on Linux.
/// AppImage injects `LD_LIBRARY_PATH` pointing to bundled libraries which causes
/// conflicts (e.g. OpenSSL version mismatches) in child processes like shells,
/// LSP servers, and git. WebKit subprocesses still need it, so we only strip it
/// from child processes we spawn, not from the global process environment.
pub fn is_appimage() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some()
}

/// Strip AppImage-injected environment variables from a `tokio::process::Command`
/// so child processes use system libraries instead of bundled ones.
#[cfg(target_os = "linux")]
pub fn sanitize_child_env(cmd: &mut tokio::process::Command) {
    if is_appimage() {
        cmd.env_remove("LD_LIBRARY_PATH");
    }
}

/// Apply platform-specific child process hardening:
/// - Linux: strip AppImage-injected env vars so child uses system libraries.
/// - Windows: suppress the console window that would otherwise flash on spawn.
///
/// No-op on macOS. Use this for every background child we spawn (`git`, LSP
/// servers, shells, plugin processes, WSL/SSH tunnels, etc.) so the details
/// of each platform live in one place.
#[allow(unused_variables)]
pub fn configure_child_process(cmd: &mut tokio::process::Command) {
    #[cfg(target_os = "linux")]
    sanitize_child_env(cmd);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Trait for delivering events from core modules to the host or agent.
/// The Tauri host implements this to emit Tauri events.
/// The remote agent implements this to write JSON-RPC notifications to stdout.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: kosmos_protocol::events::Event);
}

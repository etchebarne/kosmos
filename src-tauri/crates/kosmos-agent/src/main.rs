mod dispatch;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod client;
#[cfg(not(unix))]
mod inline;

use std::path::{Path, PathBuf};

pub(crate) struct AgentState {
    watcher: kosmos_core::watcher::WatcherManager,
    terminals: kosmos_core::terminal::TerminalManager,
    lsp: kosmos_core::lsp::LspManager,
    fff: kosmos_core::fff_picker::FffPicker,
}

// ── Helpers ──

pub(crate) fn agent_data_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".kosmos-agent")
}

pub(crate) fn ensure_node_runtime(data_dir: &Path) {
    if which::which("node").is_ok() {
        return;
    }

    let alt = which::which("bun");
    if let Ok(runtime_path) = alt {
        let bin_dir = data_dir.join("bin");
        std::fs::create_dir_all(&bin_dir).ok();
        let node_shim = bin_dir.join("node");
        if !node_shim.exists() {
            #[cfg(unix)]
            {
                let _: Result<(), _> = std::os::unix::fs::symlink(&runtime_path, &node_shim);
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::copy(&runtime_path, &node_shim);
            }
        }
        if let Ok(path) = std::env::var("PATH") {
            std::env::set_var("PATH", format!("{}:{}", bin_dir.display(), path));
        }
    }
}

pub(crate) fn to_json(val: impl serde::Serialize) -> Result<serde_json::Value, String> {
    serde_json::to_value(val).map_err(|e| format!("Serialization error: {e}"))
}

// ── Entry point ──

#[tokio::main]
async fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("kosmos_agent=info,kosmos_core=info,warn")
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    #[cfg(unix)]
    {
        if std::env::args().any(|a| a == "--daemon") {
            daemon::daemon_main().await;
        } else {
            client::client_main().await;
        }
    }

    #[cfg(not(unix))]
    {
        inline::inline_main().await;
    }
}

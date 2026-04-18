use super::connection::ConnectionType;
use super::deploy;
use super::router::BackendRouter;
use tauri::State;

#[tauri::command]
pub async fn list_wsl_distros() -> Result<Vec<String>, String> {
    deploy::list_wsl_distros().await
}

#[tauri::command]
pub async fn deploy_agent_wsl(
    app: tauri::AppHandle,
    distro: String,
) -> Result<(), String> {
    deploy::deploy_to_wsl(&app, &distro).await
}

#[tauri::command]
pub async fn check_agent_version(distro: String) -> Result<Option<String>, String> {
    Ok(deploy::check_remote_version(&distro).await)
}

#[derive(serde::Serialize)]
pub struct WslDirEntry {
    name: String,
    is_dir: bool,
}

#[tauri::command]
pub async fn wsl_resolve_home(distro: String) -> Result<String, String> {
    deploy::wsl_resolve_home(&distro).await
}

#[tauri::command]
pub async fn wsl_list_dir(distro: String, path: String) -> Result<Vec<WslDirEntry>, String> {
    let entries = deploy::wsl_list_dir(&distro, &path).await?;
    Ok(entries
        .into_iter()
        .map(|(name, is_dir)| WslDirEntry { name, is_dir })
        .collect())
}

/// Connect a workspace to a remote backend (WSL or SSH).
/// Spawns a kosmos-agent process in the remote environment.
#[tauri::command]
pub async fn remote_connect(
    router: State<'_, BackendRouter>,
    workspace_path: String,
    connection: ConnectionType,
) -> Result<(), String> {
    router.connect(&workspace_path, connection).await
}

/// Disconnect a workspace from its remote backend.
/// Also cleans up any LSP server mappings for the workspace.
#[tauri::command]
pub async fn remote_disconnect(
    router: State<'_, BackendRouter>,
    remote_servers: State<'_, crate::lsp::RemoteServerMap>,
    workspace_path: String,
) -> Result<(), String> {
    remote_servers.retain_workspace(&workspace_path).await;
    router.disconnect(&workspace_path).await;
    Ok(())
}

/// Check if a workspace has an active remote connection.
#[tauri::command]
pub async fn remote_is_connected(
    router: State<'_, BackendRouter>,
    workspace_path: String,
) -> Result<bool, String> {
    Ok(router.is_remote(&workspace_path).await)
}

/// Ensure a remote workspace is connected. Handles the full flow:
/// - For WSL: ensures distro is running, deploys agent if needed, connects
/// - Skips if already connected
/// Returns Ok(true) if connected, Ok(false) if skipped (local workspace).
#[tauri::command]
pub async fn remote_ensure_connected(
    app: tauri::AppHandle,
    router: State<'_, BackendRouter>,
    workspace_path: String,
    connection: ConnectionType,
) -> Result<bool, String> {
    if router.is_remote(&workspace_path).await {
        return Ok(true);
    }

    match &connection {
        ConnectionType::Local => Ok(false),
        ConnectionType::Wsl { distro } => {
            deploy::ensure_wsl_running(distro).await?;
            deploy::deploy_to_wsl(&app, distro).await?;
            router
                .connect(&workspace_path, connection.clone())
                .await?;

            Ok(true)
        }
        ConnectionType::Ssh { host, user } => {
            deploy::deploy_to_ssh(&app, host, user.as_deref()).await?;
            router
                .connect(&workspace_path, connection.clone())
                .await?;
            Ok(true)
        }
    }
}

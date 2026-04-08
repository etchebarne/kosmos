use kosmos_core::search::ContentMatch;
use kosmos_protocol::requests::Request;
use tauri::State;

use crate::remote::router::BackendRouter;

fn no_agent_error(path: &str) -> String {
    format!("Remote agent not connected for path: {path}")
}

/// Walk the workspace and return all file paths (respects .gitignore).
#[tauri::command]
pub async fn list_workspace_files(
    router: State<'_, BackendRouter>,
    path: String,
) -> Result<Vec<String>, String> {
    if let Some((agent, remote_path)) = router.resolve(&path).await {
        let val = agent
            .request(Request::ListWorkspaceFiles {
                path: remote_path,
            })
            .await?;
        serde_json::from_value(val).map_err(|e| e.to_string())
    } else if BackendRouter::is_remote_path(&path) {
        Err(no_agent_error(&path))
    } else {
        let root = path.clone();
        tokio::task::spawn_blocking(move || kosmos_core::search::list_workspace_files(&root))
            .await
            .map_err(|e| e.to_string())?
    }
}

/// Search file contents for a query string (case-insensitive).
#[tauri::command]
pub async fn search_in_files(
    router: State<'_, BackendRouter>,
    path: String,
    query: String,
    max_results: Option<usize>,
) -> Result<Vec<ContentMatch>, String> {
    if let Some((agent, remote_path)) = router.resolve(&path).await {
        let val = agent
            .request(Request::SearchInFiles {
                path: remote_path,
                query,
                max_results,
            })
            .await?;
        serde_json::from_value(val).map_err(|e| e.to_string())
    } else if BackendRouter::is_remote_path(&path) {
        Err(no_agent_error(&path))
    } else {
        let root = path.clone();
        tokio::task::spawn_blocking(move || {
            kosmos_core::search::search_in_files(&root, &query, max_results)
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

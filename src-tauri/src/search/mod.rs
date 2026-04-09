use kosmos_core::search::ContentMatch;
use kosmos_protocol::requests::Request;
use kosmos_protocol::ToStringErr;
use tauri::State;

use crate::remote::router::BackendRouter;
use crate::remote::routing::{resolve, Route};

/// Walk the workspace and return all file paths (respects .gitignore).
#[tauri::command]
pub async fn list_workspace_files(
    router: State<'_, BackendRouter>,
    path: String,
) -> Result<Vec<String>, String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let val = agent
                .request(Request::ListWorkspaceFiles { path: remote_path })
                .await?;
            serde_json::from_value(val).str_err()
        }
        Route::Local => {
            let root = path;
            tokio::task::spawn_blocking(move || kosmos_core::search::list_workspace_files(&root))
                .await
                .str_err()?
        }
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
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let val = agent
                .request(Request::SearchInFiles {
                    path: remote_path,
                    query,
                    max_results,
                })
                .await?;
            serde_json::from_value(val).str_err()
        }
        Route::Local => {
            let root = path;
            tokio::task::spawn_blocking(move || {
                kosmos_core::search::search_in_files(&root, &query, max_results)
            })
            .await
            .str_err()?
        }
    }
}

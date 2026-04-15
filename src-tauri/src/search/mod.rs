use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kosmos_core::search::{ContentMatch, FuzzyFileMatch};
use kosmos_protocol::requests::Request;
use kosmos_protocol::ToStringErr;
use tauri::State;

use crate::remote::router::BackendRouter;
use crate::remote::routing::{resolve, Route};

// ── File list cache ──

/// In-memory cache of workspace file lists, invalidated by the file-system
/// watcher so that `fuzzy_search_files` doesn't re-walk the directory tree
/// on every keystroke.
pub struct FileListCache {
    inner: Mutex<HashMap<String, Arc<Vec<String>>>>,
}

impl FileListCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, path: &str) -> Option<Arc<Vec<String>>> {
        self.inner.lock().unwrap().get(path).cloned()
    }

    fn set(&self, path: String, files: Vec<String>) {
        self.inner
            .lock()
            .unwrap()
            .insert(path, Arc::new(files));
    }

    /// Drop all cached file lists (called when the watcher detects changes).
    pub fn invalidate(&self) {
        self.inner.lock().unwrap().clear();
    }
}

// ── Commands ──

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

/// Fuzzy-search workspace files in Rust, returning scored results with match
/// indices. Local searches use a cached file list to avoid repeated walks.
#[tauri::command]
pub async fn fuzzy_search_files(
    router: State<'_, BackendRouter>,
    cache: State<'_, Arc<FileListCache>>,
    path: String,
    query: String,
    max_results: Option<usize>,
) -> Result<Vec<FuzzyFileMatch>, String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let val = agent
                .request(Request::FuzzySearchFiles {
                    path: remote_path,
                    query,
                    max_results,
                })
                .await?;
            serde_json::from_value(val).str_err()
        }
        Route::Local => {
            // Populate cache on first call (directory walk), then reuse.
            let files = if let Some(cached) = cache.get(&path) {
                cached
            } else {
                let root = path.clone();
                let files = tokio::task::spawn_blocking(move || {
                    kosmos_core::search::list_workspace_files(&root)
                })
                .await
                .str_err()??;
                cache.set(path.clone(), files.clone());
                cache.get(&path).unwrap()
            };

            tokio::task::spawn_blocking(move || {
                kosmos_core::search::fuzzy_match_files(&files, &query, max_results)
            })
            .await
            .str_err()
        }
    }
}

/// Search file contents for a query string (literal or regex).
#[tauri::command]
pub async fn search_in_files(
    router: State<'_, BackendRouter>,
    path: String,
    query: String,
    max_results: Option<usize>,
    use_regex: Option<bool>,
) -> Result<Vec<ContentMatch>, String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let val = agent
                .request(Request::SearchInFiles {
                    path: remote_path,
                    query,
                    max_results,
                    use_regex,
                })
                .await?;
            serde_json::from_value(val).str_err()
        }
        Route::Local => {
            let root = path;
            tokio::task::spawn_blocking(move || {
                kosmos_core::search::search_in_files(&root, &query, max_results, use_regex)
            })
            .await
            .str_err()?
        }
    }
}

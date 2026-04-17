use std::sync::{Mutex, OnceLock};

use kosmos_core::fff_picker::{FffHit, FffPicker};
use kosmos_core::fuzzy::{FuzzyHit, MatchMode};
use kosmos_core::search::ContentMatch;
use kosmos_protocol::ToStringErr;
use kosmos_protocol::requests::Request;
use tauri::{AppHandle, Manager, State};

use crate::remote::router::BackendRouter;
use crate::remote::routing::{Route, resolve};

/// Lazily-initialized global [`FffPicker`]. The underlying `FilePicker` is
/// re-created when the active workspace changes; the frecency database is
/// opened once and shared across workspaces.
pub struct FffPickerState {
    picker: OnceLock<FffPicker>,
    init_lock: Mutex<()>,
}

impl FffPickerState {
    pub fn new() -> Self {
        Self {
            picker: OnceLock::new(),
            init_lock: Mutex::new(()),
        }
    }

    fn get_or_init(&self, app: &AppHandle) -> Result<&FffPicker, String> {
        if let Some(p) = self.picker.get() {
            return Ok(p);
        }
        let _guard = self.init_lock.lock().unwrap();
        if let Some(p) = self.picker.get() {
            return Ok(p);
        }
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app_data_dir: {e}"))?;
        let db_path = data_dir.join("fff-frecency.lmdb");
        let picker = FffPicker::new(db_path)?;
        let _ = self.picker.set(picker);
        Ok(self.picker.get().unwrap())
    }
}

impl Default for FffPickerState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Commands ──

/// Initialize (or switch) the fff index to `path`. Idempotent for the same
/// path. Local workspaces only — remote agents manage their own picker.
#[tauri::command]
pub async fn fff_set_workspace(
    app: AppHandle,
    router: State<'_, BackendRouter>,
    state: State<'_, FffPickerState>,
    path: String,
) -> Result<(), String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            agent
                .request(Request::FffSetWorkspace { path: remote_path })
                .await?;
            Ok(())
        }
        Route::Local => {
            let picker = state.get_or_init(&app)?.clone();
            tokio::task::spawn_blocking(move || picker.set_workspace(std::path::Path::new(&path)))
                .await
                .str_err()?
        }
    }
}

/// Fuzzy-search workspace files. Returns score-sorted hits with highlight
/// indices (byte offsets into `relative_path`).
#[tauri::command]
pub async fn fff_search_files(
    app: AppHandle,
    router: State<'_, BackendRouter>,
    state: State<'_, FffPickerState>,
    path: String,
    query: String,
    max_results: Option<usize>,
) -> Result<Vec<FffHit>, String> {
    let limit = max_results.unwrap_or(50);
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let val = agent
                .request(Request::FffSearchFiles {
                    path: remote_path,
                    query,
                    max_results: Some(limit),
                })
                .await?;
            serde_json::from_value(val).str_err()
        }
        Route::Local => {
            let picker = state.get_or_init(&app)?.clone();
            tokio::task::spawn_blocking(move || picker.search(&query, limit))
                .await
                .str_err()?
        }
    }
}

/// Record an access in the frecency DB. Called by the frontend every time a
/// file is opened from the picker (or anywhere else worth boosting).
#[tauri::command]
pub async fn fff_track_access(
    app: AppHandle,
    router: State<'_, BackendRouter>,
    state: State<'_, FffPickerState>,
    path: String,
) -> Result<(), String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            agent
                .request(Request::FffTrackAccess { path: remote_path })
                .await?;
            Ok(())
        }
        Route::Local => {
            let picker = state.get_or_init(&app)?.clone();
            tokio::task::spawn_blocking(move || picker.track_access(std::path::Path::new(&path)))
                .await
                .str_err()?
        }
    }
}

/// Generic fuzzy match over a caller-supplied list of strings. Use this for
/// command palettes, branch pickers, any list that isn't a workspace file
/// index. Runs locally (no routing) — the caller already has the data.
#[tauri::command]
pub async fn fuzzy_match(
    query: String,
    items: Vec<String>,
    mode: Option<MatchMode>,
    limit: Option<usize>,
) -> Result<Vec<FuzzyHit>, String> {
    tokio::task::spawn_blocking(move || {
        kosmos_core::fuzzy::fuzzy_match(&query, &items, mode.unwrap_or_default(), limit)
    })
    .await
    .str_err()
}

/// Content search (literal or regex) powered by fff-search's grep engine.
/// Reuses the picker's indexed file list, mmap cache, and rayon parallelism.
#[tauri::command]
pub async fn search_in_files(
    app: AppHandle,
    router: State<'_, BackendRouter>,
    state: State<'_, FffPickerState>,
    path: String,
    query: String,
    max_results: Option<usize>,
    use_regex: Option<bool>,
) -> Result<Vec<ContentMatch>, String> {
    let limit = max_results.unwrap_or(100);
    let regex = use_regex.unwrap_or(false);
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let val = agent
                .request(Request::SearchInFiles {
                    path: remote_path,
                    query,
                    max_results: Some(limit),
                    use_regex: Some(regex),
                })
                .await?;
            serde_json::from_value(val).str_err()
        }
        Route::Local => {
            // Make sure fff is pointed at this workspace before querying. Usually
            // the frontend has already called `fff_set_workspace`, but if a
            // caller hits content search first we still want the right index.
            let picker = state.get_or_init(&app)?.clone();
            let workspace = path.clone();
            tokio::task::spawn_blocking(move || {
                picker.set_workspace(std::path::Path::new(&workspace))?;
                picker.grep(&query, limit, regex)
            })
            .await
            .str_err()?
        }
    }
}

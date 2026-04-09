use std::path::Path;

use kosmos_protocol::requests::Request;
use kosmos_protocol::types::DirEntry;
use kosmos_protocol::ToStringErr;
use tauri::AppHandle;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::remote::router::BackendRouter;
use crate::remote::routing::{resolve, Route};

/// Extract the `wsl://distro` prefix from a full remote path.
fn remote_prefix<'a>(full_path: &'a str, linux_path: &str) -> Result<&'a str, String> {
    let cutoff = full_path
        .len()
        .checked_sub(linux_path.len())
        .ok_or_else(|| format!("Invalid remote path prefix: full={full_path}, linux={linux_path}"))?;
    Ok(&full_path[..cutoff])
}

#[tauri::command]
pub async fn read_dir(
    router: State<'_, BackendRouter>,
    path: String,
) -> Result<Vec<DirEntry>, String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let val = agent
                .request(Request::ReadDir {
                    path: remote_path.clone(),
                })
                .await?;
            let mut entries: Vec<DirEntry> =
                serde_json::from_value(val).str_err()?;
            let prefix = remote_prefix(&path, &remote_path)?;
            for entry in &mut entries {
                entry.path = format!("{}{}", prefix, entry.path);
            }
            Ok(entries)
        }
        Route::Local => {
            tokio::task::spawn_blocking(move || kosmos_core::file_tree::read_dir(&path))
                .await
                .str_err()?
                .str_err()
        }
    }
}

#[tauri::command]
pub async fn move_file(
    router: State<'_, BackendRouter>,
    source: String,
    dest_dir: String,
) -> Result<String, String> {
    match resolve(&router, &source).await? {
        Route::Remote(agent, remote_source) => {
            let prefix = remote_prefix(&source, &remote_source)?.to_string();
            let remote_dest = router
                .resolve(&dest_dir)
                .await
                .map(|(_, p)| p)
                .unwrap_or(dest_dir);
            let val = agent
                .request(Request::MoveFile {
                    source: remote_source,
                    dest_dir: remote_dest,
                })
                .await?;
            let linux_path: String = serde_json::from_value(val).str_err()?;
            Ok(format!("{}{}", prefix, linux_path))
        }
        Route::Local => {
            kosmos_core::file_tree::move_file(&source, &dest_dir).str_err()
        }
    }
}

routed_cmd!(void fn create_file(path) {
    request(p) => Request::CreateFile { path: p },
    local => async { kosmos_core::file_tree::create_file(&path) },
});

routed_cmd!(void fn create_dir(path) {
    request(p) => Request::CreateDir { path: p },
    local => async { kosmos_core::file_tree::create_dir(&path) },
});

#[tauri::command]
pub async fn rename_entry(
    router: State<'_, BackendRouter>,
    path: String,
    new_name: String,
) -> Result<String, String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            let prefix = remote_prefix(&path, &remote_path)?.to_string();
            let val = agent
                .request(Request::RenameEntry {
                    path: remote_path,
                    new_name,
                })
                .await?;
            let linux_path: String = serde_json::from_value(val).str_err()?;
            Ok(format!("{}{}", prefix, linux_path))
        }
        Route::Local => {
            kosmos_core::file_tree::rename_entry(&path, &new_name).str_err()
        }
    }
}

#[tauri::command]
pub async fn copy_entry(
    router: State<'_, BackendRouter>,
    source: String,
    dest_dir: String,
) -> Result<String, String> {
    match resolve(&router, &source).await? {
        Route::Remote(agent, remote_source) => {
            let prefix = remote_prefix(&source, &remote_source)?.to_string();
            let remote_dest = router
                .resolve(&dest_dir)
                .await
                .map(|(_, p)| p)
                .unwrap_or(dest_dir);
            let val = agent
                .request(Request::CopyEntry {
                    source: remote_source,
                    dest_dir: remote_dest,
                })
                .await?;
            let linux_path: String = serde_json::from_value(val).str_err()?;
            Ok(format!("{}{}", prefix, linux_path))
        }
        Route::Local => {
            kosmos_core::file_tree::copy_entry(&source, &dest_dir).str_err()
        }
    }
}

routed_cmd!(void fn trash_entry(path) {
    request(p) => Request::TrashEntry { path: p },
    local => async { kosmos_core::file_tree::trash_entry(&path) },
});

routed_cmd!(void fn delete_entry(path) {
    request(p) => Request::DeleteEntry { path: p },
    local => async { kosmos_core::file_tree::delete_entry(&path) },
});

#[tauri::command]
pub fn reveal_in_explorer(app: AppHandle, path: &str) -> Result<(), String> {
    if BackendRouter::is_remote_path(path) {
        return Err("Cannot reveal remote files in the local file explorer".into());
    }
    app.opener()
        .reveal_item_in_dir(Path::new(path))
        .map_err(|e| e.to_string())
}

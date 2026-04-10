use std::sync::Arc;

use kosmos_core::watcher::WatcherManager;
use kosmos_protocol::requests::Request;
use kosmos_protocol::ToStringErr;
use tauri::State;

use crate::remote::router::BackendRouter;
use crate::remote::routing::{resolve, Route};

// ── Routed commands ──

routed_cmd!(val fn get_git_branch(path) -> Option<String> {
    request(p) => Request::GetGitBranch { path: p },
    local => kosmos_core::git::get_git_branch(&path),
});

routed_cmd!(val fn get_git_status(path) -> kosmos_protocol::types::GitStatusInfo {
    request(p) => Request::GetGitStatus { path: p },
    local => kosmos_core::git::get_git_status(&path),
});

routed_cmd!(val fn get_git_remote_owner(path) -> Option<String> {
    request(p) => Request::GetGitRemoteOwner { path: p },
    local => kosmos_core::git::get_git_remote_owner(&path),
});

routed_cmd!(void fn git_stage(path, files: Vec<String>) {
    request(p) => Request::GitStage { path: p, files },
    local => kosmos_core::git::git_stage(&path, files),
});

routed_cmd!(void fn git_unstage(path, files: Vec<String>) {
    request(p) => Request::GitUnstage { path: p, files },
    local => kosmos_core::git::git_unstage(&path, files),
});

routed_cmd!(void fn git_stage_all(path) {
    request(p) => Request::GitStageAll { path: p },
    local => kosmos_core::git::git_stage_all(&path),
});

routed_cmd!(void fn git_commit(path, message: String) {
    request(p) => Request::GitCommit { path: p, message },
    local => kosmos_core::git::git_commit(&path, &message),
});

routed_cmd!(val fn git_list_branches(path) -> Vec<kosmos_protocol::types::GitBranchInfo> {
    request(p) => Request::GitListBranches { path: p },
    local => kosmos_core::git::git_list_branches(&path),
});

routed_cmd!(void fn git_checkout(path, branch: String) {
    request(p) => Request::GitCheckout { path: p, branch },
    local => kosmos_core::git::git_checkout(&path, &branch),
});

routed_cmd!(void fn git_delete_branch(path, branch: String) {
    request(p) => Request::GitDeleteBranch { path: p, branch },
    local => kosmos_core::git::git_delete_branch(&path, &branch),
});

routed_cmd!(void fn git_discard(path, files: Vec<String>) {
    request(p) => Request::GitDiscard { path: p, files },
    local => kosmos_core::git::git_discard(&path, files),
});

routed_cmd!(void fn git_trash_untracked(path, files: Vec<String>) {
    request(p) => Request::GitTrashUntracked { path: p, files },
    local => async { kosmos_core::git::git_trash_untracked(&path, files) },
});

routed_cmd!(void fn git_stash_all(path) {
    request(p) => Request::GitStashAll { path: p },
    local => kosmos_core::git_stash::git_stash_all(&path),
});

routed_cmd!(void fn git_stash_files(path, files: Vec<String>) {
    request(p) => Request::GitStashFiles { path: p, files },
    local => kosmos_core::git_stash::git_stash_files(&path, files),
});

routed_cmd!(val fn git_stash_list(path) -> Vec<kosmos_protocol::types::GitStashEntry> {
    request(p) => Request::GitStashList { path: p },
    local => kosmos_core::git_stash::git_stash_list(&path),
});

routed_cmd!(val fn git_stash_show(path, index: usize) -> Vec<kosmos_protocol::types::GitStashFile> {
    request(p) => Request::GitStashShow { path: p, index },
    local => kosmos_core::git_stash::git_stash_show(&path, index),
});

routed_cmd!(void fn git_stash_pop(path, index: usize) {
    request(p) => Request::GitStashPop { path: p, index },
    local => kosmos_core::git_stash::git_stash_pop(&path, index),
});

routed_cmd!(void fn git_stash_drop(path, index: usize) {
    request(p) => Request::GitStashDrop { path: p, index },
    local => kosmos_core::git_stash::git_stash_drop(&path, index),
});

routed_cmd!(void fn git_discard_all_tracked(path) {
    request(p) => Request::GitDiscardAllTracked { path: p },
    local => kosmos_core::git::git_discard_all_tracked(&path),
});

routed_cmd!(void fn git_trash_all_untracked(path) {
    request(p) => Request::GitTrashAllUntracked { path: p },
    local => kosmos_core::git::git_trash_all_untracked(&path),
});

routed_cmd!(val fn git_diff(path, file: String, staged: bool) -> String {
    request(p) => Request::GitDiff { path: p, file, staged },
    local => kosmos_core::git::git_diff(&path, &file, staged),
});

routed_cmd!(val fn git_diff_untracked(path, file: String) -> String {
    request(p) => Request::GitDiffUntracked { path: p, file },
    local => kosmos_core::git::git_diff_untracked(&path, &file),
});

routed_cmd!(void fn git_init(path) {
    request(p) => Request::GitInit { path: p },
    local => kosmos_core::git::git_init(&path),
});

routed_cmd!(void fn git_fetch(path) {
    request(p) => Request::GitFetch { path: p },
    local => kosmos_core::git::git_fetch(&path),
});

routed_cmd!(void fn git_pull(path) {
    request(p) => Request::GitPull { path: p },
    local => kosmos_core::git::git_pull(&path),
});

routed_cmd!(void fn git_pull_rebase(path) {
    request(p) => Request::GitPullRebase { path: p },
    local => kosmos_core::git::git_pull_rebase(&path),
});

routed_cmd!(void fn git_push(path) {
    request(p) => Request::GitPush { path: p },
    local => kosmos_core::git::git_push(&path),
});

routed_cmd!(void fn git_force_push(path) {
    request(p) => Request::GitForcePush { path: p },
    local => kosmos_core::git::git_force_push(&path),
});

// ── Hand-written commands (custom routing logic) ──

#[tauri::command]
pub async fn watch_workspace(
    router: State<'_, BackendRouter>,
    watcher: State<'_, Arc<WatcherManager>>,
    path: String,
) -> Result<(), String> {
    match resolve(&router, &path).await? {
        Route::Remote(agent, remote_path) => {
            agent
                .request(Request::WatchWorkspace { path: remote_path })
                .await?;
            Ok(())
        }
        Route::Local => {
            // Setting up recursive inotify watches can block for seconds on large
            // repos. Fire-and-forget on the blocking pool so this command returns
            // instantly and doesn't hold up the IPC channel.
            let watcher = Arc::clone(&*watcher);
            tokio::task::spawn_blocking(move || {
                if let Err(e) = watcher.watch(&path) {
                    tracing::warn!("watch_workspace failed for {path}: {e}");
                }
            });
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn unwatch_workspace(
    router: State<'_, BackendRouter>,
    watcher: State<'_, Arc<WatcherManager>>,
    path: Option<String>,
) -> Result<(), String> {
    if let Some(ref p) = path {
        if let Some((agent, _)) = router.resolve(p).await {
            let _ = agent.request(Request::UnwatchWorkspace).await;
            return Ok(());
        }
    }
    watcher.unwatch().str_err()
}

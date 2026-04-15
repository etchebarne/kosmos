use std::sync::Arc;

use kosmos_protocol::requests::{Request, ResponseMessage};
use kosmos_protocol::ToStringErr;

use crate::{to_json, AgentState};

pub(crate) async fn dispatch(
    state: &AgentState,
    request: Request,
) -> Result<serde_json::Value, String> {
    match request {
        // ── File tree ──
        Request::ReadDir { path } => {
            let r = kosmos_core::file_tree::read_dir(&path).str_err()?;
            Ok(to_json(r)?)
        }
        Request::MoveFile { source, dest_dir } => {
            let r = kosmos_core::file_tree::move_file(&source, &dest_dir).str_err()?;
            Ok(to_json(r)?)
        }
        Request::CreateFile { path } => {
            kosmos_core::file_tree::create_file(&path).str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::CreateDir { path } => {
            kosmos_core::file_tree::create_dir(&path).str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::RenameEntry { path, new_name } => {
            let r = kosmos_core::file_tree::rename_entry(&path, &new_name).str_err()?;
            Ok(to_json(r)?)
        }
        Request::CopyEntry { source, dest_dir } => {
            let r = kosmos_core::file_tree::copy_entry(&source, &dest_dir).str_err()?;
            Ok(to_json(r)?)
        }
        Request::TrashEntry { path } => {
            kosmos_core::file_tree::trash_entry(&path).str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::DeleteEntry { path } => {
            kosmos_core::file_tree::delete_entry(&path).str_err()?;
            Ok(serde_json::Value::Null)
        }

        // ── Editor ──
        Request::ReadFile { path } => {
            let r = kosmos_core::editor::read_file(&path).await.str_err()?;
            Ok(to_json(r)?)
        }
        Request::WriteFile { path, content } => {
            kosmos_core::editor::write_file(&path, &content)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }

        // ── Git ──
        Request::GetGitBranch { path } => {
            let r = kosmos_core::git::get_git_branch(&path).await.str_err()?;
            Ok(to_json(r)?)
        }
        Request::GetGitRemoteOwner { path } => {
            let r = kosmos_core::git::get_git_remote_owner(&path)
                .await
                .str_err()?;
            Ok(to_json(r)?)
        }
        Request::GetGitStatus { path } => {
            let r = kosmos_core::git::get_git_status(&path).await.str_err()?;
            Ok(to_json(r)?)
        }
        Request::GitStage { path, files } => {
            kosmos_core::git::git_stage(&path, files).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitUnstage { path, files } => {
            kosmos_core::git::git_unstage(&path, files)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitStageAll { path } => {
            kosmos_core::git::git_stage_all(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitCommit { path, message } => {
            kosmos_core::git::git_commit(&path, &message)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitListBranches { path } => {
            let r = kosmos_core::git::git_list_branches(&path)
                .await
                .str_err()?;
            Ok(to_json(r)?)
        }
        Request::GitCheckout { path, branch } => {
            kosmos_core::git::git_checkout(&path, &branch)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitDeleteBranch { path, branch } => {
            kosmos_core::git::git_delete_branch(&path, &branch)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitDiscard { path, files } => {
            kosmos_core::git::git_discard(&path, files)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitTrashUntracked { path, files } => {
            kosmos_core::git::git_trash_untracked(&path, files).str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitStashAll { path } => {
            kosmos_core::git_stash::git_stash_all(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitStashFiles { path, files } => {
            kosmos_core::git_stash::git_stash_files(&path, files)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitStashList { path } => {
            let r = kosmos_core::git_stash::git_stash_list(&path).await.str_err()?;
            Ok(to_json(r)?)
        }
        Request::GitStashShow { path, index } => {
            let r = kosmos_core::git_stash::git_stash_show(&path, index)
                .await
                .str_err()?;
            Ok(to_json(r)?)
        }
        Request::GitStashPop { path, index } => {
            kosmos_core::git_stash::git_stash_pop(&path, index)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitStashDrop { path, index } => {
            kosmos_core::git_stash::git_stash_drop(&path, index)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitDiscardAllTracked { path } => {
            kosmos_core::git::git_discard_all_tracked(&path)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitTrashAllUntracked { path } => {
            kosmos_core::git::git_trash_all_untracked(&path)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitBlameLine { path, file, line } => {
            let r = kosmos_core::git::git_blame_line(&path, &file, line)
                .await
                .str_err()?;
            Ok(to_json(r)?)
        }
        Request::GitDiff { path, file, staged } => {
            let r = kosmos_core::git::git_diff(&path, &file, staged).await.str_err()?;
            Ok(to_json(r)?)
        }
        Request::GitDiffUntracked { path, file } => {
            let r = kosmos_core::git::git_diff_untracked(&path, &file)
                .await
                .str_err()?;
            Ok(to_json(r)?)
        }
        Request::GitInit { path } => {
            kosmos_core::git::git_init(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitFetch { path } => {
            kosmos_core::git::git_fetch(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitPull { path } => {
            kosmos_core::git::git_pull(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitPullRebase { path } => {
            kosmos_core::git::git_pull_rebase(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitPush { path } => {
            kosmos_core::git::git_push(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::GitForcePush { path } => {
            kosmos_core::git::git_force_push(&path).await.str_err()?;
            Ok(serde_json::Value::Null)
        }

        // ── Search ──
        Request::ListWorkspaceFiles { path } => {
            let r = kosmos_core::search::list_workspace_files(&path)?;
            Ok(to_json(r)?)
        }
        Request::FuzzySearchFiles {
            path,
            query,
            max_results,
        } => {
            let r = kosmos_core::search::fuzzy_search_files(&path, &query, max_results)?;
            Ok(to_json(r)?)
        }
        Request::SearchInFiles {
            path,
            query,
            max_results,
            use_regex,
        } => {
            let r = kosmos_core::search::search_in_files(&path, &query, max_results, use_regex)?;
            Ok(to_json(r)?)
        }

        // ── Watcher ──
        Request::WatchWorkspace { path } => {
            state.watcher.watch(&path).str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::UnwatchWorkspace => {
            state.watcher.unwatch().str_err()?;
            Ok(serde_json::Value::Null)
        }

        // ── Terminal ──
        Request::TerminalListShells => {
            let r = kosmos_core::terminal::list_shells();
            Ok(to_json(r)?)
        }
        Request::TerminalList => {
            let ids = state.terminals.list();
            Ok(to_json(ids)?)
        }
        Request::TerminalSpawn {
            id,
            program,
            args,
            cwd,
            cols,
            rows,
        } => {
            state
                .terminals
                .spawn(id, &program, &args, &cwd, cols, rows)
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::TerminalWrite { id, data } => {
            state.terminals.write(&id, &data).str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::TerminalResize { id, cols, rows } => {
            state.terminals.resize(&id, cols, rows).str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::TerminalClose { id } => {
            state.terminals.close(&id).str_err()?;
            Ok(serde_json::Value::Null)
        }

        // ── LSP ──
        Request::LspStart {
            workspace_path,
            language_id,
        } => {
            let r = state
                .lsp
                .start(&workspace_path, &language_id)
                .await
                .str_err()?;
            Ok(to_json(r)?)
        }
        Request::LspSend { server_id, message } => {
            state.lsp.send(&server_id, &message).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::LspStop { server_id } => {
            state.lsp.stop(&server_id).await.str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::LspStopWorkspace { workspace_path } => {
            state
                .lsp
                .stop_workspace(&workspace_path)
                .await
                .str_err()?;
            Ok(serde_json::Value::Null)
        }
        Request::LspCheckAvailability { workspace_path } => {
            let r = state.lsp.check_availability(&workspace_path);
            Ok(to_json(r)?)
        }
        Request::LspScanProjects { workspace_path } => {
            let r = state.lsp.scan_projects(&workspace_path);
            Ok(to_json(r)?)
        }
        Request::LspResolveRoot {
            file_path,
            language_id,
            workspace_path,
        } => {
            let r = kosmos_core::lsp::LspManager::resolve_root(
                &file_path,
                &language_id,
                &workspace_path,
            );
            Ok(to_json(r)?)
        }
        Request::LspLanguageGroups => {
            let r = kosmos_core::lsp::LspManager::language_groups();
            Ok(to_json(r)?)
        }
        Request::LspInstalledList => {
            let r = state.lsp.installed_list();
            Ok(to_json(r)?)
        }
        Request::LspInstallServer { name } => {
            let r = state.lsp.install_server(&name).await.str_err()?;
            Ok(to_json(r)?)
        }
        Request::LspUninstallServer { name } => {
            state.lsp.uninstall_server(&name).str_err()?;
            Ok(serde_json::Value::Null)
        }

        // ── Keepalive ──
        Request::Ping => Ok(serde_json::Value::Null),
    }
}

pub(crate) async fn run_dispatch(
    state: Arc<AgentState>,
    id: u64,
    request: Request,
) -> ResponseMessage {
    let handle = tokio::runtime::Handle::current();
    match tokio::task::spawn_blocking(move || handle.block_on(dispatch(&state, request))).await {
        Ok(Ok(result)) => ResponseMessage::ok(id, result),
        Ok(Err(error)) => ResponseMessage::err(id, error),
        Err(e) => ResponseMessage::err(id, format!("Task panicked: {e}")),
    }
}

use crate::tabs::git::{
    GitDiff, GitDiffViewState, GitError, GitRemote, GitRepository, GitRepositorySnapshot, GitStash,
    GitTag,
};
use crate::tree::{TabId, TabKind, WorkspaceId};

use super::State;

impl State {
    pub fn git_status(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<GitRepositorySnapshot, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::snapshot(directory)
    }

    pub fn git_diff(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<GitDiff, GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let view_state = self
            .git_diff_view_state(workspace_id, tab_id)
            .ok_or(GitError::TabNotFound)?;

        if !self.is_git_diff_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        GitRepository::diff(workspace.directory(), view_state.path())
    }

    pub fn save_git_diff_file(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        path: &str,
        content: &str,
        stage: bool,
    ) -> Result<(), GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !self.is_git_diff_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        GitRepository::save_diff_file(workspace.directory(), path, content, stage)
    }

    pub fn open_git_diff_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        git_tab_id: TabId,
        path: &str,
    ) -> Result<(), GitError> {
        self.mark_persistent_change();

        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let path = GitRepository::normalize_path(path)?;

        self.git_workspace_directory(Some(workspace_id), git_tab_id)?;

        if let Some((pane_id, tab_id)) = self.git_diff_tab(workspace_id) {
            self.update_git_diff_view_state(workspace_id, tab_id, path);

            return if self.activate_tab(Some(workspace_id), pane_id, tab_id) {
                Ok(())
            } else {
                Err(GitError::TabNotFound)
            };
        }

        let target_pane_id = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?
            .root()
            .largest_pane_id();
        let tab = self.next_tab(TabKind::Diff, None);
        let tab_id = tab.id();
        let view_state = GitDiffViewState::new(workspace_id, tab_id, path);
        let workspace = self
            .workspace_mut(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !workspace.add_tab_to_pane(target_pane_id, tab) {
            return Err(GitError::TabNotFound);
        }

        workspace.activate_tab(target_pane_id, tab_id);
        self.git_diff_view_states.push(view_state);

        Ok(())
    }

    pub fn init_git_repository(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::init(directory)
    }

    pub fn stage_git_paths(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        paths: &[String],
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stage_paths(directory, paths)
    }

    pub fn unstage_git_paths(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        paths: &[String],
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::unstage_paths(directory, paths)
    }

    pub fn stage_all_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stage_all(directory)
    }

    pub fn unstage_all_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::unstage_all(directory)
    }

    pub fn commit_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        message: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::commit(directory, message)
    }

    pub fn switch_git_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        branch: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::switch_branch(directory, branch)
    }

    pub fn track_git_remote_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        branch: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::track_remote_branch(directory, branch)
    }

    pub fn create_git_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
        start_point: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::create_branch(directory, name, start_point)
    }

    pub fn delete_git_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        branch: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::delete_branch(directory, branch)
    }

    pub fn fetch_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::fetch(directory)
    }

    pub fn pull_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        rebase: bool,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::pull(directory, rebase)
    }

    pub fn push_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        force: bool,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::push(directory, force)
    }

    pub fn stash_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stash(directory)
    }

    pub fn stash_staged_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stash_staged_changes(directory)
    }

    pub fn git_stashes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitStash>, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stashes(directory)
    }

    pub fn apply_git_stash(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        selector: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::apply_stash(directory, selector)
    }

    pub fn drop_git_stash(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        selector: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::drop_stash(directory, selector)
    }

    pub fn git_remotes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitRemote>, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::remotes(directory)
    }

    pub fn add_git_remote(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
        url: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::add_remote(directory, name, url)
    }

    pub fn remove_git_remote(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::remove_remote(directory, name)
    }

    pub fn git_tags(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitTag>, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::tags(directory)
    }

    pub fn create_git_tag(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::create_tag(directory, name)
    }

    pub fn delete_git_tag(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::delete_tag(directory, name)
    }

    pub fn discard_all_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::discard_all_changes(directory)
    }

    pub fn discard_staged_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::discard_staged_changes(directory)
    }
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::tabs::file_tree::{
    FileTree, FileTreeDirectory, FileTreeEntryKind, FileTreeError, FileTreeViewState,
};
use crate::tabs::git::{
    GitDiff, GitDiffViewState, GitError, GitRepository, GitRepositorySnapshot, GitStash,
};
use crate::tabs::terminal::{TerminalError, TerminalOutput, TerminalSessions, TerminalSize};
use crate::tree::{
    Pane, PaneId, PaneNode, SplitAxis, SplitPaneId, Tab, TabId, TabKind, Workspace, WorkspaceId,
    WorkspaceList,
};

#[derive(Debug)]
pub struct State {
    workspaces: WorkspaceList,
    file_tree_view_states: Vec<FileTreeViewState>,
    git_diff_view_states: Vec<GitDiffViewState>,
    terminal_sessions: TerminalSessions,
    next_workspace_id: u64,
    next_pane_id: u64,
    next_split_id: u64,
    next_tab_id: u64,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_workspaces(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
    ) -> Option<Self> {
        Self::from_workspaces_with_file_tree_view_states(
            workspaces,
            active_workspace_id,
            Vec::new(),
        )
    }

    pub fn from_workspaces_with_file_tree_view_states(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
        file_tree_view_states: Vec<FileTreeViewState>,
    ) -> Option<Self> {
        Self::from_workspaces_with_view_states(
            workspaces,
            active_workspace_id,
            file_tree_view_states,
            Vec::new(),
        )
    }

    pub fn from_workspaces_with_view_states(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
        file_tree_view_states: Vec<FileTreeViewState>,
        git_diff_view_states: Vec<GitDiffViewState>,
    ) -> Option<Self> {
        let mut workspace_list = WorkspaceList::new();

        for workspace in workspaces {
            if !workspace_list.add_workspace(workspace) {
                return None;
            }
        }

        match active_workspace_id {
            Some(active_workspace_id) if workspace_list.activate_workspace(active_workspace_id) => {
            }
            None if workspace_list.is_empty() => {}
            _ => return None,
        }

        Self::from_workspace_list(workspace_list, file_tree_view_states, git_diff_view_states)
    }

    pub fn workspaces(&self) -> &WorkspaceList {
        &self.workspaces
    }

    pub fn file_tree_view_states(&self) -> &[FileTreeViewState] {
        &self.file_tree_view_states
    }

    pub fn git_diff_view_states(&self) -> &[GitDiffViewState] {
        &self.git_diff_view_states
    }

    pub fn file_tree(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: Option<TabId>,
    ) -> Result<FileTree, FileTreeError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;
        let expanded_paths = match tab_id {
            Some(tab_id) if self.is_file_tree_tab(workspace_id, tab_id) => self
                .file_tree_view_state(workspace_id, tab_id)
                .map(FileTreeViewState::expanded_paths)
                .unwrap_or(&[]),
            Some(_) => return Err(FileTreeError::TabNotFound),
            None => &[],
        };

        FileTree::scan_with_expanded_paths(workspace.directory(), expanded_paths)
    }

    pub fn file_tree_children(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        directory_path: &str,
    ) -> Result<FileTreeDirectory, FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;

        FileTree::scan_children(directory, directory_path)
    }

    pub fn set_file_tree_expanded_paths(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        expanded_paths: Vec<String>,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };

        if !self.is_file_tree_tab(workspace_id, tab_id) {
            return false;
        }

        let view_state = FileTreeViewState::new(workspace_id, tab_id, expanded_paths);
        self.remove_file_tree_view_state(workspace_id, tab_id);

        if !view_state.expanded_paths().is_empty() {
            self.file_tree_view_states.push(view_state);
        }

        true
    }

    pub fn create_file_tree_entry(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        parent_path: Option<&str>,
        name: &str,
        kind: FileTreeEntryKind,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::create_entry(directory, parent_path, name, kind).map(|_| ())
    }

    pub fn rename_file_tree_entry(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_path: &str,
        destination_path: &str,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::rename_entry(directory, source_path, destination_path).map(|_| ())
    }

    pub fn move_file_tree_entries(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::move_entries(directory, source_paths, target_directory_path).map(|_| ())
    }

    pub fn copy_file_tree_entries(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::copy_entries(directory, source_paths, target_directory_path).map(|_| ())
    }

    pub fn delete_file_tree_entries(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        paths: &[String],
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::delete_entries(directory, paths)
    }

    pub fn resolve_file_tree_path(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        path: Option<&str>,
    ) -> Result<PathBuf, FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::resolve_path(directory, path)
    }

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

    pub fn open_git_diff_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        git_tab_id: TabId,
        path: &str,
    ) -> Result<(), GitError> {
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

    pub fn open_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(TerminalError::WorkspaceNotFound)?;
        let directory = self
            .terminal_workspace_directory(workspace_id, tab_id)?
            .to_path_buf();
        let size = TerminalSize::new(columns, rows)?;

        self.terminal_sessions
            .open(workspace_id, tab_id, &directory, size)
    }

    pub fn write_terminal_input(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        input: &str,
    ) -> Result<(), TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;

        self.terminal_sessions
            .write_input(workspace_id, tab_id, input)
    }

    pub fn read_terminal_output(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;

        self.terminal_sessions.read_output(workspace_id, tab_id)
    }

    pub fn resize_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
    ) -> Result<(), TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;
        let size = TerminalSize::new(columns, rows)?;

        self.terminal_sessions.resize(workspace_id, tab_id, size)
    }

    pub fn open_workspace(&mut self, directory: impl Into<PathBuf>) -> WorkspaceId {
        let directory = directory.into();

        if let Some(workspace_id) = self
            .workspaces
            .workspaces()
            .iter()
            .find(|workspace| workspace.directory() == directory.as_path())
            .map(Workspace::id)
        {
            self.workspaces.activate_workspace(workspace_id);
            return workspace_id;
        }

        let workspace_id = self.next_workspace_id();
        let initial_pane = self.blank_pane();
        let workspace = Workspace::new(workspace_id, directory, initial_pane);

        self.workspaces.add_workspace(workspace);

        workspace_id
    }

    pub fn activate_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        self.workspaces.activate_workspace(workspace_id)
    }

    pub fn close_workspace(&mut self, workspace_id: Option<WorkspaceId>) -> Option<Workspace> {
        let closed_workspace = match workspace_id {
            Some(workspace_id) => self.workspaces.close_workspace(workspace_id),
            None => self.workspaces.close_active_workspace(),
        };

        if let Some(workspace) = &closed_workspace {
            self.remove_workspace_file_tree_view_states(workspace.id());
            self.remove_workspace_git_diff_view_states(workspace.id());
            self.terminal_sessions.close_workspace(workspace.id());
        }

        closed_workspace
    }

    pub fn split_pane(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: Option<PaneId>,
        axis: SplitAxis,
        new_pane_first: bool,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let split_id = self.next_split_id();
        let new_pane = self.blank_pane();
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };
        let pane_id = pane_id.unwrap_or_else(|| workspace.active_pane_id());

        workspace.split_pane_with_new_pane_first(
            split_id,
            pane_id,
            axis,
            new_pane,
            0.5,
            new_pane_first,
        )
    }

    pub fn activate_pane(&mut self, workspace_id: Option<WorkspaceId>, pane_id: PaneId) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.activate_pane(pane_id)
    }

    pub fn move_pane(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        target_pane_id: PaneId,
        axis: SplitAxis,
        new_pane_first: bool,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let split_id = self.next_split_id();
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.move_pane_to_split(split_id, pane_id, target_pane_id, axis, 0.5, new_pane_first)
    }

    pub fn open_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: Option<PaneId>,
        title: Option<String>,
        kind: TabKind,
    ) -> bool {
        if kind == TabKind::Diff {
            return false;
        }

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let tab = self.next_tab(kind, title);
        let tab_id = tab.id();
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };
        let pane_id = pane_id.unwrap_or_else(|| workspace.active_pane_id());

        if workspace.add_tab_to_pane(pane_id, tab) {
            workspace.activate_tab(pane_id, tab_id);
            true
        } else {
            false
        }
    }

    pub fn activate_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.activate_tab(pane_id, tab_id)
    }

    pub fn set_tab_kind(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
        kind: TabKind,
    ) -> bool {
        if kind == TabKind::Diff {
            return false;
        }

        let keep_file_tree_state = kind == TabKind::FileTree;
        let keep_terminal_session = kind == TabKind::Terminal;
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let close_terminal_session = !keep_terminal_session
            && self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Terminal);
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        let updated = workspace.set_tab_kind(pane_id, tab_id, kind);

        if updated && !keep_file_tree_state {
            self.remove_file_tree_view_state(workspace_id, tab_id);
        }

        if updated {
            self.remove_git_diff_view_state(workspace_id, tab_id);
        }

        if updated && close_terminal_session {
            self.terminal_sessions.close(workspace_id, tab_id);
        }

        updated
    }

    pub fn split_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        target_pane_id: PaneId,
        tab_id: TabId,
        axis: SplitAxis,
        new_pane_first: bool,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspaces.workspace(workspace_id) else {
            return false;
        };
        let Some(source_pane) = workspace.root().find_pane(pane_id) else {
            return false;
        };
        if !workspace.root().contains_pane(target_pane_id) {
            return false;
        }
        if !source_pane.contains_tab(tab_id) {
            return false;
        }

        let fallback_tab =
            (source_pane.tabs().len() == 1).then(|| self.next_tab(TabKind::Blank, None));
        let new_pane_id = self.next_pane_id();
        let split_id = self.next_split_id();

        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };
        let Some(source_pane) = workspace.root_mut().find_pane_mut(pane_id) else {
            return false;
        };
        let Some(tab) = source_pane.remove_tab(tab_id) else {
            return false;
        };

        if let Some(fallback_tab) = fallback_tab {
            source_pane.insert_tab(0, fallback_tab);
        }

        let new_pane = Pane::new(new_pane_id, tab);
        workspace.split_pane_with_new_pane_first(
            split_id,
            target_pane_id,
            axis,
            new_pane,
            0.5,
            new_pane_first,
        )
    }

    pub fn close_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
    ) -> Option<Tab> {
        let workspace_id = self.resolve_workspace_id(workspace_id)?;
        let fallback_pane = self.blank_pane();
        let workspace = self.workspace_mut(workspace_id)?;

        let removed_tab = workspace.close_tab(pane_id, tab_id, fallback_pane);

        if removed_tab.is_some() {
            self.remove_file_tree_view_state(workspace_id, tab_id);
            self.remove_git_diff_view_state(workspace_id, tab_id);
        }

        if removed_tab
            .as_ref()
            .is_some_and(|tab| tab.kind() == &TabKind::Terminal)
        {
            self.terminal_sessions.close(workspace_id, tab_id);
        }

        removed_tab
    }

    pub fn move_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        target_pane_id: PaneId,
        tab_id: TabId,
        target_index: usize,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.move_tab_to_pane(pane_id, target_pane_id, tab_id, target_index)
    }

    pub fn resize_split(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        split_id: SplitPaneId,
        ratio: f32,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.resize_split(split_id, ratio)
    }

    fn workspace_mut(&mut self, workspace_id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.workspace_mut(workspace_id)
    }

    fn resolve_workspace_id(&self, workspace_id: Option<WorkspaceId>) -> Option<WorkspaceId> {
        workspace_id.or_else(|| self.workspaces.active_workspace_id())
    }

    fn next_workspace_id(&mut self) -> WorkspaceId {
        let workspace_id = WorkspaceId::new(self.next_workspace_id);
        self.next_workspace_id += 1;
        workspace_id
    }

    fn next_pane_id(&mut self) -> PaneId {
        let pane_id = PaneId::new(self.next_pane_id);
        self.next_pane_id += 1;
        pane_id
    }

    fn next_split_id(&mut self) -> SplitPaneId {
        let split_id = SplitPaneId::new(self.next_split_id);
        self.next_split_id += 1;
        split_id
    }

    fn next_tab_id(&mut self) -> TabId {
        let tab_id = TabId::new(self.next_tab_id);
        self.next_tab_id += 1;
        tab_id
    }

    fn blank_pane(&mut self) -> Pane {
        let pane_id = self.next_pane_id();
        let tab = self.next_tab(TabKind::Blank, None);

        Pane::new(pane_id, tab)
    }

    fn next_tab(&mut self, kind: TabKind, title: Option<String>) -> Tab {
        let title = title.unwrap_or_else(|| kind.default_title().to_owned());

        Tab::new(self.next_tab_id(), title, kind)
    }

    fn file_tree_view_state(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Option<&FileTreeViewState> {
        self.file_tree_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
    }

    fn remove_file_tree_view_state(&mut self, workspace_id: WorkspaceId, tab_id: TabId) {
        self.file_tree_view_states
            .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
    }

    fn remove_workspace_file_tree_view_states(&mut self, workspace_id: WorkspaceId) {
        self.file_tree_view_states
            .retain(|state| state.workspace_id() != workspace_id);
    }

    fn git_diff_view_state(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Option<&GitDiffViewState> {
        self.git_diff_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
    }

    fn git_diff_tab(&self, workspace_id: WorkspaceId) -> Option<(PaneId, TabId)> {
        self.git_diff_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id)
            .and_then(|state| {
                tab_pane_id_in_workspace_list(&self.workspaces, workspace_id, state.tab_id())
                    .map(|pane_id| (pane_id, state.tab_id()))
            })
    }

    fn update_git_diff_view_state(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        path: impl Into<String>,
    ) {
        if let Some(state) = self
            .git_diff_view_states
            .iter_mut()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
        {
            state.set_path(path);
        }
    }

    fn remove_git_diff_view_state(&mut self, workspace_id: WorkspaceId, tab_id: TabId) {
        self.git_diff_view_states
            .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
    }

    fn remove_workspace_git_diff_view_states(&mut self, workspace_id: WorkspaceId) {
        self.git_diff_view_states
            .retain(|state| state.workspace_id() != workspace_id);
    }

    fn is_file_tree_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::FileTree)
    }

    fn is_terminal_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Terminal)
    }

    fn is_git_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Git)
    }

    fn is_git_diff_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Diff)
    }

    fn file_tree_workspace_directory(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<&Path, FileTreeError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;

        if !self.is_file_tree_tab(workspace_id, tab_id) {
            return Err(FileTreeError::TabNotFound);
        }

        Ok(workspace.directory())
    }

    fn terminal_workspace_id(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<WorkspaceId, TerminalError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(TerminalError::WorkspaceNotFound)?;

        self.terminal_workspace_directory(workspace_id, tab_id)?;

        Ok(workspace_id)
    }

    fn terminal_workspace_directory(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Result<&Path, TerminalError> {
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(TerminalError::WorkspaceNotFound)?;

        if !self.is_terminal_tab(workspace_id, tab_id) {
            return Err(TerminalError::TabNotFound);
        }

        Ok(workspace.directory())
    }

    fn git_workspace_directory(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<&Path, GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !self.is_git_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        Ok(workspace.directory())
    }

    fn tab_kind(&self, workspace_id: WorkspaceId, tab_id: TabId) -> Option<&TabKind> {
        let workspace = self.workspaces.workspace(workspace_id)?;

        tab_kind_in_node(workspace.root(), tab_id)
    }

    fn from_workspace_list(
        workspaces: WorkspaceList,
        file_tree_view_states: Vec<FileTreeViewState>,
        git_diff_view_states: Vec<GitDiffViewState>,
    ) -> Option<Self> {
        let next_ids = NextIds::from_workspaces(&workspaces)?;

        if file_tree_view_states.iter().any(|state| {
            tab_kind_in_workspace_list(&workspaces, state.workspace_id(), state.tab_id())
                != Some(&TabKind::FileTree)
        }) {
            return None;
        }

        if !git_diff_view_states_are_valid(&workspaces, &git_diff_view_states) {
            return None;
        }

        Some(Self {
            workspaces,
            file_tree_view_states,
            git_diff_view_states,
            terminal_sessions: TerminalSessions::default(),
            next_workspace_id: next_ids.workspace_id,
            next_pane_id: next_ids.pane_id,
            next_split_id: next_ids.split_id,
            next_tab_id: next_ids.tab_id,
        })
    }
}

#[derive(Debug, Default)]
struct MaxIds {
    workspace_id: u64,
    pane_id: u64,
    split_id: u64,
    tab_id: u64,
}

impl MaxIds {
    fn visit_workspace(&mut self, workspace: &Workspace) {
        self.workspace_id = self.workspace_id.max(workspace.id().value());
        self.visit_pane_node(workspace.root());
    }

    fn visit_pane_node(&mut self, node: &crate::tree::PaneNode) {
        match node {
            crate::tree::PaneNode::Leaf(pane) => self.visit_pane(pane),
            crate::tree::PaneNode::Split(split) => {
                self.split_id = self.split_id.max(split.id().value());
                self.visit_pane_node(split.first());
                self.visit_pane_node(split.second());
            }
        }
    }

    fn visit_pane(&mut self, pane: &Pane) {
        self.pane_id = self.pane_id.max(pane.id().value());

        for tab in pane.tabs() {
            self.tab_id = self.tab_id.max(tab.id().value());
        }
    }
}

#[derive(Debug)]
struct NextIds {
    workspace_id: u64,
    pane_id: u64,
    split_id: u64,
    tab_id: u64,
}

impl NextIds {
    fn from_workspaces(workspaces: &WorkspaceList) -> Option<Self> {
        let mut max_ids = MaxIds::default();

        for workspace in workspaces.workspaces() {
            max_ids.visit_workspace(workspace);
        }

        Some(Self {
            workspace_id: next_id_after(max_ids.workspace_id)?,
            pane_id: next_id_after(max_ids.pane_id)?,
            split_id: next_id_after(max_ids.split_id)?,
            tab_id: next_id_after(max_ids.tab_id)?,
        })
    }
}

fn next_id_after(id: u64) -> Option<u64> {
    id.checked_add(1)
}

fn tab_kind_in_workspace_list(
    workspaces: &WorkspaceList,
    workspace_id: WorkspaceId,
    tab_id: TabId,
) -> Option<&TabKind> {
    let workspace = workspaces.workspace(workspace_id)?;

    tab_kind_in_node(workspace.root(), tab_id)
}

fn tab_pane_id_in_workspace_list(
    workspaces: &WorkspaceList,
    workspace_id: WorkspaceId,
    tab_id: TabId,
) -> Option<PaneId> {
    let workspace = workspaces.workspace(workspace_id)?;

    tab_pane_id_in_node(workspace.root(), tab_id)
}

fn tab_kind_in_node(node: &PaneNode, tab_id: TabId) -> Option<&TabKind> {
    match node {
        PaneNode::Leaf(pane) => pane
            .tabs()
            .iter()
            .find(|tab| tab.id() == tab_id)
            .map(Tab::kind),
        PaneNode::Split(split) => tab_kind_in_node(split.first(), tab_id)
            .or_else(|| tab_kind_in_node(split.second(), tab_id)),
    }
}

fn tab_pane_id_in_node(node: &PaneNode, tab_id: TabId) -> Option<PaneId> {
    match node {
        PaneNode::Leaf(pane) if pane.contains_tab(tab_id) => Some(pane.id()),
        PaneNode::Leaf(_) => None,
        PaneNode::Split(split) => tab_pane_id_in_node(split.first(), tab_id)
            .or_else(|| tab_pane_id_in_node(split.second(), tab_id)),
    }
}

fn git_diff_view_states_are_valid(
    workspaces: &WorkspaceList,
    view_states: &[GitDiffViewState],
) -> bool {
    let mut tabs_with_view_state = HashSet::new();
    let mut workspaces_with_view_state = HashSet::new();

    for state in view_states {
        let key = (state.workspace_id(), state.tab_id());

        if tab_kind_in_workspace_list(workspaces, key.0, key.1) != Some(&TabKind::Diff)
            || !tabs_with_view_state.insert(key)
            || !workspaces_with_view_state.insert(state.workspace_id())
        {
            return false;
        }
    }

    workspaces.workspaces().iter().all(|workspace| {
        diff_tabs_have_view_state(workspace.root(), workspace.id(), &tabs_with_view_state)
    })
}

fn diff_tabs_have_view_state(
    node: &PaneNode,
    workspace_id: WorkspaceId,
    tabs_with_view_state: &HashSet<(WorkspaceId, TabId)>,
) -> bool {
    match node {
        PaneNode::Leaf(pane) => pane.tabs().iter().all(|tab| {
            tab.kind() != &TabKind::Diff || tabs_with_view_state.contains(&(workspace_id, tab.id()))
        }),
        PaneNode::Split(split) => {
            diff_tabs_have_view_state(split.first(), workspace_id, tabs_with_view_state)
                && diff_tabs_have_view_state(split.second(), workspace_id, tabs_with_view_state)
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            workspaces: WorkspaceList::new(),
            file_tree_view_states: Vec::new(),
            git_diff_view_states: Vec::new(),
            terminal_sessions: TerminalSessions::default(),
            next_workspace_id: 1,
            next_pane_id: 1,
            next_split_id: 1,
            next_tab_id: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_workspace_creates_active_workspace() {
        let mut state = State::new();

        let workspace_id = state.open_workspace("/workspaces/main");

        assert_eq!(workspace_id, WorkspaceId::new(1));
        assert_eq!(state.workspaces().active_workspace_id(), Some(workspace_id));
        assert_eq!(state.workspaces().workspaces().len(), 1);
    }

    #[test]
    fn opening_existing_workspace_path_activates_existing_workspace() {
        let mut state = State::new();

        let first_workspace_id = state.open_workspace("/workspaces/first");
        let second_workspace_id = state.open_workspace("/workspaces/second");
        let reopened_workspace_id = state.open_workspace("/workspaces/first");

        assert_eq!(second_workspace_id, WorkspaceId::new(2));
        assert_eq!(reopened_workspace_id, first_workspace_id);
        assert_eq!(
            state.workspaces().active_workspace_id(),
            Some(first_workspace_id)
        );
        assert_eq!(state.workspaces().workspaces().len(), 2);
        assert_eq!(
            state.open_workspace("/workspaces/third"),
            WorkspaceId::new(3)
        );
    }

    #[test]
    fn opening_tab_adds_it_to_active_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(state.open_tab(None, None, None, TabKind::Search));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let pane = workspace
            .active_pane()
            .expect("workspace should have an active pane");

        assert_eq!(pane.tabs().len(), 2);
        assert_eq!(pane.active_tab().title(), "Search");
    }

    #[test]
    fn generic_tab_operations_cannot_create_diff_tabs() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(!state.open_tab(None, None, None, TabKind::Diff));
        assert!(!state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Diff,));

        let active_tab = state
            .workspaces()
            .active_workspace()
            .expect("workspace should exist")
            .active_pane()
            .expect("pane should exist")
            .active_tab();

        assert_eq!(active_tab.kind(), &TabKind::Blank);
        assert!(state.git_diff_view_states().is_empty());
    }

    #[test]
    fn restoring_diff_tabs_requires_exactly_one_view_state() {
        let workspace_id = WorkspaceId::new(1);
        let tab_id = TabId::new(1);
        let workspace = Workspace::new(
            workspace_id,
            "/workspaces/main",
            Pane::new(PaneId::new(1), Tab::new(tab_id, "Diff", TabKind::Diff)),
        );

        assert!(State::from_workspaces(vec![workspace.clone()], Some(workspace_id)).is_none());
        assert!(
            State::from_workspaces_with_view_states(
                vec![workspace],
                Some(workspace_id),
                Vec::new(),
                vec![
                    GitDiffViewState::new(workspace_id, tab_id, "README.md"),
                    GitDiffViewState::new(workspace_id, tab_id, "README.md"),
                ],
            )
            .is_none()
        );
    }

    #[test]
    fn setting_tab_kind_updates_kind_and_default_title() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Git));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let pane = workspace
            .active_pane()
            .expect("workspace should have an active pane");

        assert_eq!(pane.active_tab().title(), "Git");
        assert_eq!(pane.active_tab().kind(), &TabKind::Git);
    }

    #[test]
    fn opening_git_diff_tab_places_it_in_largest_pane() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Git,
        ));
        assert!(state.split_pane(
            Some(workspace_id),
            Some(PaneId::new(1)),
            SplitAxis::Horizontal,
            false,
        ));
        assert!(state.resize_split(Some(workspace_id), SplitPaneId::new(1), 0.7));

        state
            .open_git_diff_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .expect("diff tab should open");

        let workspace = state
            .workspaces()
            .workspace(workspace_id)
            .expect("workspace should exist");
        let largest_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("largest pane should exist");

        assert_eq!(workspace.active_pane_id(), PaneId::new(1));
        assert_eq!(largest_pane.active_tab_id(), TabId::new(3));
        assert_eq!(largest_pane.active_tab().kind(), &TabKind::Diff);
        assert_eq!(state.git_diff_view_states()[0].path(), "src/main.rs");
    }

    #[test]
    fn opening_existing_git_diff_tab_reuses_it_and_updates_focus_path() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Git,
        ));

        state
            .open_git_diff_tab(Some(workspace_id), TabId::new(1), "README.md")
            .expect("diff tab should open");
        state
            .open_git_diff_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .expect("existing diff tab should activate");

        let workspace = state
            .workspaces()
            .workspace(workspace_id)
            .expect("workspace should exist");
        let pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("pane should exist");

        assert_eq!(pane.tabs().len(), 2);
        assert_eq!(state.git_diff_view_states().len(), 1);
        assert_eq!(state.git_diff_view_states()[0].path(), "src/main.rs");
        assert_eq!(pane.active_tab_id(), TabId::new(2));
        assert_eq!(pane.active_tab().title(), "Diff");
    }

    #[test]
    fn splitting_tab_moves_it_to_a_new_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        state.open_tab(None, None, None, TabKind::Search);

        assert!(state.split_tab(
            None,
            PaneId::new(1),
            PaneId::new(1),
            TabId::new(2),
            SplitAxis::Horizontal,
            false,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");

        assert_eq!(workspace.root().pane_count(), 2);
        assert_eq!(workspace.active_pane_id(), PaneId::new(2));

        let source_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("source pane should remain");
        let new_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("new pane should exist");

        assert_eq!(source_pane.tabs().len(), 1);
        assert_eq!(new_pane.active_tab().id(), TabId::new(2));
    }

    #[test]
    fn splitting_only_tab_keeps_source_pane_valid() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(state.split_tab(
            None,
            PaneId::new(1),
            PaneId::new(1),
            TabId::new(1),
            SplitAxis::Vertical,
            false,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let source_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("source pane should remain");
        let new_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("new pane should exist");

        assert_eq!(source_pane.tabs().len(), 1);
        assert_eq!(new_pane.active_tab().id(), TabId::new(1));
    }

    #[test]
    fn moving_pane_reuses_existing_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.move_pane(
            None,
            PaneId::new(1),
            PaneId::new(2),
            SplitAxis::Vertical,
            false,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");

        assert_eq!(workspace.root().pane_count(), 2);
        assert!(workspace.root().contains_pane(PaneId::new(1)));
        assert!(workspace.root().contains_pane(PaneId::new(2)));
        assert_eq!(workspace.active_pane_id(), PaneId::new(1));
    }

    #[test]
    fn moving_tab_to_another_pane_adds_it_to_target_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        state.open_tab(None, None, None, TabKind::Search);
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.move_tab(None, PaneId::new(1), PaneId::new(2), TabId::new(2), 1,));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let source_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("source pane should remain");
        let target_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("target pane should exist");

        assert_eq!(source_pane.tabs().len(), 1);
        assert_eq!(
            target_pane.tabs().iter().map(Tab::id).collect::<Vec<_>>(),
            vec![TabId::new(3), TabId::new(2)]
        );
        assert_eq!(target_pane.active_tab_id(), TabId::new(2));
        assert_eq!(workspace.active_pane_id(), PaneId::new(2));
    }

    #[test]
    fn moving_last_tab_to_another_pane_removes_source_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.move_tab(
            None,
            PaneId::new(1),
            PaneId::new(2),
            TabId::new(1),
            usize::MAX,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let target_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("target pane should exist");

        assert_eq!(workspace.root().pane_count(), 1);
        assert!(!workspace.root().contains_pane(PaneId::new(1)));
        assert_eq!(
            target_pane.tabs().iter().map(Tab::id).collect::<Vec<_>>(),
            vec![TabId::new(2), TabId::new(1)]
        );
        assert_eq!(target_pane.active_tab_id(), TabId::new(1));
        assert_eq!(workspace.active_pane_id(), PaneId::new(2));
    }

    #[test]
    fn resizing_split_updates_server_owned_ratio() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.resize_split(None, SplitPaneId::new(1), 0.7));
        assert!(!state.resize_split(None, SplitPaneId::new(1), 1.0));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let crate::tree::PaneNode::Split(split) = workspace.root() else {
            panic!("workspace root should be split");
        };

        assert_eq!(split.ratio(), 0.7);
    }

    #[test]
    fn resized_split_survives_workspace_switches() {
        let mut state = State::new();
        let first_workspace_id = state.open_workspace("/workspaces/first");
        assert!(state.split_pane(Some(first_workspace_id), None, SplitAxis::Horizontal, false,));
        assert!(state.resize_split(Some(first_workspace_id), SplitPaneId::new(1), 0.7));

        state.open_workspace("/workspaces/second");
        assert!(state.activate_workspace(first_workspace_id));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("first workspace should be active again");
        let crate::tree::PaneNode::Split(split) = workspace.root() else {
            panic!("workspace root should be split");
        };

        assert_eq!(split.ratio(), 0.7);
    }

    #[test]
    fn file_tree_requires_a_workspace() {
        let state = State::new();

        let error = state
            .file_tree(None, None)
            .expect_err("missing workspace should fail");

        assert!(matches!(error, FileTreeError::WorkspaceNotFound));
    }

    #[test]
    fn file_tree_expanded_paths_are_stored_for_file_tree_tabs() {
        let root = test_directory("file-tree-state");
        std::fs::create_dir(root.join("src")).expect("test directory should be created");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::FileTree));

        assert!(state.set_file_tree_expanded_paths(
            Some(workspace_id),
            TabId::new(1),
            vec!["src".to_owned(), "missing".to_owned()],
        ));

        let file_tree = state
            .file_tree(Some(workspace_id), Some(TabId::new(1)))
            .expect("file tree should load");

        assert_eq!(file_tree.expanded_paths(), &["src/"]);
        assert_eq!(state.file_tree_view_states().len(), 1);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn file_tree_expanded_paths_require_a_file_tree_tab() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");

        assert!(!state.set_file_tree_expanded_paths(
            Some(workspace_id),
            TabId::new(1),
            vec!["src".to_owned()],
        ));

        let error = state
            .file_tree(Some(workspace_id), Some(TabId::new(1)))
            .expect_err("blank tabs should not expose file tree state");

        assert!(matches!(error, FileTreeError::TabNotFound));
    }

    #[test]
    fn terminal_sessions_require_a_terminal_tab() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");

        let error = state
            .read_terminal_output(Some(workspace_id), TabId::new(1))
            .expect_err("blank tabs should not expose terminal sessions");

        assert!(matches!(error, TerminalError::TabNotFound));
        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Terminal));

        let error = state
            .read_terminal_output(Some(workspace_id), TabId::new(1))
            .expect_err("terminal tabs should require a started session");

        assert!(matches!(error, TerminalError::SessionNotFound));
    }

    #[test]
    fn closing_tab_removes_file_tree_view_state() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");
        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::FileTree));
        assert!(state.set_file_tree_expanded_paths(
            Some(workspace_id),
            TabId::new(1),
            vec!["src".to_owned()],
        ));

        assert!(
            state
                .close_tab(Some(workspace_id), PaneId::new(1), TabId::new(1))
                .is_some()
        );

        assert!(state.file_tree_view_states().is_empty());
    }

    fn test_directory(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kosmos-core-state-{}-{name}-{nanos}",
            std::process::id()
        ));

        std::fs::create_dir_all(&root).expect("test root should be created");

        root
    }
}

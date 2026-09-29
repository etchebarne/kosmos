use std::path::{Path, PathBuf};

use crate::tabs::file_tree::{
    EntryTransfer, FileTree, FileTreeDirectory, FileTreeEntryKind, FileTreeError, FileTreeViewState,
};
use crate::tabs::git::{FileTreeGitDecorations, GitError, GitRepository};
use crate::tree::{TabId, WorkspaceId};

use super::{FileTreeGitDecorationsError, State};

/// A workspace-relative entry path that a file tree mutation moved elsewhere.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryRelocation {
    pub source: String,
    pub destination: String,
}

impl State {
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

    pub fn file_tree_root(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<&Path, FileTreeError> {
        self.file_tree_workspace_directory(workspace_id, tab_id)
    }

    pub fn file_tree_git_decorations(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<FileTreeGitDecorations, FileTreeGitDecorationsError> {
        let directory = self
            .file_tree_workspace_directory(workspace_id, tab_id)
            .map_err(FileTreeGitDecorationsError::FileTree)?;

        match GitRepository::workspace_changes(directory) {
            Ok(changes) => Ok(FileTreeGitDecorations::from_changes(changes)),
            Err(GitError::Discover { .. } | GitError::NotWorktree(_)) => {
                Ok(FileTreeGitDecorations::default())
            }
            Err(error) => Err(FileTreeGitDecorationsError::Git(error)),
        }
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
        self.mark_persistent_change();

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
    ) -> Result<Vec<EntryRelocation>, FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        let destination = FileTree::rename_entry(directory, source_path, destination_path)?;
        let transfer = EntryTransfer {
            source: directory.join(source_path),
            destination,
        };
        Ok(entry_relocations(directory, &[transfer]))
    }

    pub fn move_file_tree_entries(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<Vec<EntryRelocation>, FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        let transfers = FileTree::move_entries(directory, source_paths, target_directory_path)?;
        Ok(entry_relocations(directory, &transfers))
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
}

fn entry_relocations(root: &Path, transfers: &[EntryTransfer]) -> Vec<EntryRelocation> {
    transfers
        .iter()
        .filter_map(|transfer| {
            Some(EntryRelocation {
                source: root_relative_path(root, &transfer.source)?,
                destination: root_relative_path(root, &transfer.destination)?,
            })
        })
        .filter(|relocation| relocation.source != relocation.destination)
        .collect()
}

fn root_relative_path(root: &Path, path: &Path) -> Option<String> {
    let components = path
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    (!components.is_empty()).then(|| components.join("/"))
}

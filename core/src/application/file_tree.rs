use crate::state::EntryRelocation;
use crate::tabs::file_tree::FileTreeError;
use crate::tree::{TabId, WorkspaceId};

use super::{Application, ApplicationError, EditorSessionId};

impl Application {
    /// Renames a file tree entry and keeps open editors attached to their documents.
    pub fn rename_file_tree_entry(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_path: &str,
        destination_path: &str,
    ) -> Result<(), ApplicationError> {
        let workspace_id = self.file_tree_workspace_id(workspace_id)?;
        let relocations = self.state.rename_file_tree_entry(
            Some(workspace_id),
            tab_id,
            source_path,
            destination_path,
        )?;
        self.follow_entry_relocations(workspace_id, &relocations)
    }

    /// Moves file tree entries and keeps open editors attached to their documents.
    pub fn move_file_tree_entries(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<(), ApplicationError> {
        let workspace_id = self.file_tree_workspace_id(workspace_id)?;
        let relocations = self.state.move_file_tree_entries(
            Some(workspace_id),
            tab_id,
            source_paths,
            target_directory_path,
        )?;
        self.follow_entry_relocations(workspace_id, &relocations)
    }

    /// Deletes file tree entries and closes the editors that showed them. Unsaved
    /// editors block the deletion so their changes are never discarded silently.
    pub fn delete_file_tree_entries(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        paths: &[String],
    ) -> Result<(), ApplicationError> {
        let workspace_id = self.file_tree_workspace_id(workspace_id)?;
        self.ensure_no_unsaved_documents_at(workspace_id, paths)?;
        self.state
            .delete_file_tree_entries(Some(workspace_id), tab_id, paths)?;
        self.close_deleted_editor_tabs(workspace_id, paths)
    }

    fn file_tree_workspace_id(
        &self,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<WorkspaceId, ApplicationError> {
        self.state
            .resolve_workspace_id(workspace_id)
            .ok_or(ApplicationError::FileTree(FileTreeError::WorkspaceNotFound))
    }

    fn follow_entry_relocations(
        &mut self,
        workspace_id: WorkspaceId,
        relocations: &[EntryRelocation],
    ) -> Result<(), ApplicationError> {
        self.persist_state_change(|state| state.retarget_editor_tabs(workspace_id, relocations))?;
        for relocation in relocations {
            self.editor_sessions.retarget_path(
                workspace_id,
                &relocation.source,
                &relocation.destination,
            );
        }
        Ok(())
    }

    fn ensure_no_unsaved_documents_at(
        &self,
        workspace_id: WorkspaceId,
        paths: &[String],
    ) -> Result<(), ApplicationError> {
        let unsaved = paths
            .iter()
            .flat_map(|path| self.editor_sessions.at_or_below(workspace_id, path))
            .filter(|session| session.is_dirty())
            .map(|session| session.path)
            .collect::<Vec<_>>();
        if unsaved.is_empty() {
            Ok(())
        } else {
            Err(ApplicationError::UnsavedDocuments(unsaved))
        }
    }

    fn close_deleted_editor_tabs(
        &mut self,
        workspace_id: WorkspaceId,
        paths: &[String],
    ) -> Result<(), ApplicationError> {
        let tab_ids = self.state.editor_tabs_at_or_below(workspace_id, paths);
        self.persist_state_change(|state| {
            state.close_editor_tabs_at_or_below(workspace_id, paths)
        })?;
        for tab_id in tab_ids {
            self.editor_sessions
                .remove(EditorSessionId::new(workspace_id, tab_id));
        }
        Ok(())
    }
}

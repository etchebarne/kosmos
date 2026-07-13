use crate::tabs::editor::{
    EditorDocument, EditorError, EditorLocation, EditorViewState,
    normalize_path as normalize_editor_path, save_document,
};
use crate::tabs::git::{GitError, GitLineHunk, GitRepository};
use crate::tree::{TabId, TabKind, WorkspaceId};

use super::{OpenEditorLocation, State};

impl State {
    pub fn open_editor_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        source_tab_id: TabId,
        path: &str,
    ) -> Result<(), EditorError> {
        self.mark_persistent_change();

        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !self.is_editor_source_tab(workspace_id, source_tab_id) {
            return Err(EditorError::SourceTabNotFound);
        }

        let document = EditorDocument::read(workspace.directory(), path)?;
        let path = document.path().to_owned();

        if let Some((pane_id, tab_id)) = self.editor_tab(workspace_id, &path) {
            return if self.activate_tab(Some(workspace_id), pane_id, tab_id) {
                Ok(())
            } else {
                Err(EditorError::TabNotFound)
            };
        }

        let target_pane_id = workspace.root().largest_pane_id();
        let title = path
            .rsplit('/')
            .next()
            .expect("normalized editor paths have a file name")
            .to_owned();
        let tab = self.next_tab(TabKind::Editor, Some(title));
        let tab_id = tab.id();
        let view_state = EditorViewState::new(workspace_id, tab_id, path);
        let workspace = self
            .workspace_mut(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !workspace.add_tab_to_pane(target_pane_id, tab) {
            return Err(EditorError::TabNotFound);
        }

        workspace.activate_tab(target_pane_id, tab_id);
        self.editor_view_states.push(view_state);

        Ok(())
    }

    pub fn open_editor_location(
        &mut self,
        workspace_id: WorkspaceId,
        path: &str,
    ) -> Result<OpenEditorLocation, EditorError> {
        let path = normalize_editor_path(path)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        let source_tab_id = self
            .editor_source_tab_id(workspace_id)
            .ok_or(EditorError::SourceTabNotFound)?;
        let document = EditorDocument::read(workspace.directory(), &path)?;
        let path = document.path().to_owned();
        let existing_tab = self.editor_tab(workspace_id, &path);
        let target_pane_id = workspace.root().largest_pane_id();

        self.mark_persistent_change();

        let tab_id = if let Some((pane_id, tab_id)) = existing_tab {
            if !self.activate_tab(Some(workspace_id), pane_id, tab_id) {
                return Err(EditorError::TabNotFound);
            }
            tab_id
        } else {
            let title = path
                .rsplit('/')
                .next()
                .expect("normalized editor paths have a file name")
                .to_owned();
            let tab = self.next_tab(TabKind::Editor, Some(title));
            let tab_id = tab.id();
            let view_state = EditorViewState::new(workspace_id, tab_id, path.clone());
            let workspace = self
                .workspace_mut(workspace_id)
                .ok_or(EditorError::WorkspaceNotFound)?;

            if !workspace.add_tab_to_pane(target_pane_id, tab) {
                return Err(EditorError::TabNotFound);
            }

            workspace.activate_tab(target_pane_id, tab_id);
            self.editor_view_states.push(view_state);
            tab_id
        };

        if !self.workspaces.activate_workspace(workspace_id) {
            return Err(EditorError::WorkspaceNotFound);
        }

        Ok(OpenEditorLocation {
            workspaces: self.workspaces.clone(),
            source_tab_id,
            workspace_id,
            tab_id,
            path,
        })
    }
    pub fn editor_document(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<EditorDocument, EditorError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }

        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?;

        EditorDocument::read(workspace.directory(), view_state.path())
    }

    pub fn editor_session_target(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(WorkspaceId, String), EditorError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }
        let path = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?
            .path()
            .to_owned();
        Ok((workspace_id, path))
    }

    pub fn editor_location(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Result<EditorLocation, EditorError> {
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }
        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?;

        EditorLocation::resolve(workspace.directory(), view_state.path())
    }

    pub fn save_editor_document(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        content: &str,
    ) -> Result<(), EditorError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }

        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?;

        save_document(workspace.directory(), view_state.path(), content)
    }

    pub fn editor_git_line_hunks(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitLineHunk>, GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(GitError::TabNotFound)?;

        match GitRepository::file_line_hunks(workspace.directory(), view_state.path()) {
            Ok(hunks) => Ok(hunks),
            Err(GitError::Discover { .. } | GitError::NotWorktree(_)) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }
}

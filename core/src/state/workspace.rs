use std::path::PathBuf;

use crate::tree::{
    Pane, PaneId, SplitAxis, SplitPaneId, Tab, TabId, TabKind, Workspace, WorkspaceId,
};

use super::{PersistenceScope, State};

impl State {
    pub fn open_workspace(&mut self, directory: impl Into<PathBuf>) -> WorkspaceId {
        self.mark_persistent_change();

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
        self.mark_persistent_change_with_scope(PersistenceScope::ActiveWorkspace);
        self.workspaces.activate_workspace(workspace_id)
    }

    pub fn close_workspace(&mut self, workspace_id: Option<WorkspaceId>) -> Option<Workspace> {
        self.mark_persistent_change();

        let closed_workspace = match workspace_id {
            Some(workspace_id) => self.workspaces.close_workspace(workspace_id),
            None => self.workspaces.close_active_workspace(),
        };

        if let Some(workspace) = &closed_workspace {
            self.remove_workspace_file_tree_view_states(workspace.id());
            self.remove_workspace_git_diff_view_states(workspace.id());
            self.remove_workspace_editor_view_states(workspace.id());
            self.remove_workspace_terminal_view_states(workspace.id());
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
        self.mark_persistent_change();

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
        self.mark_persistent_change();

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
        self.mark_persistent_change();

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
        self.mark_persistent_change();

        if matches!(kind, TabKind::Diff | TabKind::Editor) {
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
        self.mark_persistent_change();

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
        self.mark_persistent_change();

        if matches!(kind, TabKind::Diff | TabKind::Editor) {
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
            self.remove_editor_view_state(workspace_id, tab_id);
            if !keep_terminal_session {
                self.remove_terminal_view_state(workspace_id, tab_id);
            }
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
        self.mark_persistent_change();

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
        self.mark_persistent_change();

        let workspace_id = self.resolve_workspace_id(workspace_id)?;
        let fallback_pane = self.blank_pane();
        let workspace = self.workspace_mut(workspace_id)?;

        let removed_tab = workspace.close_tab(pane_id, tab_id, fallback_pane);

        if removed_tab.is_some() {
            self.remove_file_tree_view_state(workspace_id, tab_id);
            self.remove_git_diff_view_state(workspace_id, tab_id);
            self.remove_editor_view_state(workspace_id, tab_id);
            self.remove_terminal_view_state(workspace_id, tab_id);
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
        self.mark_persistent_change();

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
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.resize_split(split_id, ratio)
    }
}

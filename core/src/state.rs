use std::path::PathBuf;

use crate::tree::{
    Pane, PaneId, SplitAxis, Tab, TabId, TabKind, Workspace, WorkspaceId, WorkspaceList,
};

#[derive(Debug)]
pub struct State {
    workspaces: WorkspaceList,
    next_workspace_id: u64,
    next_pane_id: u64,
    next_tab_id: u64,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn workspaces(&self) -> &WorkspaceList {
        &self.workspaces
    }

    pub fn open_workspace(&mut self, directory: impl Into<PathBuf>) -> WorkspaceId {
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
        match workspace_id {
            Some(workspace_id) => self.workspaces.close_workspace(workspace_id),
            None => self.workspaces.close_active_workspace(),
        }
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
        let new_pane = self.blank_pane();
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };
        let pane_id = pane_id.unwrap_or_else(|| workspace.active_pane_id());

        workspace.split_pane_with_new_pane_first(pane_id, axis, new_pane, 0.5, new_pane_first)
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

    pub fn open_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: Option<PaneId>,
        title: impl Into<String>,
        kind: TabKind,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let tab = self.next_tab(title, kind);
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

    pub fn close_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
    ) -> Option<Tab> {
        let workspace_id = self.resolve_workspace_id(workspace_id)?;
        let fallback_pane = self.blank_pane();
        let workspace = self.workspace_mut(workspace_id)?;

        workspace.close_tab(pane_id, tab_id, fallback_pane)
    }

    pub fn reorder_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
        target_index: usize,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.reorder_tab_in_pane(pane_id, tab_id, target_index)
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

    fn next_tab_id(&mut self) -> TabId {
        let tab_id = TabId::new(self.next_tab_id);
        self.next_tab_id += 1;
        tab_id
    }

    fn blank_pane(&mut self) -> Pane {
        let pane_id = self.next_pane_id();
        let tab = self.next_tab("Blank", TabKind::Blank);

        Pane::new(pane_id, tab)
    }

    fn next_tab(&mut self, title: impl Into<String>, kind: TabKind) -> Tab {
        Tab::new(self.next_tab_id(), title, kind)
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            workspaces: WorkspaceList::new(),
            next_workspace_id: 1,
            next_pane_id: 1,
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
    fn opening_tab_adds_it_to_active_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(state.open_tab(None, None, "Search", TabKind::Search));

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
}

use std::path::PathBuf;

use crate::tree::{
    Pane, PaneId, SplitAxis, SplitPaneId, Tab, TabId, TabKind, Workspace, WorkspaceId,
    WorkspaceList,
};

#[derive(Debug)]
pub struct State {
    workspaces: WorkspaceList,
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

        Self::from_workspace_list(workspace_list)
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

    pub fn set_tab_kind(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
        kind: TabKind,
    ) -> bool {
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.set_tab_kind(pane_id, tab_id, kind)
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
            (source_pane.tabs().len() == 1).then(|| self.next_tab("Blank", TabKind::Blank));
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
        let tab = self.next_tab("Blank", TabKind::Blank);

        Pane::new(pane_id, tab)
    }

    fn next_tab(&mut self, title: impl Into<String>, kind: TabKind) -> Tab {
        Tab::new(self.next_tab_id(), title, kind)
    }

    fn from_workspace_list(workspaces: WorkspaceList) -> Option<Self> {
        let next_ids = NextIds::from_workspaces(&workspaces)?;

        Some(Self {
            workspaces,
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

impl Default for State {
    fn default() -> Self {
        Self {
            workspaces: WorkspaceList::new(),
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
    fn splitting_tab_moves_it_to_a_new_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        state.open_tab(None, None, "Search", TabKind::Search);

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
}

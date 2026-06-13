use std::path::{Path, PathBuf};

use super::panes::{Pane, PaneId, PaneNode, SplitAxis};
use super::tabs::{Tab, TabId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorkspaceId(u64);

impl WorkspaceId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    id: WorkspaceId,
    name: String,
    directory: PathBuf,
    root: PaneNode,
    active_pane: PaneId,
}

impl Workspace {
    pub fn new(id: WorkspaceId, directory: impl Into<PathBuf>, initial_pane: Pane) -> Self {
        let active_pane = initial_pane.id();
        let directory = directory.into();
        let name = workspace_name(&directory);

        Self {
            id,
            name,
            directory,
            root: PaneNode::leaf(initial_pane),
            active_pane,
        }
    }

    pub fn from_root(
        id: WorkspaceId,
        directory: impl Into<PathBuf>,
        root: PaneNode,
        active_pane: PaneId,
    ) -> Option<Self> {
        root.contains_pane(active_pane).then(|| {
            let directory = directory.into();

            Self {
                id,
                name: workspace_name(&directory),
                directory,
                root,
                active_pane,
            }
        })
    }

    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn rename(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn root(&self) -> &PaneNode {
        &self.root
    }

    pub fn root_mut(&mut self) -> &mut PaneNode {
        &mut self.root
    }

    pub fn active_pane_id(&self) -> PaneId {
        self.active_pane
    }

    pub fn active_pane(&self) -> Option<&Pane> {
        self.root.find_pane(self.active_pane)
    }

    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.root.find_pane_mut(self.active_pane)
    }

    pub fn activate_pane(&mut self, pane_id: PaneId) -> bool {
        if self.root.contains_pane(pane_id) {
            self.active_pane = pane_id;
            true
        } else {
            false
        }
    }

    pub fn split_active_pane(&mut self, axis: SplitAxis, new_pane: Pane, ratio: f32) -> bool {
        let new_pane_id = new_pane.id();

        if self
            .root
            .split_pane(self.active_pane, axis, new_pane, ratio)
        {
            self.active_pane = new_pane_id;
            true
        } else {
            false
        }
    }

    pub fn add_tab_to_active_pane(&mut self, tab: Tab) -> bool {
        if let Some(pane) = self.active_pane_mut() {
            pane.add_tab(tab);
            true
        } else {
            false
        }
    }

    pub fn add_tab_to_pane(&mut self, pane_id: PaneId, tab: Tab) -> bool {
        if let Some(pane) = self.root.find_pane_mut(pane_id) {
            pane.add_tab(tab);
            true
        } else {
            false
        }
    }

    pub fn activate_tab_in_active_pane(&mut self, tab_id: TabId) -> bool {
        self.active_pane_mut()
            .is_some_and(|pane| pane.activate_tab(tab_id))
    }

    pub fn activate_tab(&mut self, pane_id: PaneId, tab_id: TabId) -> bool {
        if let Some(pane) = self.root.find_pane_mut(pane_id) {
            self.active_pane = pane_id;
            pane.activate_tab(tab_id)
        } else {
            false
        }
    }

    pub fn close_tab(
        &mut self,
        pane_id: PaneId,
        tab_id: TabId,
        fallback_pane: Pane,
    ) -> Option<Tab> {
        let pane = self.root.find_pane_mut(pane_id)?;
        let removed_tab = pane.remove_tab(tab_id)?;

        if !pane.is_empty() {
            return Some(removed_tab);
        }

        self.remove_empty_pane(pane_id, fallback_pane);

        Some(removed_tab)
    }

    fn remove_empty_pane(&mut self, pane_id: PaneId, fallback_pane: Pane) {
        let fallback_pane_id = fallback_pane.id();
        let old_root = std::mem::replace(&mut self.root, PaneNode::leaf(fallback_pane));
        let (root, removed_pane) = old_root.remove_pane(pane_id);

        debug_assert!(removed_pane.is_some());

        if let Some(root) = root {
            self.active_pane = root.first_pane_id();
            self.root = root;
        } else {
            self.active_pane = fallback_pane_id;
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceList {
    workspaces: Vec<Workspace>,
    active_workspace: Option<WorkspaceId>,
}

impl WorkspaceList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    pub fn active_workspace_id(&self) -> Option<WorkspaceId> {
        self.active_workspace
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        let active_workspace = self.active_workspace?;

        self.workspace(active_workspace)
    }

    pub fn active_workspace_mut(&mut self) -> Option<&mut Workspace> {
        let active_workspace = self.active_workspace?;

        self.workspace_mut(active_workspace)
    }

    pub fn workspace(&self, workspace_id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.id() == workspace_id)
    }

    pub fn workspace_mut(&mut self, workspace_id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces
            .iter_mut()
            .find(|workspace| workspace.id() == workspace_id)
    }

    pub fn add_workspace(&mut self, workspace: Workspace) -> bool {
        let workspace_id = workspace.id();

        if self.workspace(workspace_id).is_some() {
            return false;
        }

        self.workspaces.push(workspace);
        self.active_workspace = Some(workspace_id);

        true
    }

    pub fn activate_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        if self.workspace(workspace_id).is_none() {
            return false;
        }

        self.active_workspace = Some(workspace_id);

        true
    }

    pub fn close_active_workspace(&mut self) -> Option<Workspace> {
        let active_workspace = self.active_workspace?;

        self.close_workspace(active_workspace)
    }

    pub fn close_workspace(&mut self, workspace_id: WorkspaceId) -> Option<Workspace> {
        let workspace_index = self
            .workspaces
            .iter()
            .position(|workspace| workspace.id() == workspace_id)?;
        let removed_workspace = self.workspaces.remove(workspace_index);

        if self.active_workspace == Some(workspace_id) {
            self.active_workspace = next_workspace_id(&self.workspaces, workspace_index);
        }

        Some(removed_workspace)
    }
}

fn workspace_name(directory: &Path) -> String {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| directory.display().to_string())
}

fn next_workspace_id(
    workspaces: &[Workspace],
    closed_workspace_index: usize,
) -> Option<WorkspaceId> {
    workspaces
        .get(closed_workspace_index)
        .or_else(|| {
            closed_workspace_index
                .checked_sub(1)
                .and_then(|previous_index| workspaces.get(previous_index))
        })
        .map(Workspace::id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::TabKind;
    use std::path::Path;

    fn pane(pane_id: u64, tab_id: u64) -> Pane {
        Pane::new(
            PaneId::new(pane_id),
            Tab::new(TabId::new(tab_id), "Blank", TabKind::Blank),
        )
    }

    fn workspace(workspace_id: u64, pane_id: u64, tab_id: u64) -> Workspace {
        Workspace::new(
            WorkspaceId::new(workspace_id),
            format!("/workspaces/workspace-{workspace_id}"),
            pane(pane_id, tab_id),
        )
    }

    #[test]
    fn workspace_starts_with_initial_pane_active() {
        let workspace = Workspace::new(WorkspaceId::new(1), "/workspaces/main", pane(1, 1));

        assert_eq!(workspace.active_pane_id(), PaneId::new(1));
        assert!(workspace.active_pane().is_some());
    }

    #[test]
    fn workspace_is_set_on_a_directory() {
        let workspace = Workspace::new(WorkspaceId::new(1), "/workspaces/main", pane(1, 1));

        assert_eq!(workspace.name(), "main");
        assert_eq!(workspace.directory(), Path::new("/workspaces/main"));
    }

    #[test]
    fn splitting_active_pane_focuses_new_pane() {
        let mut workspace = Workspace::new(WorkspaceId::new(1), "/workspaces/main", pane(1, 1));

        assert!(workspace.split_active_pane(SplitAxis::Vertical, pane(2, 2), 0.5));

        assert_eq!(workspace.active_pane_id(), PaneId::new(2));
        assert!(workspace.root().contains_pane(PaneId::new(1)));
        assert!(workspace.root().contains_pane(PaneId::new(2)));
    }

    #[test]
    fn closing_last_tab_removes_pane() {
        let mut workspace = Workspace::new(WorkspaceId::new(1), "/workspaces/main", pane(1, 1));
        workspace.split_active_pane(SplitAxis::Vertical, pane(2, 2), 0.5);

        let removed_tab = workspace.close_tab(PaneId::new(2), TabId::new(2), pane(3, 3));

        assert_eq!(removed_tab.map(|tab| tab.id()), Some(TabId::new(2)));
        assert!(!workspace.root().contains_pane(PaneId::new(2)));
        assert!(workspace.root().contains_pane(PaneId::new(1)));
        assert_eq!(workspace.root().pane_count(), 1);
    }

    #[test]
    fn closing_last_workspace_tab_respawns_pane() {
        let mut workspace = Workspace::new(WorkspaceId::new(1), "/workspaces/main", pane(1, 1));

        let removed_tab = workspace.close_tab(PaneId::new(1), TabId::new(1), pane(2, 2));

        assert_eq!(removed_tab.map(|tab| tab.id()), Some(TabId::new(1)));
        assert!(!workspace.root().contains_pane(PaneId::new(1)));
        assert!(workspace.root().contains_pane(PaneId::new(2)));
        assert_eq!(workspace.active_pane_id(), PaneId::new(2));
        assert_eq!(workspace.root().pane_count(), 1);
    }

    #[test]
    fn workspace_list_can_be_empty() {
        let workspaces = WorkspaceList::new();

        assert!(workspaces.is_empty());
        assert_eq!(workspaces.active_workspace_id(), None);
        assert!(workspaces.active_workspace().is_none());
    }

    #[test]
    fn adding_workspace_activates_it() {
        let mut workspaces = WorkspaceList::new();

        assert!(workspaces.add_workspace(workspace(1, 1, 1)));

        assert_eq!(workspaces.active_workspace_id(), Some(WorkspaceId::new(1)));
        assert_eq!(
            workspaces.active_workspace().map(Workspace::id),
            Some(WorkspaceId::new(1))
        );
    }

    #[test]
    fn workspace_list_rejects_duplicate_workspace_ids() {
        let mut workspaces = WorkspaceList::new();

        assert!(workspaces.add_workspace(workspace(1, 1, 1)));
        assert!(!workspaces.add_workspace(workspace(1, 2, 2)));

        assert_eq!(workspaces.workspaces().len(), 1);
    }

    #[test]
    fn closing_active_workspace_activates_neighbor() {
        let mut workspaces = WorkspaceList::new();
        workspaces.add_workspace(workspace(1, 1, 1));
        workspaces.add_workspace(workspace(2, 2, 2));

        let closed_workspace = workspaces.close_active_workspace();

        assert_eq!(
            closed_workspace.map(|workspace| workspace.id()),
            Some(WorkspaceId::new(2))
        );
        assert_eq!(workspaces.active_workspace_id(), Some(WorkspaceId::new(1)));
    }

    #[test]
    fn closing_last_workspace_leaves_workspace_list_empty() {
        let mut workspaces = WorkspaceList::new();
        workspaces.add_workspace(workspace(1, 1, 1));

        let closed_workspace = workspaces.close_workspace(WorkspaceId::new(1));

        assert_eq!(
            closed_workspace.map(|workspace| workspace.id()),
            Some(WorkspaceId::new(1))
        );
        assert!(workspaces.is_empty());
        assert_eq!(workspaces.active_workspace_id(), None);
    }
}

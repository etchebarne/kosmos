use std::path::PathBuf;

use pane_tree::PaneTree;

pub struct Workspace {
    pub id: usize,
    pub path: PathBuf,
    pub name: String,
    pub pane_tree: PaneTree,
}

impl Workspace {
    pub fn initial(&self) -> String {
        self.name
            .chars()
            .next()
            .map_or_else(|| "?".to_string(), |c| c.to_ascii_uppercase().to_string())
    }
}

pub struct WorkspaceManager {
    workspaces: Vec<Workspace>,
    active: Option<usize>,
    previous_active: Option<usize>,
    next_id: usize,
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceManager {
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            active: None,
            previous_active: None,
            next_id: 0,
        }
    }

    pub fn from_parts(workspaces: Vec<Workspace>, active: Option<usize>, next_id: usize) -> Self {
        Self {
            workspaces,
            active,
            previous_active: active,
            next_id,
        }
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn active_id(&self) -> Option<usize> {
        self.active
    }

    pub fn previous_active_id(&self) -> Option<usize> {
        self.previous_active
    }

    pub fn next_id(&self) -> usize {
        self.next_id
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        let id = self.active?;
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn add(&mut self, path: PathBuf) -> usize {
        if let Some(existing) = self.workspaces.iter().find(|w| w.path == path) {
            let id = existing.id;
            self.set_active(Some(id));
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        let name = path
            .file_name()
            .and_then(|os| os.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.display().to_string());
        self.workspaces.push(Workspace {
            id,
            path,
            name,
            pane_tree: PaneTree::new(),
        });
        self.set_active(Some(id));
        id
    }

    fn set_active(&mut self, id: Option<usize>) {
        if self.active != id {
            self.previous_active = self.active;
            self.active = id;
        }
    }

    pub fn active_pane_tree(&self) -> Option<&PaneTree> {
        let id = self.active?;
        self.workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| &w.pane_tree)
    }

    pub fn active_pane_tree_mut(&mut self) -> Option<&mut PaneTree> {
        let id = self.active?;
        self.workspaces
            .iter_mut()
            .find(|w| w.id == id)
            .map(|w| &mut w.pane_tree)
    }

    pub fn close(&mut self, id: usize) -> bool {
        let Some(pos) = self.workspaces.iter().position(|w| w.id == id) else {
            return false;
        };
        self.workspaces.remove(pos);
        if self.active == Some(id) {
            let next = self
                .workspaces
                .get(pos)
                .or_else(|| pos.checked_sub(1).and_then(|p| self.workspaces.get(p)))
                .map(|w| w.id);
            self.set_active(next);
        }
        if self.previous_active == Some(id) {
            self.previous_active = self.active;
        }
        true
    }

    pub fn select(&mut self, id: usize) -> bool {
        if self.active == Some(id) {
            return false;
        }
        if !self.workspaces.iter().any(|w| w.id == id) {
            return false;
        }
        self.set_active(Some(id));
        true
    }

    pub fn reorder_before(&mut self, drag_id: usize, target_id: usize) -> bool {
        if drag_id == target_id {
            return false;
        }
        let Some(from) = self.workspaces.iter().position(|w| w.id == drag_id) else {
            return false;
        };
        let Some(target) = self.workspaces.iter().position(|w| w.id == target_id) else {
            return false;
        };
        let workspace = self.workspaces.remove(from);
        let insert_at = self
            .workspaces
            .iter()
            .position(|w| w.id == target_id)
            .unwrap_or(target);
        if insert_at == from {
            self.workspaces.insert(from, workspace);
            return false;
        }
        self.workspaces.insert(insert_at, workspace);
        true
    }

    pub fn move_to_end(&mut self, drag_id: usize) -> bool {
        let Some(from) = self.workspaces.iter().position(|w| w.id == drag_id) else {
            return false;
        };
        if from + 1 == self.workspaces.len() {
            return false;
        }
        let workspace = self.workspaces.remove(from);
        self.workspaces.push(workspace);
        true
    }
}

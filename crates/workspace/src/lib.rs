use std::path::PathBuf;

use gpui::SharedString;
use pane_tree::PaneTree;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Workspace {
    pub id: usize,
    pub path: PathBuf,
    pub name: SharedString,
    pub pane_tree: PaneTree,
}

impl Workspace {
    pub fn initial(&self) -> SharedString {
        self.name
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string())
            .into()
    }
}

pub struct WorkspaceManager {
    workspaces: Vec<Workspace>,
    active: Option<usize>,
    next_id: usize,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            active: None,
            next_id: 0,
        }
    }

    pub fn from_parts(workspaces: Vec<Workspace>, active: Option<usize>, next_id: usize) -> Self {
        Self {
            workspaces,
            active,
            next_id,
        }
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn active_id(&self) -> Option<usize> {
        self.active
    }

    pub fn next_id(&self) -> usize {
        self.next_id
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        let id = self.active?;
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn add(&mut self, path: PathBuf) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let name: SharedString = path
            .file_name()
            .and_then(|os| os.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        self.workspaces.push(Workspace {
            id,
            path,
            name,
            pane_tree: PaneTree::new(),
        });
        self.active = Some(id);
        id
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

    pub fn select(&mut self, id: usize) -> bool {
        if self.active == Some(id) {
            return false;
        }
        if !self.workspaces.iter().any(|w| w.id == id) {
            return false;
        }
        self.active = Some(id);
        true
    }
}

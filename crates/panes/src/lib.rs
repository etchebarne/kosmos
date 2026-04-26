use serde::{Deserialize, Serialize};
use tabs::Tab;

#[derive(Clone, Serialize, Deserialize)]
pub struct Pane {
    id: usize,
    tabs: Vec<Tab>,
    active_tab: usize,
}

impl Pane {
    pub fn new(id: usize, initial_tab: Tab) -> Self {
        Self {
            id,
            active_tab: initial_tab.id,
            tabs: vec![initial_tab],
        }
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub fn has_tab(&self, tab_id: usize) -> bool {
        self.tabs.iter().any(|t| t.id == tab_id)
    }

    pub fn add_tab(&mut self, tab: Tab) {
        self.active_tab = tab.id;
        self.tabs.push(tab);
    }

    pub fn insert_tab_before(&mut self, tab: Tab, before_tab_id: usize) -> bool {
        let Some(index) = self.tabs.iter().position(|t| t.id == before_tab_id) else {
            return false;
        };
        self.active_tab = tab.id;
        self.tabs.insert(index, tab);
        true
    }

    pub fn select_tab(&mut self, tab_id: usize) -> bool {
        if !self.has_tab(tab_id) {
            return false;
        }
        self.active_tab = tab_id;
        true
    }

    pub fn take_tab(&mut self, tab_id: usize) -> Option<Tab> {
        let index = self.tabs.iter().position(|t| t.id == tab_id)?;
        let tab = self.tabs.remove(index);
        if self.active_tab == tab_id && !self.tabs.is_empty() {
            let next = index.saturating_sub(1).min(self.tabs.len() - 1);
            self.active_tab = self.tabs[next].id;
        }
        Some(tab)
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

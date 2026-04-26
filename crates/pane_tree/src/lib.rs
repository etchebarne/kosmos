#[cfg(test)]
mod tests;

use panes::Pane;
use serde::{Deserialize, Serialize};
use tabs::{Tab, TabKind, registry};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    Row,
    Column,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum PaneNode {
    Leaf(Pane),
    Split {
        id: usize,
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

#[derive(Serialize, Deserialize)]
pub struct PaneTree {
    root: PaneNode,
    next_tab_id: usize,
    next_pane_id: usize,
    next_split_id: usize,
}

impl Default for PaneTree {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneTree {
    pub fn new() -> Self {
        Self {
            root: PaneNode::Leaf(Pane::new(0, Tab::new(0, &registry::BLANK))),
            next_tab_id: 1,
            next_pane_id: 1,
            next_split_id: 1,
        }
    }

    pub fn root(&self) -> &PaneNode {
        &self.root
    }

    pub fn total_tabs(&self) -> usize {
        Self::total_tabs_in(&self.root)
    }

    pub fn add_tab(&mut self, pane_id: usize, kind: &'static TabKind) -> bool {
        let id = self.next_tab_id;
        let Some(pane) = Self::find_pane_mut(&mut self.root, pane_id) else {
            return false;
        };
        pane.add_tab(Tab::new(id, kind));
        self.next_tab_id += 1;
        true
    }

    pub fn replace_tab_kind(
        &mut self,
        pane_id: usize,
        tab_id: usize,
        kind: &'static TabKind,
    ) -> bool {
        let Some(pane) = Self::find_pane_mut(&mut self.root, pane_id) else {
            return false;
        };
        pane.replace_tab(tab_id, Tab::new(tab_id, kind))
    }

    pub fn select_tab(&mut self, pane_id: usize, tab_id: usize) -> bool {
        let Some(pane) = Self::find_pane_mut(&mut self.root, pane_id) else {
            return false;
        };
        pane.select_tab(tab_id)
    }

    pub fn close_tab(&mut self, pane_id: usize, tab_id: usize) -> bool {
        if Self::total_tabs_in(&self.root) == 1 {
            return false;
        }

        let Some(pane) = Self::find_pane_mut(&mut self.root, pane_id) else {
            return false;
        };
        if pane.take_tab(tab_id).is_none() {
            return false;
        }

        Self::collapse_empty_panes(&mut self.root);
        true
    }

    pub fn move_tab_before(
        &mut self,
        source_pane_id: usize,
        tab_id: usize,
        target_pane_id: usize,
        target_tab_id: usize,
    ) -> bool {
        if source_pane_id == target_pane_id && tab_id == target_tab_id {
            return false;
        }
        if !Self::pane_has_tab(&self.root, source_pane_id, tab_id) {
            return false;
        }
        if !Self::pane_has_tab(&self.root, target_pane_id, target_tab_id) {
            return false;
        }

        let source = Self::find_pane_mut(&mut self.root, source_pane_id).unwrap();
        let tab = source.take_tab(tab_id).unwrap();
        let target = Self::find_pane_mut(&mut self.root, target_pane_id).unwrap();
        target.insert_tab_before(tab, target_tab_id);
        Self::collapse_empty_panes(&mut self.root);
        true
    }

    pub fn move_tab_to_pane(
        &mut self,
        source_pane_id: usize,
        tab_id: usize,
        target_pane_id: usize,
    ) -> bool {
        if source_pane_id == target_pane_id {
            return false;
        }
        if !Self::pane_has_tab(&self.root, source_pane_id, tab_id) {
            return false;
        }
        if Self::find_pane(&self.root, target_pane_id).is_none() {
            return false;
        }

        let source = Self::find_pane_mut(&mut self.root, source_pane_id).unwrap();
        let tab = source.take_tab(tab_id).unwrap();
        let target = Self::find_pane_mut(&mut self.root, target_pane_id).unwrap();
        target.add_tab(tab);
        Self::collapse_empty_panes(&mut self.root);
        true
    }

    pub fn split_pane(
        &mut self,
        source_pane_id: usize,
        tab_id: usize,
        target_pane_id: usize,
        drop_zone: DropZone,
    ) -> bool {
        if drop_zone == DropZone::Center || Self::total_tabs_in(&self.root) == 1 {
            return false;
        }
        if !Self::pane_has_tab(&self.root, source_pane_id, tab_id) {
            return false;
        }
        if Self::find_pane(&self.root, target_pane_id).is_none() {
            return false;
        }
        if source_pane_id == target_pane_id {
            let source_pane = Self::find_pane(&self.root, source_pane_id).unwrap();
            if source_pane.tabs().len() == 1 {
                return false;
            }
        }

        let new_pane_id = self.next_pane_id;
        let new_split_id = self.next_split_id;

        let source = Self::find_pane_mut(&mut self.root, source_pane_id).unwrap();
        let tab = source.take_tab(tab_id).unwrap();
        Self::split_leaf_with_tab(
            &mut self.root,
            target_pane_id,
            tab,
            new_pane_id,
            new_split_id,
            drop_zone,
        );

        self.next_pane_id += 1;
        self.next_split_id += 1;
        Self::collapse_empty_panes(&mut self.root);
        true
    }

    pub fn resize_split(&mut self, split_id: usize, ratio: f32) -> bool {
        let Some(split_ratio) = Self::find_split_ratio_mut(&mut self.root, split_id) else {
            return false;
        };
        *split_ratio = ratio.clamp(0.15, 0.85);
        true
    }

    fn pane_has_tab(node: &PaneNode, pane_id: usize, tab_id: usize) -> bool {
        Self::find_pane(node, pane_id).is_some_and(|p| p.has_tab(tab_id))
    }

    fn find_pane(node: &PaneNode, pane_id: usize) -> Option<&Pane> {
        match node {
            PaneNode::Leaf(pane) if pane.id() == pane_id => Some(pane),
            PaneNode::Leaf(_) => None,
            PaneNode::Split { first, second, .. } => {
                Self::find_pane(first, pane_id).or_else(|| Self::find_pane(second, pane_id))
            }
        }
    }

    fn find_pane_mut(node: &mut PaneNode, pane_id: usize) -> Option<&mut Pane> {
        match node {
            PaneNode::Leaf(pane) if pane.id() == pane_id => Some(pane),
            PaneNode::Leaf(_) => None,
            PaneNode::Split { first, second, .. } => {
                Self::find_pane_mut(first, pane_id).or_else(|| Self::find_pane_mut(second, pane_id))
            }
        }
    }

    fn total_tabs_in(node: &PaneNode) -> usize {
        match node {
            PaneNode::Leaf(pane) => pane.tabs().len(),
            PaneNode::Split { first, second, .. } => {
                Self::total_tabs_in(first) + Self::total_tabs_in(second)
            }
        }
    }

    fn find_split_ratio_mut(node: &mut PaneNode, split_id: usize) -> Option<&mut f32> {
        match node {
            PaneNode::Leaf(_) => None,
            PaneNode::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == split_id {
                    Some(ratio)
                } else {
                    Self::find_split_ratio_mut(first, split_id)
                        .or_else(|| Self::find_split_ratio_mut(second, split_id))
                }
            }
        }
    }

    fn split_leaf_with_tab(
        node: &mut PaneNode,
        pane_id: usize,
        tab: Tab,
        new_pane_id: usize,
        new_split_id: usize,
        drop_zone: DropZone,
    ) -> bool {
        match node {
            PaneNode::Leaf(pane) if pane.id() == pane_id => {
                let axis = match drop_zone {
                    DropZone::Left | DropZone::Right => SplitAxis::Row,
                    DropZone::Top | DropZone::Bottom => SplitAxis::Column,
                    DropZone::Center => return false,
                };
                let new_pane = PaneNode::Leaf(Pane::new(new_pane_id, tab));
                let existing_pane = PaneNode::Leaf(pane.clone());

                let (first, second) = match drop_zone {
                    DropZone::Left | DropZone::Top => (new_pane, existing_pane),
                    DropZone::Right | DropZone::Bottom => (existing_pane, new_pane),
                    DropZone::Center => return false,
                };

                *node = PaneNode::Split {
                    id: new_split_id,
                    axis,
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            PaneNode::Leaf(_) => false,
            PaneNode::Split { first, second, .. } => {
                Self::split_leaf_with_tab(
                    first,
                    pane_id,
                    tab.clone(),
                    new_pane_id,
                    new_split_id,
                    drop_zone,
                ) || Self::split_leaf_with_tab(
                    second,
                    pane_id,
                    tab,
                    new_pane_id,
                    new_split_id,
                    drop_zone,
                )
            }
        }
    }

    fn collapse_empty_panes(node: &mut PaneNode) -> bool {
        let replacement = match node {
            PaneNode::Leaf(pane) => return pane.is_empty(),
            PaneNode::Split { first, second, .. } => {
                let first_empty = Self::collapse_empty_panes(first);
                let second_empty = Self::collapse_empty_panes(second);

                match (first_empty, second_empty) {
                    (true, true) => return true,
                    (true, false) => Some((**second).clone()),
                    (false, true) => Some((**first).clone()),
                    (false, false) => None,
                }
            }
        };

        if let Some(replacement) = replacement {
            *node = replacement;
        }

        false
    }
}

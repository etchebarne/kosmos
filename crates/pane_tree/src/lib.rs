#[cfg(test)]
mod tests;

use panes::Pane;
use tabs::{Tab, TabKind, registry};

#[derive(Clone, Copy, PartialEq, Eq)]
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

#[derive(Clone)]
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

pub struct PaneTree {
    root: PaneNode,
    next_tab_id: usize,
    next_pane_id: usize,
    next_split_id: usize,
    active_pane_id: usize,
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
            active_pane_id: 0,
        }
    }

    pub fn from_parts(
        root: PaneNode,
        next_tab_id: usize,
        next_pane_id: usize,
        next_split_id: usize,
    ) -> Self {
        let active_pane_id = Self::first_pane_id(&root).unwrap_or(0);
        Self {
            root,
            next_tab_id,
            next_pane_id,
            next_split_id,
            active_pane_id,
        }
    }

    pub fn root(&self) -> &PaneNode {
        &self.root
    }

    pub fn next_tab_id(&self) -> usize {
        self.next_tab_id
    }

    pub fn next_pane_id(&self) -> usize {
        self.next_pane_id
    }

    pub fn next_split_id(&self) -> usize {
        self.next_split_id
    }

    pub fn active_pane_id(&self) -> usize {
        self.active_pane_id
    }

    pub fn active_pane(&self) -> Option<&Pane> {
        Self::find_pane(&self.root, self.active_pane_id)
    }

    pub fn pane(&self, pane_id: usize) -> Option<&Pane> {
        Self::find_pane(&self.root, pane_id)
    }

    pub fn find_tab(&self, mut predicate: impl FnMut(&Tab) -> bool) -> Option<(usize, usize)> {
        Self::find_tab_in(&self.root, &mut predicate).map(|(pane_id, tab)| (pane_id, tab.id))
    }

    pub fn total_tabs(&self) -> usize {
        Self::total_tabs_in(&self.root)
    }

    pub fn add_tab(&mut self, pane_id: usize, kind: &'static TabKind) -> bool {
        self.append_new_tab(pane_id, |id| Tab::new(id, kind))
            .is_some()
    }

    /// Pane id with the largest rendered area, weighted by accumulated split
    /// ratios. Ties go to the first pane encountered in DFS order.
    pub fn biggest_pane_id(&self) -> usize {
        Self::biggest_pane_in(&self.root, 1.0)
            .map(|(id, _)| id)
            .unwrap_or(self.active_pane_id)
    }

    pub fn append_new_tab(
        &mut self,
        pane_id: usize,
        build_tab: impl FnOnce(usize) -> Tab,
    ) -> Option<(usize, usize)> {
        let tab_id = self.next_tab_id;
        let pane = Self::find_pane_mut(&mut self.root, pane_id)?;
        pane.add_tab(build_tab(tab_id));
        self.next_tab_id += 1;
        self.active_pane_id = pane_id;
        Some((pane_id, pane.tabs().len()))
    }

    pub fn insert_new_tab_before(
        &mut self,
        pane_id: usize,
        target_tab_id: usize,
        build_tab: impl FnOnce(usize) -> Tab,
    ) -> Option<(usize, usize)> {
        let pane = Self::find_pane_mut(&mut self.root, pane_id)?;
        if !pane.has_tab(target_tab_id) {
            return None;
        }
        let tab_id = self.next_tab_id;
        if !pane.insert_tab_before(build_tab(tab_id), target_tab_id) {
            return None;
        }
        self.next_tab_id += 1;
        self.active_pane_id = pane_id;
        Some((pane_id, pane.tabs().len()))
    }

    pub fn split_pane_with_new_tab(
        &mut self,
        target_pane_id: usize,
        drop_zone: DropZone,
        build_tab: impl FnOnce(usize) -> Tab,
    ) -> Option<(usize, usize)> {
        if drop_zone == DropZone::Center {
            return None;
        }
        if Self::find_pane(&self.root, target_pane_id).is_none() {
            return None;
        }

        let new_pane_id = self.next_pane_id;
        let new_split_id = self.next_split_id;
        let tab_id = self.next_tab_id;
        let tab = build_tab(tab_id);

        if !Self::split_leaf_with_tab(
            &mut self.root,
            target_pane_id,
            tab,
            new_pane_id,
            new_split_id,
            drop_zone,
        ) {
            return None;
        }

        self.next_tab_id += 1;
        self.next_pane_id += 1;
        self.next_split_id += 1;
        self.active_pane_id = new_pane_id;
        Some((new_pane_id, 1))
    }

    pub fn add_tab_to_active(&mut self, kind: &'static TabKind) -> bool {
        self.add_tab(self.active_pane_id, kind)
    }

    pub fn focus_pane(&mut self, pane_id: usize) -> bool {
        if Self::find_pane(&self.root, pane_id).is_none() {
            return false;
        }
        if self.active_pane_id == pane_id {
            return false;
        }
        self.active_pane_id = pane_id;
        true
    }

    pub fn close_active_tab(&mut self) -> bool {
        let Some(pane) = self.active_pane() else {
            return false;
        };
        let tab_id = pane.active_tab();
        self.close_tab(self.active_pane_id, tab_id)
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
        if !pane.replace_tab(tab_id, Tab::new(tab_id, kind)) {
            return false;
        }
        self.active_pane_id = pane_id;
        true
    }

    pub fn select_tab(&mut self, pane_id: usize, tab_id: usize) -> bool {
        let Some(pane) = Self::find_pane_mut(&mut self.root, pane_id) else {
            return false;
        };
        if !pane.select_tab(tab_id) {
            return false;
        }
        self.active_pane_id = pane_id;
        true
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
        self.refresh_active_pane(pane_id);
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
        self.active_pane_id = target_pane_id;
        self.refresh_active_pane(target_pane_id);
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
        self.active_pane_id = target_pane_id;
        self.refresh_active_pane(target_pane_id);
        true
    }

    pub fn move_tab_to_end(
        &mut self,
        source_pane_id: usize,
        tab_id: usize,
        target_pane_id: usize,
    ) -> bool {
        if !Self::pane_has_tab(&self.root, source_pane_id, tab_id) {
            return false;
        }
        if Self::find_pane(&self.root, target_pane_id).is_none() {
            return false;
        }
        if source_pane_id == target_pane_id {
            let target_pane = Self::find_pane(&self.root, target_pane_id).unwrap();
            if target_pane.tabs().last().map(|t| t.id) == Some(tab_id) {
                return false;
            }
        }

        let source = Self::find_pane_mut(&mut self.root, source_pane_id).unwrap();
        let tab = source.take_tab(tab_id).unwrap();
        let target = Self::find_pane_mut(&mut self.root, target_pane_id).unwrap();
        target.add_tab(tab);
        Self::collapse_empty_panes(&mut self.root);
        self.active_pane_id = target_pane_id;
        self.refresh_active_pane(target_pane_id);
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
        self.active_pane_id = new_pane_id;
        self.refresh_active_pane(new_pane_id);
        true
    }

    pub fn resize_split(&mut self, split_id: usize, ratio: f32) -> bool {
        let Some(split_ratio) = Self::find_split_ratio_mut(&mut self.root, split_id) else {
            return false;
        };
        *split_ratio = ratio.clamp(0.15, 0.85);
        true
    }

    fn refresh_active_pane(&mut self, preferred: usize) {
        if Self::find_pane(&self.root, preferred).is_some() {
            self.active_pane_id = preferred;
            return;
        }
        if Self::find_pane(&self.root, self.active_pane_id).is_some() {
            return;
        }
        if let Some(fallback) = Self::first_pane_id(&self.root) {
            self.active_pane_id = fallback;
        }
    }

    fn first_pane_id(node: &PaneNode) -> Option<usize> {
        match node {
            PaneNode::Leaf(pane) => Some(pane.id()),
            PaneNode::Split { first, second, .. } => {
                Self::first_pane_id(first).or_else(|| Self::first_pane_id(second))
            }
        }
    }

    fn pane_has_tab(node: &PaneNode, pane_id: usize, tab_id: usize) -> bool {
        Self::find_pane(node, pane_id).is_some_and(|p| p.has_tab(tab_id))
    }

    fn biggest_pane_in(node: &PaneNode, weight: f32) -> Option<(usize, f32)> {
        match node {
            PaneNode::Leaf(pane) => Some((pane.id(), weight)),
            PaneNode::Split {
                ratio,
                first,
                second,
                ..
            } => {
                let a = Self::biggest_pane_in(first, weight * ratio);
                let b = Self::biggest_pane_in(second, weight * (1.0 - ratio));
                match (a, b) {
                    (Some(a), Some(b)) => Some(if a.1 >= b.1 { a } else { b }),
                    (a, b) => a.or(b),
                }
            }
        }
    }

    fn find_tab_in<'a>(
        node: &'a PaneNode,
        predicate: &mut impl FnMut(&Tab) -> bool,
    ) -> Option<(usize, &'a Tab)> {
        match node {
            PaneNode::Leaf(pane) => pane
                .tabs()
                .iter()
                .find(|tab| predicate(tab))
                .map(|tab| (pane.id(), tab)),
            PaneNode::Split { first, second, .. } => {
                Self::find_tab_in(first, predicate).or_else(|| Self::find_tab_in(second, predicate))
            }
        }
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

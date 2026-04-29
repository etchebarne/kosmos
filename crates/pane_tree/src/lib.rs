pub mod actions;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use gpui::{Context, InteractiveElement};
use panes::Pane;
use tabs::{Tab, TabKind, registry};

pub use actions::{CloseTab, NewTab};

/// Implemented by the entity that owns a [`PaneTree`] so action handlers can
/// reach it. Lives next to the actions so a feature crate is fully responsible
/// for its own keyboard surface — the binary just provides a tree.
pub trait PaneTreeContext: Sized + 'static {
    fn with_active_tree(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut PaneTree) -> bool);

    fn on_tab_appended(&mut self, _pane_id: usize, _new_tab_count: usize, _cx: &mut Context<Self>) {
    }
}

/// Extension trait: chain `.wire_pane_tree_actions(cx)` onto a focusable element
/// to register the pane-tree action handlers in one line.
pub trait WirePaneTreeActions: Sized {
    fn wire_pane_tree_actions<T: PaneTreeContext>(self, cx: &mut Context<T>) -> Self;
}

impl<E: InteractiveElement + 'static> WirePaneTreeActions for E {
    fn wire_pane_tree_actions<T: PaneTreeContext>(self, cx: &mut Context<T>) -> Self {
        self.on_action(cx.listener(|this, _: &CloseTab, _, cx| {
            this.with_active_tree(cx, |tree| tree.close_active_tab());
        }))
        .on_action(cx.listener(|this, _: &NewTab, _, cx| {
            let mut appended: Option<(usize, usize)> = None;
            this.with_active_tree(cx, |tree| {
                if !tree.add_tab_to_active(&registry::BLANK) {
                    return false;
                }
                let pane_id = tree.active_pane_id();
                let count = tree.active_pane().map(|p| p.tabs().len()).unwrap_or(0);
                appended = Some((pane_id, count));
                true
            });
            if let Some((pane_id, count)) = appended {
                this.on_tab_appended(pane_id, count, cx);
            }
        }))
    }
}

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
        self.active_pane_id = pane_id;
        true
    }

    /// Pane id with the largest rendered area, weighted by accumulated split
    /// ratios. Ties go to the first pane encountered in DFS order.
    pub fn biggest_pane_id(&self) -> usize {
        Self::biggest_pane_in(&self.root, 1.0)
            .map(|(id, _)| id)
            .unwrap_or(self.active_pane_id)
    }

    /// Open `path` in a file_editor tab. If a file_editor tab already exists
    /// for this path anywhere in the tree, focus it; otherwise add a new tab
    /// to the biggest pane. Returns `(pane_id, tab_count)` so the caller can
    /// scroll the tab strip into view.
    pub fn open_file_editor(&mut self, path: PathBuf) -> Option<(usize, usize)> {
        if let Some((pane_id, tab_id)) = Self::find_file_editor_tab(&self.root, &path) {
            self.select_tab(pane_id, tab_id);
            let count = Self::find_pane(&self.root, pane_id)
                .map(|p| p.tabs().len())
                .unwrap_or(0);
            return Some((pane_id, count));
        }

        let pane_id = self.biggest_pane_id();
        let tab_id = self.next_tab_id;
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let tab = Tab::new(tab_id, &registry::FILE_EDITOR)
            .with_title(title)
            .with_path(path);

        let pane = Self::find_pane_mut(&mut self.root, pane_id)?;
        pane.add_tab(tab);
        self.next_tab_id += 1;
        self.active_pane_id = pane_id;
        let count = pane.tabs().len();
        Some((pane_id, count))
    }

    /// Open `path` in a file_editor tab inside `target_pane_id`. If that pane
    /// already has a file_editor for this path, focus it; otherwise append a
    /// new tab there. Returns `(pane_id, tab_count)` so the caller can scroll
    /// the tab strip.
    pub fn open_file_in_pane(
        &mut self,
        path: PathBuf,
        target_pane_id: usize,
    ) -> Option<(usize, usize)> {
        let pane = Self::find_pane(&self.root, target_pane_id)?;
        let existing = pane.tabs().iter().find(|t| {
            t.kind.as_ref() == registry::FILE_EDITOR.id && t.path.as_deref() == Some(&path)
        });
        if let Some(tab) = existing {
            let tab_id = tab.id;
            self.select_tab(target_pane_id, tab_id);
            let count = Self::find_pane(&self.root, target_pane_id)
                .map(|p| p.tabs().len())
                .unwrap_or(0);
            return Some((target_pane_id, count));
        }

        let tab_id = self.next_tab_id;
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let tab = Tab::new(tab_id, &registry::FILE_EDITOR)
            .with_title(title)
            .with_path(path);

        let pane = Self::find_pane_mut(&mut self.root, target_pane_id)?;
        pane.add_tab(tab);
        self.next_tab_id += 1;
        self.active_pane_id = target_pane_id;
        let count = pane.tabs().len();
        Some((target_pane_id, count))
    }

    /// Insert a file_editor tab for `path` immediately before `target_tab_id`
    /// in `target_pane_id`. If that pane already has a file_editor tab for the
    /// path, focus it instead.
    pub fn open_file_before(
        &mut self,
        path: PathBuf,
        target_pane_id: usize,
        target_tab_id: usize,
    ) -> Option<(usize, usize)> {
        let pane = Self::find_pane(&self.root, target_pane_id)?;
        if !pane.has_tab(target_tab_id) {
            return None;
        }
        let existing = pane.tabs().iter().find(|t| {
            t.kind.as_ref() == registry::FILE_EDITOR.id && t.path.as_deref() == Some(&path)
        });
        if let Some(tab) = existing {
            let tab_id = tab.id;
            self.select_tab(target_pane_id, tab_id);
            let count = Self::find_pane(&self.root, target_pane_id)
                .map(|p| p.tabs().len())
                .unwrap_or(0);
            return Some((target_pane_id, count));
        }

        let tab_id = self.next_tab_id;
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let tab = Tab::new(tab_id, &registry::FILE_EDITOR)
            .with_title(title)
            .with_path(path);

        let pane = Self::find_pane_mut(&mut self.root, target_pane_id)?;
        if !pane.insert_tab_before(tab, target_tab_id) {
            return None;
        }
        self.next_tab_id += 1;
        self.active_pane_id = target_pane_id;
        let count = pane.tabs().len();
        Some((target_pane_id, count))
    }

    /// Split `target_pane_id` along `drop_zone` and seed the new pane with a
    /// file_editor tab for `path`. Returns `(new_pane_id, tab_count)`.
    pub fn split_pane_with_file(
        &mut self,
        path: PathBuf,
        target_pane_id: usize,
        drop_zone: DropZone,
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
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let tab = Tab::new(tab_id, &registry::FILE_EDITOR)
            .with_title(title)
            .with_path(path);

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

    fn find_file_editor_tab(node: &PaneNode, path: &Path) -> Option<(usize, usize)> {
        match node {
            PaneNode::Leaf(pane) => pane
                .tabs()
                .iter()
                .find(|t| {
                    t.kind.as_ref() == registry::FILE_EDITOR.id && t.path.as_deref() == Some(path)
                })
                .map(|t| (pane.id(), t.id)),
            PaneNode::Split { first, second, .. } => Self::find_file_editor_tab(first, path)
                .or_else(|| Self::find_file_editor_tab(second, path)),
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

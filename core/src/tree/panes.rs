use super::tabs::{Tab, TabId, TabKind};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaneId(u64);

impl PaneId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SplitPaneId(u64);

impl SplitPaneId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaneNode {
    Leaf(Pane),
    Split(SplitPane),
}

impl PaneNode {
    pub fn leaf(pane: Pane) -> Self {
        Self::Leaf(pane)
    }

    pub fn split(
        id: SplitPaneId,
        axis: SplitAxis,
        ratio: f32,
        first: PaneNode,
        second: PaneNode,
    ) -> Self {
        Self::Split(SplitPane::new(id, axis, ratio, first, second))
    }

    pub fn find_pane(&self, pane_id: PaneId) -> Option<&Pane> {
        match self {
            Self::Leaf(pane) if pane.id == pane_id => Some(pane),
            Self::Leaf(_) => None,
            Self::Split(split) => split
                .first()
                .find_pane(pane_id)
                .or_else(|| split.second().find_pane(pane_id)),
        }
    }

    pub fn find_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        match self {
            Self::Leaf(pane) if pane.id == pane_id => Some(pane),
            Self::Leaf(_) => None,
            Self::Split(split) => {
                if split.first().contains_pane(pane_id) {
                    split.first_mut().find_pane_mut(pane_id)
                } else {
                    split.second_mut().find_pane_mut(pane_id)
                }
            }
        }
    }

    pub fn contains_pane(&self, pane_id: PaneId) -> bool {
        self.find_pane(pane_id).is_some()
    }

    pub fn first_pane_id(&self) -> PaneId {
        match self {
            Self::Leaf(pane) => pane.id(),
            Self::Split(split) => split.first().first_pane_id(),
        }
    }

    pub fn pane_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split(split) => split.first().pane_count() + split.second().pane_count(),
        }
    }

    pub fn largest_pane_id(&self) -> PaneId {
        self.largest_pane(1.0).0
    }

    pub fn split_pane(
        &mut self,
        split_id: SplitPaneId,
        pane_id: PaneId,
        axis: SplitAxis,
        new_pane: Pane,
        ratio: f32,
    ) -> bool {
        self.split_pane_with_new_pane_first(split_id, pane_id, axis, new_pane, ratio, false)
    }

    pub fn split_pane_with_new_pane_first(
        &mut self,
        split_id: SplitPaneId,
        pane_id: PaneId,
        axis: SplitAxis,
        new_pane: Pane,
        ratio: f32,
        new_pane_first: bool,
    ) -> bool {
        validate_split_ratio(ratio);

        match self {
            Self::Leaf(pane) if pane.id == pane_id => {
                let existing_pane = Pane {
                    id: pane.id,
                    tabs: std::mem::take(&mut pane.tabs),
                    active_tab: pane.active_tab,
                };

                *self = if new_pane_first {
                    Self::split(
                        split_id,
                        axis,
                        ratio,
                        Self::Leaf(new_pane),
                        Self::Leaf(existing_pane),
                    )
                } else {
                    Self::split(
                        split_id,
                        axis,
                        ratio,
                        Self::Leaf(existing_pane),
                        Self::Leaf(new_pane),
                    )
                };

                true
            }
            Self::Leaf(_) => false,
            Self::Split(split) => {
                if split.first().contains_pane(pane_id) {
                    split.first_mut().split_pane_with_new_pane_first(
                        split_id,
                        pane_id,
                        axis,
                        new_pane,
                        ratio,
                        new_pane_first,
                    )
                } else {
                    split.second_mut().split_pane_with_new_pane_first(
                        split_id,
                        pane_id,
                        axis,
                        new_pane,
                        ratio,
                        new_pane_first,
                    )
                }
            }
        }
    }

    pub(crate) fn remove_pane(self, pane_id: PaneId) -> (Option<Self>, Option<Pane>) {
        match self {
            Self::Leaf(pane) if pane.id == pane_id => (None, Some(pane)),
            Self::Leaf(pane) => (Some(Self::Leaf(pane)), None),
            Self::Split(split) => remove_pane_from_split(split, pane_id),
        }
    }

    pub fn set_split_ratio(&mut self, split_id: SplitPaneId, ratio: f32) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split(split) if split.id() == split_id => split.try_set_ratio(ratio),
            Self::Split(split) => {
                if split.first_mut().set_split_ratio(split_id, ratio) {
                    true
                } else {
                    split.second_mut().set_split_ratio(split_id, ratio)
                }
            }
        }
    }

    fn largest_pane(&self, area: f32) -> (PaneId, f32) {
        match self {
            Self::Leaf(pane) => (pane.id(), area),
            Self::Split(split) => {
                let first = split.first().largest_pane(area * split.ratio());
                let second = split.second().largest_pane(area * (1.0 - split.ratio()));

                if first.1 >= second.1 { first } else { second }
            }
        }
    }
}

fn remove_pane_from_split(split: SplitPane, pane_id: PaneId) -> (Option<PaneNode>, Option<Pane>) {
    let SplitPane {
        id,
        axis,
        ratio,
        first,
        second,
    } = split;

    let (first_node, removed_pane) = (*first).remove_pane(pane_id);
    if let Some(removed_pane) = removed_pane {
        let node = match first_node {
            Some(first_node) => PaneNode::split(id, axis, ratio, first_node, *second),
            None => *second,
        };

        return (Some(node), Some(removed_pane));
    }

    let first_node = first_node.expect("pane removal without a removed pane must keep the node");
    let (second_node, removed_pane) = (*second).remove_pane(pane_id);
    if let Some(removed_pane) = removed_pane {
        let node = match second_node {
            Some(second_node) => PaneNode::split(id, axis, ratio, first_node, second_node),
            None => first_node,
        };

        return (Some(node), Some(removed_pane));
    }

    let second_node = second_node.expect("pane removal without a removed pane must keep the node");

    (
        Some(PaneNode::split(id, axis, ratio, first_node, second_node)),
        None,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplitPane {
    id: SplitPaneId,
    axis: SplitAxis,
    ratio: f32,
    first: Box<PaneNode>,
    second: Box<PaneNode>,
}

impl SplitPane {
    pub fn new(
        id: SplitPaneId,
        axis: SplitAxis,
        ratio: f32,
        first: PaneNode,
        second: PaneNode,
    ) -> Self {
        validate_split_ratio(ratio);

        Self {
            id,
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    pub fn id(&self) -> SplitPaneId {
        self.id
    }

    pub fn axis(&self) -> SplitAxis {
        self.axis
    }

    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        validate_split_ratio(ratio);

        self.ratio = ratio;
    }

    pub fn try_set_ratio(&mut self, ratio: f32) -> bool {
        if !is_valid_split_ratio(ratio) {
            return false;
        }

        self.ratio = ratio;
        true
    }

    pub fn first(&self) -> &PaneNode {
        &self.first
    }

    pub fn first_mut(&mut self) -> &mut PaneNode {
        &mut self.first
    }

    pub fn second(&self) -> &PaneNode {
        &self.second
    }

    pub fn second_mut(&mut self) -> &mut PaneNode {
        &mut self.second
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    id: PaneId,
    tabs: Vec<Tab>,
    active_tab: Option<TabId>,
}

impl Pane {
    pub fn new(id: PaneId, initial_tab: Tab) -> Self {
        let active_tab = initial_tab.id();

        Self {
            id,
            tabs: vec![initial_tab],
            active_tab: Some(active_tab),
        }
    }

    pub fn with_tab(id: PaneId, tab: Tab) -> Self {
        Self::new(id, tab)
    }

    pub fn id(&self) -> PaneId {
        self.id
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub(crate) fn rename_tab(&mut self, tab_id: TabId, title: impl Into<String>) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id() == tab_id) else {
            return false;
        };
        tab.rename(title);
        true
    }

    pub fn active_tab_id(&self) -> TabId {
        self.active_tab
            .expect("a pane exposed by core must always have an active tab")
    }

    pub fn active_tab(&self) -> &Tab {
        let active_tab_id = self.active_tab_id();

        self.tabs
            .iter()
            .find(|tab| tab.id() == active_tab_id)
            .expect("a pane exposed by core must contain its active tab")
    }

    pub fn add_tab(&mut self, tab: Tab) {
        self.insert_tab(self.tabs.len(), tab);
    }

    pub fn insert_tab(&mut self, index: usize, tab: Tab) {
        let tab_id = tab.id();
        let index = index.min(self.tabs.len());
        self.tabs.insert(index, tab);

        if self.active_tab.is_none() {
            self.active_tab = Some(tab_id);
        }
    }

    pub fn reorder_tab(&mut self, tab_id: TabId, target_index: usize) -> bool {
        let Some(current_index) = self.tabs.iter().position(|tab| tab.id() == tab_id) else {
            return false;
        };
        let target_index = target_index.min(self.tabs.len());
        let target_index = if target_index > current_index {
            target_index - 1
        } else {
            target_index
        };

        if current_index == target_index {
            return false;
        }

        let tab = self.tabs.remove(current_index);
        self.tabs.insert(target_index, tab);

        true
    }

    pub fn activate_tab(&mut self, tab_id: TabId) -> bool {
        if self.contains_tab(tab_id) {
            self.active_tab = Some(tab_id);
            true
        } else {
            false
        }
    }

    pub fn set_tab_kind(&mut self, tab_id: TabId, kind: TabKind) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id() == tab_id) else {
            return false;
        };

        tab.set_kind(kind);
        true
    }

    pub(crate) fn remove_tab(&mut self, tab_id: TabId) -> Option<Tab> {
        let tab_index = self.tabs.iter().position(|tab| tab.id() == tab_id)?;
        let removed_tab = self.tabs.remove(tab_index);

        if self.active_tab == Some(tab_id) {
            self.active_tab = self
                .tabs
                .get(tab_index)
                .or_else(|| self.tabs.last())
                .map(Tab::id);
        }

        Some(removed_tab)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn contains_tab(&self, tab_id: TabId) -> bool {
        self.tabs.iter().any(|tab| tab.id() == tab_id)
    }
}

fn validate_split_ratio(ratio: f32) {
    assert!(
        is_valid_split_ratio(ratio),
        "split ratio must be a finite value between 0.0 and 1.0"
    );
}

fn is_valid_split_ratio(ratio: f32) -> bool {
    ratio.is_finite() && ratio > 0.0 && ratio < 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Tab, TabKind};

    #[test]
    fn removing_active_tab_moves_focus_to_neighbor() {
        let first_tab = Tab::new(TabId::new(1), "First", TabKind::Blank);
        let second_tab = Tab::new(TabId::new(2), "Second", TabKind::Blank);
        let mut pane = Pane::with_tab(PaneId::new(1), first_tab);

        pane.add_tab(second_tab);
        pane.activate_tab(TabId::new(1));

        let removed = pane.remove_tab(TabId::new(1));

        assert_eq!(removed.map(|tab| tab.id()), Some(TabId::new(1)));
        assert_eq!(pane.active_tab_id(), TabId::new(2));
    }

    #[test]
    fn reordering_tab_places_it_at_target_index() {
        let mut pane = Pane::with_tab(
            PaneId::new(1),
            Tab::new(TabId::new(1), "First", TabKind::Blank),
        );
        pane.add_tab(Tab::new(TabId::new(2), "Second", TabKind::Blank));
        pane.add_tab(Tab::new(TabId::new(3), "Third", TabKind::Blank));

        assert!(pane.reorder_tab(TabId::new(1), 3));

        let tab_ids = pane.tabs().iter().map(Tab::id).collect::<Vec<_>>();
        assert_eq!(tab_ids, vec![TabId::new(2), TabId::new(3), TabId::new(1)]);
    }

    #[test]
    fn splitting_leaf_creates_recursive_node() {
        let first_pane = Pane::new(
            PaneId::new(1),
            Tab::new(TabId::new(1), "First", TabKind::Blank),
        );
        let second_pane = Pane::new(
            PaneId::new(2),
            Tab::new(TabId::new(2), "Second", TabKind::Blank),
        );
        let mut root = PaneNode::leaf(first_pane);

        assert!(root.split_pane(
            SplitPaneId::new(1),
            PaneId::new(1),
            SplitAxis::Horizontal,
            second_pane,
            0.5,
        ));

        assert!(root.contains_pane(PaneId::new(1)));
        assert!(root.contains_pane(PaneId::new(2)));
    }

    #[test]
    fn splitting_leaf_can_place_new_pane_first() {
        let first_pane = Pane::new(
            PaneId::new(1),
            Tab::new(TabId::new(1), "First", TabKind::Blank),
        );
        let second_pane = Pane::new(
            PaneId::new(2),
            Tab::new(TabId::new(2), "Second", TabKind::Blank),
        );
        let mut root = PaneNode::leaf(first_pane);

        assert!(root.split_pane_with_new_pane_first(
            SplitPaneId::new(1),
            PaneId::new(1),
            SplitAxis::Horizontal,
            second_pane,
            0.5,
            true,
        ));

        let PaneNode::Split(split) = root else {
            panic!("splitting a leaf must create a split node");
        };

        assert_eq!(split.first().first_pane_id(), PaneId::new(2));
        assert_eq!(split.second().first_pane_id(), PaneId::new(1));
    }

    #[test]
    fn removing_pane_collapses_split_to_sibling() {
        let first_pane = Pane::new(
            PaneId::new(1),
            Tab::new(TabId::new(1), "First", TabKind::Blank),
        );
        let second_pane = Pane::new(
            PaneId::new(2),
            Tab::new(TabId::new(2), "Second", TabKind::Blank),
        );
        let root = PaneNode::split(
            SplitPaneId::new(1),
            SplitAxis::Horizontal,
            0.5,
            PaneNode::leaf(first_pane),
            PaneNode::leaf(second_pane),
        );

        let (root, removed_pane) = root.remove_pane(PaneId::new(1));
        let root = root.expect("removing one pane from a split must keep its sibling");

        assert_eq!(removed_pane.map(|pane| pane.id()), Some(PaneId::new(1)));
        assert!(!root.contains_pane(PaneId::new(1)));
        assert!(root.contains_pane(PaneId::new(2)));
        assert_eq!(root.pane_count(), 1);
    }

    #[test]
    fn largest_pane_follows_split_ratios() {
        let first_pane = Pane::new(
            PaneId::new(1),
            Tab::new(TabId::new(1), "First", TabKind::Blank),
        );
        let nested_first_pane = Pane::new(
            PaneId::new(2),
            Tab::new(TabId::new(2), "Second", TabKind::Blank),
        );
        let nested_second_pane = Pane::new(
            PaneId::new(3),
            Tab::new(TabId::new(3), "Third", TabKind::Blank),
        );
        let root = PaneNode::split(
            SplitPaneId::new(1),
            SplitAxis::Horizontal,
            0.4,
            PaneNode::leaf(first_pane),
            PaneNode::split(
                SplitPaneId::new(2),
                SplitAxis::Vertical,
                0.75,
                PaneNode::leaf(nested_first_pane),
                PaneNode::leaf(nested_second_pane),
            ),
        );

        assert_eq!(root.largest_pane_id(), PaneId::new(2));
    }
}

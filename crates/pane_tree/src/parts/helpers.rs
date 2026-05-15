impl PaneTree {
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

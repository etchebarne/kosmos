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

    pub fn set_tab_path(&mut self, tab_id: usize, path: Option<std::path::PathBuf>) -> bool {
        let Some(tab) = Self::find_tab_mut_in(&mut self.root, tab_id) else {
            return false;
        };
        if tab.path == path {
            return false;
        }
        tab.path = path;
        true
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
        Self::find_pane(&self.root, target_pane_id)?;

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

}

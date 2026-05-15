impl PaneTree {
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

}

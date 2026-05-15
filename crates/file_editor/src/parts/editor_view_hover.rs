impl EditorView {
    pub fn hover(&self) -> Option<&EditorHover> {
        self.hover.as_ref()
    }

    pub fn editor_bounds(&self) -> Option<Bounds<Pixels>> {
        self.editor_bounds
    }

    pub fn set_editor_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.editor_bounds = Some(bounds);
    }

    pub fn gutter_hovered(&self) -> bool {
        self.gutter_hovered
    }

    pub fn set_gutter_hover_state(
        &mut self,
        hovered: bool,
        hovered_fold_line: Option<usize>,
    ) -> bool {
        let hovered_fold_line = hovered.then_some(hovered_fold_line).flatten();
        if self.gutter_hovered == hovered && self.hovered_fold_line == hovered_fold_line {
            return false;
        }
        self.gutter_hovered = hovered;
        self.hovered_fold_line = hovered_fold_line;
        true
    }

    pub fn hovered_fold_line(&self) -> Option<usize> {
        self.hovered_fold_line
    }

    pub fn folded_lines(&self) -> &HashSet<usize> {
        &self.folded_lines
    }

    pub fn toggle_folded_line(&mut self, line_index: usize) {
        if !self.folded_lines.remove(&line_index) {
            self.folded_lines.insert(line_index);
        }
    }

    pub fn begin_hover(
        &mut self,
        line_index: usize,
        byte_index: usize,
        byte_range: Range<usize>,
    ) -> Option<u64> {
        if self.hover.as_mut().is_some_and(|hover| {
            hover.line_index == line_index
                && hover.byte_range == byte_range
                && !matches!(hover.status, EditorHoverStatus::Empty)
        }) {
            if let Some(hover) = self.hover.as_mut() {
                hover.hide_pending = false;
            }
            return None;
        }

        self.hover_generation = self.hover_generation.wrapping_add(1);
        let generation = self.hover_generation;
        self.hover = Some(EditorHover {
            line_index,
            byte_index,
            byte_range,
            generation,
            hide_generation: 0,
            hide_pending: false,
            source_highlight_visible: true,
            source_bounds: None,
            popup_bounds: None,
            status: EditorHoverStatus::Loading,
        });
        Some(generation)
    }

    pub fn hover_matches(&self, generation: u64) -> bool {
        self.hover
            .as_ref()
            .is_some_and(|hover| hover.generation == generation)
    }

    pub fn finish_hover(&mut self, generation: u64, status: EditorHoverStatus) {
        let Some(hover) = self.hover.as_mut() else {
            return;
        };
        if hover.generation == generation {
            let is_empty = matches!(status, EditorHoverStatus::Empty);
            hover.status = status;
            if is_empty {
                hover.source_highlight_visible = false;
            }
        }
    }

    pub fn clear_hover_for_line(&mut self, line_index: usize) {
        if self
            .hover
            .as_ref()
            .is_some_and(|hover| hover.line_index == line_index)
        {
            self.hover_generation = self.hover_generation.wrapping_add(1);
            self.hover = None;
        }
    }

    pub fn cancel_hover_hide_for_line(&mut self, line_index: usize) {
        if let Some(hover) = self.hover.as_mut()
            && hover.line_index == line_index
        {
            hover.hide_pending = false;
        }
    }

    pub fn set_hover_source_bounds(
        &mut self,
        line_index: usize,
        byte_range: Range<usize>,
        bounds: Bounds<Pixels>,
    ) {
        if let Some(hover) = self.hover.as_mut()
            && hover.line_index == line_index
            && hover.byte_range == byte_range
        {
            hover.source_bounds = Some(bounds);
        }
    }

    pub fn set_hover_popup_bounds(&mut self, line_index: usize, bounds: Bounds<Pixels>) {
        if let Some(hover) = self.hover.as_mut()
            && hover.line_index == line_index
        {
            hover.popup_bounds = Some(bounds);
        }
    }

    pub fn schedule_hover_hide_for_line(&mut self, line_index: usize) -> Option<u64> {
        let hover = self.hover.as_mut()?;
        if hover.line_index != line_index || hover.hide_pending {
            return None;
        }

        self.hover_hide_generation = self.hover_hide_generation.wrapping_add(1);
        hover.hide_generation = self.hover_hide_generation;
        hover.hide_pending = true;
        Some(hover.hide_generation)
    }

    pub fn clear_scheduled_hover(&mut self, line_index: usize, hide_generation: u64) {
        if self.hover.as_ref().is_some_and(|hover| {
            hover.line_index == line_index
                && hover.hide_pending
                && hover.hide_generation == hide_generation
        }) {
            self.hover_generation = self.hover_generation.wrapping_add(1);
            self.hover = None;
        }
    }

    /// Pixel width measured for the longest line, valid only at the
    /// `rem_size` it was captured at. Returns `None` if we haven't measured
    /// yet or if the rem has since changed.
    pub fn cached_longest_width(&self, rem_size: Pixels) -> Option<Pixels> {
        let cached_rem = self.cached_longest_rem.get()?;
        if cached_rem != rem_size {
            return None;
        }
        self.cached_longest_width.get()
    }

    pub fn set_cached_longest_width(&self, rem_size: Pixels, width: Pixels) {
        self.cached_longest_width.set(Some(width));
        self.cached_longest_rem.set(Some(rem_size));
    }

    pub fn observed_external(&self) -> Option<EntityId> {
        self.observed_external
    }

    pub fn set_observed_external(&mut self, id: EntityId) {
        self.observed_external = Some(id);
    }

    pub fn uniform_scroll(&self) -> UniformListScrollHandle {
        self.uniform_scroll.clone()
    }

    pub fn virtual_scroll(&self) -> VirtualListState {
        self.virtual_scroll.clone()
    }
}

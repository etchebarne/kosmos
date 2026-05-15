impl EditorView {
    pub fn new(_row_count: usize, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            buffer: None,
            selected_range: 0..0,
            selection_reversed: false,
            is_selecting: false,
            marked_range: None,
            input_layout: None,
            line_layouts: HashMap::new(),
            uniform_scroll: UniformListScrollHandle::new(),
            virtual_scroll: VirtualListState::new(),
            observed_external: None,
            cached_longest_width: Cell::new(None),
            cached_longest_rem: Cell::new(None),
            editor_bounds: None,
            gutter_hovered: false,
            hovered_fold_line: None,
            folded_lines: HashSet::new(),
            hover_generation: 0,
            hover_hide_generation: 0,
            hover: None,
        }
    }

    pub fn set_buffer(&mut self, buffer: Entity<Buffer>, cx: &mut Context<Self>) {
        let len = buffer.read(cx).content().len();
        let changed = self
            .buffer
            .as_ref()
            .is_none_or(|current| current.entity_id() != buffer.entity_id());
        self.buffer = Some(buffer);
        if changed {
            self.selected_range = 0..0;
            self.selection_reversed = false;
            self.is_selecting = false;
            self.marked_range = None;
            self.line_layouts.clear();
        } else {
            self.clamp_selection(len);
        }
    }

    pub fn set_input_layout(&mut self, layout: EditorInputLayout) {
        self.input_layout = Some(layout);
    }

    pub fn set_line_input_layout(&mut self, layout: EditorLineInputLayout) {
        self.line_layouts.insert(layout.line_index, layout);
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn selected_range(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    pub fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub fn select_at_point(
        &mut self,
        position: gpui::Point<Pixels>,
        extend_selection: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(offset) = self.offset_for_point(position, cx) else {
            return;
        };
        if extend_selection {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    pub fn begin_selection_at_point(
        &mut self,
        position: gpui::Point<Pixels>,
        extend_selection: bool,
        click_count: usize,
        cx: &mut Context<Self>,
    ) {
        if click_count >= 3 {
            self.is_selecting = false;
            self.select_line_at_point(position, cx);
            return;
        }
        if click_count == 2 {
            self.is_selecting = false;
            self.select_word_at_point(position, cx);
            return;
        }
        self.is_selecting = true;
        self.select_at_point(position, extend_selection, cx);
    }

    pub fn extend_selection_at_point(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_selecting {
            return false;
        }
        let Some(offset) = self.offset_for_point(position, cx) else {
            return true;
        };
        self.select_to(offset, cx);
        true
    }

    pub fn finish_selection(&mut self) {
        self.is_selecting = false;
    }

    pub fn select_word_at_point(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(offset) = self.offset_for_point(position, cx) else {
            return;
        };
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        let Some(range) = word_range_at_offset(&content, offset) else {
            return;
        };
        self.break_undo_group(cx);
        self.selected_range = range;
        self.selection_reversed = false;
        cx.notify();
    }

    pub fn select_line_at_point(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(offset) = self.offset_for_point(position, cx) else {
            return;
        };
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        let offset = clamp_to_char_boundary(&content, offset.min(content.len()));
        self.break_undo_group(cx);
        self.selected_range =
            line_start_for_offset(&content, offset)..line_end_for_offset(&content, offset);
        self.selection_reversed = false;
        cx.notify();
    }

}

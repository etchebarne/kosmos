impl EditorView {
    pub fn undo(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(buffer) = self.buffer.clone() else {
            return;
        };
        let Some(selection) = buffer.update(cx, |buffer, cx| buffer.undo(cx)) else {
            return;
        };
        self.apply_selection_snapshot(selection, cx);
        self.marked_range = None;
    }

    pub fn redo(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(buffer) = self.buffer.clone() else {
            return;
        };
        let Some(selection) = buffer.update(cx, |buffer, cx| buffer.redo(cx)) else {
            return;
        };
        self.apply_selection_snapshot(selection, cx);
        self.marked_range = None;
    }

    fn break_undo_group(&mut self, cx: &mut Context<Self>) {
        if let Some(buffer) = self.buffer.clone() {
            buffer.update(cx, |buffer, _| buffer.break_undo_group());
        }
    }

    fn selection_snapshot(&self) -> SelectionSnapshot {
        SelectionSnapshot {
            range: self.selected_range.clone(),
            reversed: self.selection_reversed,
        }
    }

    fn apply_selection_snapshot(&mut self, selection: SelectionSnapshot, cx: &mut Context<Self>) {
        self.selected_range = selection.range;
        self.selection_reversed = selection.reversed;
        if let Some(content) = self.buffer_content(cx) {
            self.clamp_selection(content.len());
        }
        cx.notify();
    }

    fn replace_buffer_range(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        before_selection: SelectionSnapshot,
        after_selection: SelectionSnapshot,
        group_with_previous: bool,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let buffer = self.buffer.clone()?;
        let inserted = buffer.update(cx, |buffer, cx| {
            buffer.replace_range_with_selection(
                range,
                new_text,
                before_selection,
                Some(after_selection.clone()),
                group_with_previous,
                cx,
            )
        });
        self.apply_selection_snapshot(after_selection, cx);
        self.marked_range = None;
        Some(inserted)
    }

    fn buffer_content(&self, cx: &Context<Self>) -> Option<String> {
        self.buffer
            .as_ref()
            .map(|buffer| buffer.read(cx).content().to_string())
    }

    fn replace_text(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.replace_text_with_grouping(range_utf16, new_text, None, cx)
    }

    fn replace_text_with_grouping(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        group_with_previous: Option<bool>,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let before_selection = self.selection_snapshot();
        self.replace_text_with_before_selection(
            range_utf16,
            new_text,
            before_selection,
            group_with_previous,
            cx,
        )
    }

    fn replace_text_with_before_selection(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        before_selection: SelectionSnapshot,
        group_with_previous: Option<bool>,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let buffer = self.buffer.clone()?;
        let content = buffer.read(cx).content().to_string();
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| range_from_utf16(&content, range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let range = clamp_range_to_char_boundaries(&content, range);
        let normalized_new_text = normalize_newlines(new_text);
        let group_with_previous = group_with_previous
            .unwrap_or_else(|| should_group_edit(&content, &range, &normalized_new_text));
        let inserted = buffer.update(cx, |buffer, cx| {
            buffer.replace_range_with_selection(
                range,
                new_text,
                before_selection,
                None,
                group_with_previous,
                cx,
            )
        });
        self.selected_range = inserted.end..inserted.end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
        Some(inserted)
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let content = self.buffer_content(cx).unwrap_or_default();
        let offset = clamp_to_char_boundary(&content, offset.min(content.len()));
        self.break_undo_group(cx);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        let offset = clamp_to_char_boundary(&content, offset.min(content.len()));
        self.break_undo_group(cx);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn move_vertical(&mut self, delta: isize, extend_selection: bool, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        let (row, column) = line_column_for_offset(&content, self.cursor_offset());
        let target_row = if delta.is_negative() {
            row.saturating_sub(delta.unsigned_abs())
        } else {
            row.saturating_add(delta as usize)
        };
        let target = offset_for_line_column(&content, target_row, column);
        if extend_selection {
            self.select_to(target, cx);
        } else {
            self.move_to(target, cx);
        }
    }

    fn clamp_selection(&mut self, len: usize) {
        self.selected_range.start = self.selected_range.start.min(len);
        self.selected_range.end = self.selected_range.end.min(len);
        if self.selected_range.end < self.selected_range.start {
            self.selected_range = self.selected_range.end..self.selected_range.start;
            self.selection_reversed = !self.selection_reversed;
        }
        if let Some(marked) = self.marked_range.as_mut() {
            marked.start = marked.start.min(len);
            marked.end = marked.end.min(len);
        }
    }

    fn offset_for_point(&self, position: gpui::Point<Pixels>, cx: &Context<Self>) -> Option<usize> {
        let layout = self.input_layout.as_ref()?;
        let buffer = self.buffer.as_ref()?.read(cx);

        if !layout.soft_wrap {
            return self.offset_for_uniform_point(position, layout, &buffer);
        }

        for line_layout in self.line_layouts.values() {
            if let Some(offset) = offset_for_line_layout(position, line_layout, &buffer) {
                return Some(offset);
            }
        }

        self.offset_for_uniform_point(position, layout, &buffer)
    }

    fn offset_for_uniform_point(
        &self,
        position: gpui::Point<Pixels>,
        layout: &EditorInputLayout,
        buffer: &Buffer,
    ) -> Option<usize> {
        if layout.row_height <= Pixels::ZERO || layout.char_width <= Pixels::ZERO {
            return Some(buffer.content().len());
        }

        let y = (position.y - layout.bounds.top() + layout.scroll_y).max(Pixels::ZERO);
        let row_index = (y / layout.row_height).floor() as usize;
        let Some(&line_index) = layout.visible_lines.get(row_index) else {
            return Some(buffer.content().len());
        };
        let Some(line_range) = buffer.line_range(line_index) else {
            return Some(buffer.content().len());
        };

        if let Some(line_layout) = self.line_layouts.get(&line_index)
            && let Some(offset) = offset_for_line_layout(position, line_layout, buffer)
        {
            return Some(offset);
        }

        let x = (position.x - layout.bounds.left() - layout.text_left + layout.scroll_x)
            .max(Pixels::ZERO);
        let column = (x / layout.char_width).round() as usize;
        let line = &buffer.content()[line_range.clone()];
        Some(line_range.start + byte_for_visual_column(line, column))
    }

}

fn offset_for_line_layout(
    position: gpui::Point<Pixels>,
    line_layout: &EditorLineInputLayout,
    buffer: &Buffer,
) -> Option<usize> {
    let bounds = line_layout.text_layout.bounds();
    if position.y < bounds.top() || position.y > bounds.bottom() {
        return None;
    }
    let line_range = buffer.line_range(line_layout.line_index)?;
    let line = &buffer.content()[line_range.clone()];
    let display = &line[line_layout.display_byte_offset.min(line.len())..];
    let display_offset = line_layout
        .text_layout
        .index_for_position(position)
        .unwrap_or_else(|offset| offset);
    let display_offset = clamp_to_char_boundary(display, display_offset.min(display.len()));
    Some(line_range.start + line_layout.display_byte_offset + display_offset)
}

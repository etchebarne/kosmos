impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let content = self.buffer_content(cx)?;
        let range = range_from_utf16(&content, &range_utf16);
        actual_range.replace(range_to_utf16(&content, &range));
        Some(content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let content = self.buffer_content(cx)?;
        self.clamp_selection(content.len());
        Some(UTF16Selection {
            range: range_to_utf16(&content, &self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let content = self.buffer_content(cx)?;
        self.marked_range
            .as_ref()
            .map(|range| range_to_utf16(&content, range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text(range_utf16, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(inserted) = self.replace_text(range_utf16, new_text, cx) else {
            return;
        };
        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(inserted.clone());
        }
        if let Some(new_selected_range_utf16) = new_selected_range_utf16
            && let Some(content) = self.buffer_content(cx)
        {
            let local = range_from_utf16(&content, &new_selected_range_utf16);
            let inserted_len = inserted.end.saturating_sub(inserted.start);
            let start = inserted.start + local.start.min(inserted_len);
            let end = inserted.start + local.end.min(inserted_len);
            self.selected_range = start..end;
        }
        self.selection_reversed = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let content = self.buffer_content(cx)?;
        let offset = self.offset_for_point(point, cx)?;
        Some(offset_to_utf16(&content, offset))
    }
}

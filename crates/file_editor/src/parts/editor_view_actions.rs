impl EditorView {
    pub fn enter(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_with_grouping(None, "\n", Some(false), cx);
    }

    pub fn backspace(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let before_selection = self.selection_snapshot();
        if self.selected_range.is_empty() {
            let Some(content) = self.buffer_content(cx) else {
                return;
            };
            let cursor = self.cursor_offset();
            let previous = previous_char_boundary(&content, cursor);
            if previous == cursor {
                return;
            }
            self.selected_range = previous..cursor;
            self.selection_reversed = false;
        }
        self.replace_text_with_before_selection(None, "", before_selection, None, cx);
    }

    pub fn delete(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let before_selection = self.selection_snapshot();
        if self.selected_range.is_empty() {
            let Some(content) = self.buffer_content(cx) else {
                return;
            };
            let cursor = self.cursor_offset();
            let next = next_char_boundary(&content, cursor);
            if next == cursor {
                return;
            }
            self.selected_range = cursor..next;
            self.selection_reversed = false;
        }
        self.replace_text_with_before_selection(None, "", before_selection, None, cx);
    }

    pub fn left(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        if self.selected_range.is_empty() {
            self.move_to(previous_char_boundary(&content, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    pub fn right(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        if self.selected_range.is_empty() {
            self.move_to(next_char_boundary(&content, self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    pub fn up(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, cx);
    }

    pub fn down(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false, cx);
    }

    pub fn word_left(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        if self.selected_range.is_empty() {
            self.move_to(previous_word_boundary(&content, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    pub fn word_right(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        if self.selected_range.is_empty() {
            self.move_to(next_word_boundary(&content, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    pub fn select_left(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        self.select_to(previous_char_boundary(&content, self.cursor_offset()), cx);
    }

    pub fn select_right(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        self.select_to(next_char_boundary(&content, self.cursor_offset()), cx);
    }

    pub fn select_up(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, cx);
    }

    pub fn select_down(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, cx);
    }

    pub fn select_word_left(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        self.select_to(previous_word_boundary(&content, self.cursor_offset()), cx);
    }

    pub fn select_word_right(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        self.select_to(next_word_boundary(&content, self.cursor_offset()), cx);
    }

    pub fn select_all(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        self.break_undo_group(cx);
        self.selected_range = 0..content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    pub fn home(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        self.move_to(line_start_for_offset(&content, self.cursor_offset()), cx);
    }

    pub fn end(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        self.move_to(line_end_for_offset(&content, self.cursor_offset()), cx);
    }

    pub fn copy(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        let range = if self.selected_range.is_empty() {
            line_range_for_selection(&content, &self.selected_range)
        } else {
            self.selected_range.clone()
        };
        if range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(content[range].to_string()));
    }

    pub fn cut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let Some(content) = self.buffer_content(cx) else {
                return;
            };
            let range = line_range_for_selection(&content, &self.selected_range);
            if range.is_empty() {
                return;
            }
            let before_selection = self.selection_snapshot();
            cx.write_to_clipboard(ClipboardItem::new_string(
                content[range.clone()].to_string(),
            ));
            self.replace_buffer_range(
                range.clone(),
                "",
                before_selection,
                SelectionSnapshot::collapsed(range.start),
                false,
                cx,
            );
        } else {
            self.copy(window, cx);
            self.replace_text_with_grouping(None, "", Some(false), cx);
        }
    }

    pub fn paste(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_with_grouping(None, &text, Some(false), cx);
        }
    }

    pub fn duplicate_line_up(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        let line_range = line_range_for_selection(&content, &self.selected_range);
        let duplicate = duplicate_text_for_line_range(&content, &line_range, true);
        let before_selection = self.selection_snapshot();
        let after_selection = before_selection.clone();
        self.replace_buffer_range(
            line_range.start..line_range.start,
            &duplicate,
            before_selection,
            after_selection,
            false,
            cx,
        );
    }

    pub fn duplicate_line_down(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = self.buffer_content(cx) else {
            return;
        };
        let line_range = line_range_for_selection(&content, &self.selected_range);
        let duplicate = duplicate_text_for_line_range(&content, &line_range, false);
        let before_selection = self.selection_snapshot();
        let after_selection = shift_selection(&before_selection, duplicate.len());
        self.replace_buffer_range(
            line_range.end..line_range.end,
            &duplicate,
            before_selection,
            after_selection,
            false,
            cx,
        );
    }

}

impl Buffer {
    pub(crate) fn new(id: BufferId, path: PathBuf, cx: &mut Context<Self>) -> Self {
        let content = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let (line_starts, line_chars, longest_line_index) = analyze_content(&content);
        let language = language::from_path(&path);
        Self {
            id,
            path,
            language,
            content,
            dirty: false,
            current_revision: 0,
            saved_revision: 0,
            next_revision: 1,
            open_undo_group: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            line_starts,
            line_chars,
            longest_line_index,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn language(&self) -> Option<&LanguageId> {
        self.language.as_ref()
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// `line_count` plus the trailing empty spacer rows used to allow
    /// scrolling past the last real line. The renderer feeds this to
    /// `uniform_list` / `list` so they reserve scrollable space for it.
    pub fn row_count(&self) -> usize {
        self.line_starts.len() + BOTTOM_SPACER_LINES
    }

    pub fn longest_line_index(&self) -> usize {
        self.longest_line_index
    }

    /// Character count of `line_index`, excluding the trailing newline.
    /// Returns 0 for out-of-range indexes (so callers iterating past the
    /// real lines into the bottom-spacer rows can keep going without
    /// branching).
    pub fn line_chars(&self, line_index: usize) -> usize {
        self.line_chars.get(line_index).copied().unwrap_or(0)
    }

    /// Byte range of `line_index` within `content`, excluding the trailing
    /// newline. `None` if the index is out of range.
    pub fn line_range(&self, line_index: usize) -> Option<Range<usize>> {
        let start = *self.line_starts.get(line_index)?;
        let end = match self.line_starts.get(line_index + 1) {
            // Subtract one to drop the '\n' that begins the next line's start.
            Some(&next) => next - 1,
            None => self.content.len(),
        };
        Some(start..end)
    }

    pub fn line(&self, line_index: usize) -> Option<&str> {
        let range = self.line_range(line_index)?;
        Some(&self.content[range])
    }

    pub fn replace_range(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let before_selection = SelectionSnapshot::collapsed(range.end);
        self.replace_range_with_selection(range, new_text, before_selection, None, true, cx)
    }

    fn replace_range_with_selection(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        before_selection: SelectionSnapshot,
        after_selection: Option<SelectionSnapshot>,
        group_with_previous: bool,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let range = clamp_range_to_char_boundaries(&self.content, range);
        let new_text = normalize_newlines(new_text);
        let new_end = range.start + new_text.len();
        if self.content[range.clone()] == new_text {
            return range.start..new_end;
        }

        let old_text = self.content[range.clone()].to_string();
        let before_revision = self.current_revision;
        let after_revision = self.next_revision;
        self.next_revision += 1;
        let inserted =
            self.replace_range_without_history(range.clone(), &new_text, after_revision, cx);
        self.push_edit_operation(
            TextChange {
                old_position: range.start,
                old_text,
                new_position: inserted.start,
                new_text,
            },
            before_selection,
            after_selection.unwrap_or_else(|| SelectionSnapshot::collapsed(inserted.end)),
            before_revision,
            after_revision,
            group_with_previous,
        );
        self.redo_stack.clear();
        inserted
    }

    fn undo(&mut self, cx: &mut Context<Self>) -> Option<SelectionSnapshot> {
        self.open_undo_group = false;
        let element = self.undo_stack.pop()?;
        self.apply_undo_changes(&element, cx);
        let selection = element.before_selection.clone();
        self.redo_stack.push(element);
        Some(selection)
    }

    fn redo(&mut self, cx: &mut Context<Self>) -> Option<SelectionSnapshot> {
        self.open_undo_group = false;
        let element = self.redo_stack.pop()?;
        self.apply_redo_changes(&element, cx);
        let selection = element.after_selection.clone();
        self.undo_stack.push(element);
        Some(selection)
    }

    fn break_undo_group(&mut self) {
        self.open_undo_group = false;
    }

    fn push_edit_operation(
        &mut self,
        change: TextChange,
        before_selection: SelectionSnapshot,
        after_selection: SelectionSnapshot,
        before_revision: u64,
        after_revision: u64,
        group_with_previous: bool,
    ) {
        if group_with_previous
            && self.open_undo_group
            && let Some(element) = self.undo_stack.last_mut()
        {
            element.append(change, after_selection, after_revision);
            return;
        }

        self.undo_stack.push(EditStackElement::new(
            change,
            before_selection,
            after_selection,
            before_revision,
            after_revision,
        ));
        if self.undo_stack.len() > MAX_UNDO_DEPTH {
            self.undo_stack.remove(0);
        }
        self.open_undo_group = group_with_previous;
    }

    fn apply_undo_changes(&mut self, element: &EditStackElement, cx: &mut Context<Self>) {
        let mut changes = element.changes.clone();
        changes.sort_by(|a, b| b.new_position.cmp(&a.new_position));
        for change in changes {
            self.replace_range_without_history(
                change.new_position..change.new_end(),
                &change.old_text,
                element.before_revision,
                cx,
            );
        }
    }

    fn apply_redo_changes(&mut self, element: &EditStackElement, cx: &mut Context<Self>) {
        let mut changes = element.changes.clone();
        changes.sort_by(|a, b| b.old_position.cmp(&a.old_position));
        for change in changes {
            self.replace_range_without_history(
                change.old_position..change.old_end(),
                &change.new_text,
                element.after_revision,
                cx,
            );
        }
    }

    fn replace_range_without_history(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        revision: u64,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let range = clamp_range_to_char_boundaries(&self.content, range);
        let new_text = normalize_newlines(new_text);
        let new_end = range.start + new_text.len();
        if self.content[range.clone()] == new_text {
            return range.start..new_end;
        }

        let old_content = self.content.clone();
        let start_point = point_for_offset(&old_content, range.start);
        let old_end_point = point_for_offset(&old_content, range.end);
        let new_end_point = advance_point(start_point, &new_text);

        self.content.replace_range(range.clone(), &new_text);
        self.reanalyze();
        self.current_revision = revision;
        self.dirty = self.current_revision != self.saved_revision;

        cx.emit(BufferEvent::Edited {
            edits: vec![TextEdit {
                start_byte: range.start,
                old_end_byte: range.end,
                new_end_byte: new_end,
                start_point,
                old_end_point,
                new_end_point,
            }],
        });
        cx.notify();
        range.start..new_end
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        std::fs::write(&self.path, &self.content)?;
        if self.dirty {
            self.saved_revision = self.current_revision;
            self.dirty = false;
            cx.notify();
        }
        self.open_undo_group = false;
        Ok(())
    }

    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        if self.dirty {
            return;
        }
        let Ok(content) = std::fs::read_to_string(&self.path) else {
            return;
        };
        let content = content.replace("\r\n", "\n");
        if content == self.content {
            return;
        }

        let old_content = std::mem::replace(&mut self.content, content);
        self.reanalyze();
        self.current_revision = self.next_revision;
        self.next_revision += 1;
        self.saved_revision = self.current_revision;
        self.dirty = false;
        self.open_undo_group = false;
        self.undo_stack.clear();
        self.redo_stack.clear();

        cx.emit(BufferEvent::Edited {
            edits: vec![TextEdit {
                start_byte: 0,
                old_end_byte: old_content.len(),
                new_end_byte: self.content.len(),
                start_point: Point { row: 0, column: 0 },
                old_end_point: end_point(&old_content),
                new_end_point: end_point(&self.content),
            }],
        });
        cx.notify();
    }

    fn reanalyze(&mut self) {
        let (line_starts, line_chars, longest_line_index) = analyze_content(&self.content);
        self.line_starts = line_starts;
        self.line_chars = line_chars;
        self.longest_line_index = longest_line_index;
    }
}

impl Focusable for Buffer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<BufferEvent> for Buffer {}

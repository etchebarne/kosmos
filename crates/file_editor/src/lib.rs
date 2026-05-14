mod virtual_list;

pub use virtual_list::{VirtualList, VirtualListState, virtual_list};

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext, BorrowAppContext, Bounds, ClipboardItem, Context, Entity, EntityId,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, Global, Pixels, TextLayout,
    UTF16Selection, UniformListScrollHandle, Window, actions,
};
use language::LanguageId;
use settings::{ActiveSettings, SettingValue};

actions!(file_editor, [Save]);

const MAX_UNDO_DEPTH: usize = 1000;

/// Stable identifier for an open buffer. Issued by [`BufferStore`] and never
/// reused, so other systems (syntax parsers, diagnostics, persisted per-buffer
/// state) can hold onto an id across path changes, untitled buffers, or
/// multi-root collisions where two paths could otherwise alias.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct BufferId(u64);

impl BufferId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Row/column position within a buffer's text. Mirrors the shape of
/// `tree_sitter::Point` so downstream consumers can convert without us taking
/// a tree-sitter dependency here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Point {
    pub row: usize,
    pub column: usize,
}

/// One byte-level edit applied to a buffer. Mirrors `tree_sitter::InputEdit`
/// for the same reason: lets the `syntax` crate forward edits straight into
/// an incremental reparse without any cross-crate coupling here.
#[derive(Clone, Copy, Debug)]
pub struct TextEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_point: Point,
    pub old_end_point: Point,
    pub new_end_point: Point,
}

/// Events emitted by a [`Buffer`] when its observable state changes. Wired
/// through gpui's [`EventEmitter`] so per-buffer subsystems (syntax trees,
/// diagnostics, semantic analyses) can subscribe without polling. No
/// emissions exist yet — editing isn't implemented — but the contract is
/// pinned now so subsystems can be wired against it from day one.
#[derive(Clone, Debug)]
pub enum BufferEvent {
    Edited { edits: Vec<TextEdit> },
    LanguageChanged,
}

pub const SOFT_WRAP_SETTING_ID: &str = "editor.soft_wrap";

/// Extra empty rows appended to the end of the editor's row list so the user
/// can scroll past the last real line — same idea as VS Code's
/// `scrollBeyondLastLine`. The renderer is responsible for drawing rows
/// `>= line_count` as blank spacers.
pub const BOTTOM_SPACER_LINES: usize = 20;

/// Resolve `editor.soft_wrap` from the global settings, falling back to the
/// default declared in `settings::registry::EDITOR`.
pub fn soft_wrap_enabled(cx: &App) -> bool {
    cx.settings()
        .get(SOFT_WRAP_SETTING_ID)
        .and_then(SettingValue::as_bool)
        .unwrap_or(false)
}

/// In-memory view of a file open in an editor tab. Holds the loaded text plus
/// a cached `line_starts` index so the renderer (and, later, LSP-driven
/// analysis) can resolve any line in O(1) without rescanning the content.
///
/// Shared across all tabs viewing the same path. Per-tab state (scroll
/// position, list measurement caches) lives on [`EditorView`] instead so two
/// tabs of the same file scroll independently.
pub struct Buffer {
    id: BufferId,
    path: PathBuf,
    language: Option<LanguageId>,
    content: String,
    dirty: bool,
    current_revision: u64,
    saved_revision: u64,
    next_revision: u64,
    open_undo_group: bool,
    undo_stack: Vec<EditStackElement>,
    redo_stack: Vec<EditStackElement>,
    line_starts: Vec<usize>,
    /// Per-line character count (excluding the trailing newline). Used by
    /// the soft-wrap path to estimate row heights without doing real text
    /// shaping — `wraps = ceil(chars / chars_per_visible_width)`.
    line_chars: Vec<usize>,
    /// Index of the line with the most characters. Used by `uniform_list` as
    /// the row to measure when sizing the horizontal extent of the editor.
    longest_line_index: usize,
    focus_handle: FocusHandle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectionSnapshot {
    range: Range<usize>,
    reversed: bool,
}

impl SelectionSnapshot {
    fn collapsed(offset: usize) -> Self {
        Self {
            range: offset..offset,
            reversed: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextChange {
    old_position: usize,
    old_text: String,
    new_position: usize,
    new_text: String,
}

impl TextChange {
    fn old_end(&self) -> usize {
        self.old_position + self.old_text.len()
    }

    fn new_end(&self) -> usize {
        self.new_position + self.new_text.len()
    }
}

#[derive(Clone, Debug)]
struct EditStackElement {
    changes: Vec<TextChange>,
    before_selection: SelectionSnapshot,
    after_selection: SelectionSnapshot,
    before_revision: u64,
    after_revision: u64,
}

impl EditStackElement {
    fn new(
        change: TextChange,
        before_selection: SelectionSnapshot,
        after_selection: SelectionSnapshot,
        before_revision: u64,
        after_revision: u64,
    ) -> Self {
        Self {
            changes: vec![change],
            before_selection,
            after_selection,
            before_revision,
            after_revision,
        }
    }

    fn append(
        &mut self,
        change: TextChange,
        after_selection: SelectionSnapshot,
        after_revision: u64,
    ) {
        self.changes = compress_consecutive_text_changes(&self.changes, &[change]);
        self.after_selection = after_selection;
        self.after_revision = after_revision;
    }
}

fn compress_consecutive_text_changes(
    prev_edits: &[TextChange],
    curr_edits: &[TextChange],
) -> Vec<TextChange> {
    if prev_edits.is_empty() {
        return curr_edits.to_vec();
    }
    TextChangeCompressor::new(prev_edits, curr_edits).compress()
}

struct TextChangeCompressor<'a> {
    prev_edits: &'a [TextChange],
    curr_edits: &'a [TextChange],
    result: Vec<TextChange>,
    prev_delta_offset: isize,
    curr_delta_offset: isize,
}

impl<'a> TextChangeCompressor<'a> {
    fn new(prev_edits: &'a [TextChange], curr_edits: &'a [TextChange]) -> Self {
        Self {
            prev_edits,
            curr_edits,
            result: Vec::new(),
            prev_delta_offset: 0,
            curr_delta_offset: 0,
        }
    }

    fn compress(mut self) -> Vec<TextChange> {
        let mut prev_index = 0usize;
        let mut curr_index = 0usize;

        let mut prev_edit = self.get_prev(prev_index);
        let mut curr_edit = self.get_curr(curr_index);

        while prev_index < self.prev_edits.len() || curr_index < self.curr_edits.len() {
            match (prev_edit.clone(), curr_edit.clone()) {
                (None, Some(curr)) => {
                    self.accept_curr(curr);
                    curr_index += 1;
                    curr_edit = self.get_curr(curr_index);
                }
                (Some(prev), None) => {
                    self.accept_prev(prev);
                    prev_index += 1;
                    prev_edit = self.get_prev(prev_index);
                }
                (Some(prev), Some(curr)) if curr.old_end() <= prev.new_position => {
                    self.accept_curr(curr);
                    curr_index += 1;
                    curr_edit = self.get_curr(curr_index);
                }
                (Some(prev), Some(curr)) if prev.new_end() <= curr.old_position => {
                    self.accept_prev(prev);
                    prev_index += 1;
                    prev_edit = self.get_prev(prev_index);
                }
                (Some(prev), Some(curr)) if curr.old_position < prev.new_position => {
                    let (head, tail) =
                        split_curr_change(&curr, prev.new_position - curr.old_position);
                    self.accept_curr(head);
                    curr_edit = Some(tail);
                }
                (Some(prev), Some(curr)) if prev.new_position < curr.old_position => {
                    let (head, tail) =
                        split_prev_change(&prev, curr.old_position - prev.new_position);
                    self.accept_prev(head);
                    prev_edit = Some(tail);
                }
                (Some(prev), Some(curr)) => {
                    let (merge_prev, merge_curr) = if curr.old_end() == prev.new_end() {
                        prev_index += 1;
                        curr_index += 1;
                        let next_prev = self.get_prev(prev_index);
                        let next_curr = self.get_curr(curr_index);
                        let pair = (prev, curr);
                        prev_edit = next_prev;
                        curr_edit = next_curr;
                        pair
                    } else if curr.old_end() < prev.new_end() {
                        let (head, tail) = split_prev_change(&prev, curr.old_text.len());
                        curr_index += 1;
                        prev_edit = Some(tail);
                        curr_edit = self.get_curr(curr_index);
                        (head, curr)
                    } else {
                        let (head, tail) = split_curr_change(&curr, prev.new_text.len());
                        prev_index += 1;
                        prev_edit = self.get_prev(prev_index);
                        curr_edit = Some(tail);
                        (prev, head)
                    };

                    self.result.push(TextChange {
                        old_position: merge_prev.old_position,
                        old_text: merge_prev.old_text.clone(),
                        new_position: merge_curr.new_position,
                        new_text: merge_curr.new_text.clone(),
                    });
                    self.prev_delta_offset += text_delta(&merge_prev);
                    self.curr_delta_offset += text_delta(&merge_curr);
                }
                (None, None) => break,
            }
        }

        remove_no_op_changes(merge_adjacent_changes(self.result))
    }

    fn get_prev(&self, index: usize) -> Option<TextChange> {
        self.prev_edits.get(index).cloned()
    }

    fn get_curr(&self, index: usize) -> Option<TextChange> {
        self.curr_edits.get(index).cloned()
    }

    fn accept_curr(&mut self, curr: TextChange) {
        self.result
            .push(rebase_curr_change(self.prev_delta_offset, curr.clone()));
        self.curr_delta_offset += text_delta(&curr);
    }

    fn accept_prev(&mut self, prev: TextChange) {
        self.result
            .push(rebase_prev_change(self.curr_delta_offset, prev.clone()));
        self.prev_delta_offset += text_delta(&prev);
    }
}

fn text_delta(change: &TextChange) -> isize {
    change.new_text.len() as isize - change.old_text.len() as isize
}

fn shift_offset(offset: usize, delta: isize) -> usize {
    if delta >= 0 {
        offset.saturating_add(delta as usize)
    } else {
        offset.saturating_sub(delta.unsigned_abs())
    }
}

fn rebase_curr_change(prev_delta_offset: isize, change: TextChange) -> TextChange {
    TextChange {
        old_position: shift_offset(change.old_position, -prev_delta_offset),
        ..change
    }
}

fn rebase_prev_change(curr_delta_offset: isize, change: TextChange) -> TextChange {
    TextChange {
        new_position: shift_offset(change.new_position, curr_delta_offset),
        ..change
    }
}

fn split_prev_change(change: &TextChange, offset: usize) -> (TextChange, TextChange) {
    let (pre_text, post_text) = split_text_at(&change.new_text, offset);
    let pre_len = pre_text.len();
    (
        TextChange {
            old_position: change.old_position,
            old_text: change.old_text.clone(),
            new_position: change.new_position,
            new_text: pre_text,
        },
        TextChange {
            old_position: change.old_end(),
            old_text: String::new(),
            new_position: change.new_position + pre_len,
            new_text: post_text,
        },
    )
}

fn split_curr_change(change: &TextChange, offset: usize) -> (TextChange, TextChange) {
    let (pre_text, post_text) = split_text_at(&change.old_text, offset);
    let pre_len = pre_text.len();
    (
        TextChange {
            old_position: change.old_position,
            old_text: pre_text,
            new_position: change.new_position,
            new_text: change.new_text.clone(),
        },
        TextChange {
            old_position: change.old_position + pre_len,
            old_text: post_text,
            new_position: change.new_end(),
            new_text: String::new(),
        },
    )
}

fn split_text_at(text: &str, offset: usize) -> (String, String) {
    let offset = clamp_to_char_boundary(text, offset.min(text.len()));
    (text[..offset].to_string(), text[offset..].to_string())
}

fn merge_adjacent_changes(changes: Vec<TextChange>) -> Vec<TextChange> {
    let mut iter = changes.into_iter();
    let Some(mut previous) = iter.next() else {
        return Vec::new();
    };
    let mut merged = Vec::new();
    for current in iter {
        if previous.old_end() == current.old_position {
            previous.old_text.push_str(&current.old_text);
            previous.new_text.push_str(&current.new_text);
        } else {
            merged.push(previous);
            previous = current;
        }
    }
    merged.push(previous);
    merged
}

fn remove_no_op_changes(changes: Vec<TextChange>) -> Vec<TextChange> {
    changes
        .into_iter()
        .filter(|change| change.old_text != change.new_text)
        .collect()
}

fn should_group_edit(content: &str, range: &Range<usize>, new_text: &str) -> bool {
    if range.is_empty() {
        return is_single_coalescible_char(new_text);
    }
    if new_text.is_empty() {
        if let Some(old_text) = content.get(range.clone()) {
            return is_single_coalescible_char(old_text);
        }
    }

    false
}

fn is_single_coalescible_char(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some(ch) if ch != '\n') && chars.next().is_none()
}

impl Buffer {
    fn new(id: BufferId, path: PathBuf, cx: &mut Context<Self>) -> Self {
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

/// Per-tab editor state: scroll handles for the two render modes. Each
/// editor tab gets its own so two tabs viewing the same buffer scroll
/// independently.
pub struct EditorView {
    focus_handle: FocusHandle,
    buffer: Option<Entity<Buffer>>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    is_selecting: bool,
    marked_range: Option<Range<usize>>,
    input_layout: Option<EditorInputLayout>,
    line_layouts: HashMap<usize, EditorLineInputLayout>,
    /// Scroll handle used by `uniform_list` in non-soft-wrap mode.
    uniform_scroll: UniformListScrollHandle,
    /// Scroll handle used by [`virtual_list`] in soft-wrap mode. Always
    /// present (cheap to construct), but only updated while soft-wrap is on.
    virtual_scroll: VirtualListState,
    /// EntityId of an external entity (typically a syntax snapshot) that the
    /// renderer has already wired up an observer for, used to avoid attaching
    /// a fresh observer on every render frame.
    observed_external: Option<EntityId>,
    /// Cached pixel width for the buffer's longest line, captured from
    /// gpui's `last_item_size` after the first real measurement. Subsequent
    /// frames hand gpui a width-only stub element instead of re-shaping the
    /// real line, which avoids per-frame text layout for files like
    /// pnpm-lock.yaml whose longest row is a 200+ character integrity hash.
    /// `Cell` so we can update it through `&EditorView` from the renderer.
    cached_longest_width: Cell<Option<Pixels>>,
    /// `rem_size` at which `cached_longest_width` was captured. If the user
    /// zooms (changes rem), the cached pixel width is wrong and we fall back
    /// to a real measurement until it stabilizes again.
    cached_longest_rem: Cell<Option<Pixels>>,
    editor_bounds: Option<Bounds<Pixels>>,
    gutter_hovered: bool,
    hovered_fold_line: Option<usize>,
    folded_lines: HashSet<usize>,
    hover_generation: u64,
    hover_hide_generation: u64,
    hover: Option<EditorHover>,
}

#[derive(Clone)]
pub struct EditorInputLayout {
    pub bounds: Bounds<Pixels>,
    pub visible_lines: Vec<usize>,
    pub row_height: Pixels,
    pub scroll_x: Pixels,
    pub scroll_y: Pixels,
    pub text_left: Pixels,
    pub char_width: Pixels,
}

#[derive(Clone)]
pub struct EditorLineInputLayout {
    pub line_index: usize,
    pub display_byte_offset: usize,
    pub text_layout: TextLayout,
}

#[derive(Clone, Debug)]
pub struct EditorHover {
    pub line_index: usize,
    pub byte_index: usize,
    pub byte_range: Range<usize>,
    pub generation: u64,
    pub hide_generation: u64,
    pub hide_pending: bool,
    pub source_highlight_visible: bool,
    pub source_bounds: Option<Bounds<Pixels>>,
    pub popup_bounds: Option<Bounds<Pixels>>,
    pub status: EditorHoverStatus,
}

#[derive(Clone, Debug)]
pub enum EditorHoverStatus {
    Loading,
    Ready(String),
    Empty,
    Error(String),
}

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
        for line_layout in self.line_layouts.values() {
            let bounds = line_layout.text_layout.bounds();
            if position.y < bounds.top() || position.y > bounds.bottom() {
                continue;
            }
            let Some(line_range) = buffer.line_range(line_layout.line_index) else {
                continue;
            };
            let line = &buffer.content()[line_range.clone()];
            let display = &line[line_layout.display_byte_offset.min(line.len())..];
            let display_offset = line_layout
                .text_layout
                .index_for_position(position)
                .unwrap_or_else(|offset| offset);
            let display_offset = clamp_to_char_boundary(display, display_offset.min(display.len()));
            return Some(line_range.start + line_layout.display_byte_offset + display_offset);
        }

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
        let x = (position.x - layout.bounds.left() - layout.text_left + layout.scroll_x)
            .max(Pixels::ZERO);
        let column = (x / layout.char_width).round() as usize;
        let line = &buffer.content()[line_range.clone()];
        Some(line_range.start + byte_for_visual_column(line, column))
    }

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

/// Single pass over `content` that produces the line-start byte offsets,
/// per-line character counts, and the index of the line with the most
/// characters.
fn analyze_content(content: &str) -> (Vec<usize>, Vec<usize>, usize) {
    let line_count_estimate = content.bytes().filter(|b| *b == b'\n').count() + 1;
    let mut starts = Vec::with_capacity(line_count_estimate);
    let mut chars_per_line = Vec::with_capacity(line_count_estimate);
    starts.push(0);
    let mut longest_index = 0usize;
    let mut longest_chars = 0usize;
    let mut current_line_index = 0usize;
    let mut current_chars = 0usize;
    for (byte_idx, ch) in content.char_indices() {
        if ch == '\n' {
            if current_chars > longest_chars {
                longest_chars = current_chars;
                longest_index = current_line_index;
            }
            chars_per_line.push(current_chars);
            starts.push(byte_idx + 1);
            current_line_index += 1;
            current_chars = 0;
        } else {
            current_chars += 1;
        }
    }
    chars_per_line.push(current_chars);
    if current_chars > longest_chars {
        longest_index = current_line_index;
    }
    (starts, chars_per_line, longest_index)
}

fn end_point(content: &str) -> Point {
    let mut row = 0usize;
    let mut column = 0usize;
    for byte in content.bytes() {
        if byte == b'\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Point { row, column }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn clamp_to_char_boundary(content: &str, mut offset: usize) -> usize {
    offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn clamp_range_to_char_boundaries(content: &str, range: Range<usize>) -> Range<usize> {
    let start = clamp_to_char_boundary(content, range.start);
    let end = clamp_to_char_boundary(content, range.end);
    start.min(end)..end.max(start)
}

fn point_for_offset(content: &str, offset: usize) -> Point {
    let mut row = 0usize;
    let mut column = 0usize;
    for byte in content.bytes().take(offset.min(content.len())) {
        if byte == b'\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Point { row, column }
}

fn advance_point(start: Point, text: &str) -> Point {
    let mut point = start;
    for byte in text.bytes() {
        if byte == b'\n' {
            point.row += 1;
            point.column = 0;
        } else {
            point.column += 1;
        }
    }
    point
}

fn offset_from_utf16(content: &str, offset: usize) -> usize {
    let mut utf8_offset = 0usize;
    let mut utf16_count = 0usize;
    for ch in content.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }
    utf8_offset
}

fn offset_to_utf16(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    let mut utf16_offset = 0usize;
    let mut utf8_count = 0usize;
    for ch in content.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

fn range_from_utf16(content: &str, range_utf16: &Range<usize>) -> Range<usize> {
    offset_from_utf16(content, range_utf16.start)..offset_from_utf16(content, range_utf16.end)
}

fn range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(content, range.start)..offset_to_utf16(content, range.end)
}

fn previous_char_boundary(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[..offset]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_char_boundary(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[offset..]
        .char_indices()
        .nth(1)
        .map_or(content.len(), |(index, _)| offset + index)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharacterClass {
    Whitespace,
    Word,
    Punctuation,
}

fn character_class(ch: char) -> CharacterClass {
    if ch.is_whitespace() {
        CharacterClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        CharacterClass::Word
    } else {
        CharacterClass::Punctuation
    }
}

fn char_at(content: &str, offset: usize) -> Option<char> {
    content.get(offset..)?.chars().next()
}

fn previous_word_boundary(content: &str, offset: usize) -> usize {
    let mut offset = clamp_to_char_boundary(content, offset);
    while offset > 0 {
        let previous = previous_char_boundary(content, offset);
        if char_at(content, previous).is_none_or(|ch| !ch.is_whitespace()) {
            break;
        }
        offset = previous;
    }

    let Some(class) =
        char_at(content, previous_char_boundary(content, offset)).map(character_class)
    else {
        return 0;
    };
    while offset > 0 {
        let previous = previous_char_boundary(content, offset);
        if char_at(content, previous).map(character_class) != Some(class) {
            break;
        }
        offset = previous;
    }
    offset
}

fn next_word_boundary(content: &str, offset: usize) -> usize {
    let mut offset = clamp_to_char_boundary(content, offset);
    while offset < content.len() {
        if char_at(content, offset).is_none_or(|ch| !ch.is_whitespace()) {
            break;
        }
        offset = next_char_boundary(content, offset);
    }

    let Some(class) = char_at(content, offset).map(character_class) else {
        return content.len();
    };
    while offset < content.len() {
        if char_at(content, offset).map(character_class) != Some(class) {
            break;
        }
        offset = next_char_boundary(content, offset);
    }
    offset
}

fn word_range_at_offset(content: &str, offset: usize) -> Option<Range<usize>> {
    if content.is_empty() {
        return None;
    }
    let mut target = clamp_to_char_boundary(content, offset.min(content.len()));
    if target == content.len() {
        target = previous_char_boundary(content, target);
    }
    let class = char_at(content, target).map(character_class)?;

    let mut start = target;
    while start > 0 {
        let previous = previous_char_boundary(content, start);
        if char_at(content, previous).map(character_class) != Some(class) {
            break;
        }
        start = previous;
    }

    let mut end = target;
    while end < content.len() {
        if char_at(content, end).map(character_class) != Some(class) {
            break;
        }
        let next = next_char_boundary(content, end);
        if next == end {
            break;
        }
        end = next;
    }
    (start < end).then_some(start..end)
}

fn line_start_for_offset(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[..offset].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end_for_offset(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[offset..]
        .find('\n')
        .map_or(content.len(), |index| offset + index)
}

fn line_range_including_newline_for_offset(content: &str, offset: usize) -> Range<usize> {
    let offset = clamp_to_char_boundary(content, offset);
    if offset == content.len() && offset > 0 && content.ends_with('\n') {
        let previous = previous_char_boundary(content, offset);
        return previous..offset;
    }

    let start = line_start_for_offset(content, offset);
    let end = line_end_for_offset(content, offset);
    let end = if end < content.len() {
        next_char_boundary(content, end)
    } else {
        end
    };
    start..end
}

fn line_range_for_selection(content: &str, selection: &Range<usize>) -> Range<usize> {
    if selection.is_empty() {
        return line_range_including_newline_for_offset(content, selection.start);
    }

    let start = line_start_for_offset(content, selection.start);
    let end_anchor = previous_char_boundary(content, selection.end);
    start..line_range_including_newline_for_offset(content, end_anchor).end
}

fn duplicate_text_for_line_range(content: &str, range: &Range<usize>, above: bool) -> String {
    let text = content.get(range.clone()).unwrap_or_default();
    if text.is_empty() {
        return "\n".to_string();
    }

    if text.ends_with('\n') {
        return text.to_string();
    }

    if above {
        format!("{text}\n")
    } else {
        format!("\n{text}")
    }
}

fn shift_selection(selection: &SelectionSnapshot, offset: usize) -> SelectionSnapshot {
    SelectionSnapshot {
        range: selection.range.start + offset..selection.range.end + offset,
        reversed: selection.reversed,
    }
}

fn line_column_for_offset(content: &str, offset: usize) -> (usize, usize) {
    let offset = clamp_to_char_boundary(content, offset);
    let mut row = 0usize;
    let mut line_start = 0usize;
    for (index, ch) in content.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            row += 1;
            line_start = index + ch.len_utf8();
        }
    }
    (row, content[line_start..offset].chars().count())
}

fn offset_for_line_column(content: &str, target_row: usize, target_column: usize) -> usize {
    let mut row = 0usize;
    let mut line_start = 0usize;
    for (index, ch) in content.char_indices() {
        if row == target_row {
            break;
        }
        if ch == '\n' {
            row += 1;
            line_start = index + ch.len_utf8();
        }
    }
    if row < target_row {
        return content.len();
    }
    let line_end = content[line_start..]
        .find('\n')
        .map_or(content.len(), |index| line_start + index);
    content[line_start..line_end]
        .char_indices()
        .nth(target_column)
        .map_or(line_end, |(index, _)| line_start + index)
}

fn byte_for_visual_column(line: &str, target_column: usize) -> usize {
    let mut column = 0usize;
    for (index, ch) in line.char_indices() {
        if column >= target_column {
            return index;
        }
        column += if ch == '\t' { 4 - (column % 4) } else { 1 };
        if column > target_column {
            return index + ch.len_utf8();
        }
    }
    line.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(
        old_position: usize,
        old_text: &str,
        new_position: usize,
        new_text: &str,
    ) -> TextChange {
        TextChange {
            old_position,
            old_text: old_text.to_string(),
            new_position,
            new_text: new_text.to_string(),
        }
    }

    fn selection(range: Range<usize>) -> SelectionSnapshot {
        SelectionSnapshot {
            range,
            reversed: false,
        }
    }

    #[test]
    fn compresses_adjacent_insertions() {
        assert_eq!(
            compress_consecutive_text_changes(&[change(0, "", 0, "a")], &[change(1, "", 1, "b")]),
            vec![change(0, "", 0, "ab")]
        );
    }

    #[test]
    fn compresses_repeated_backspace_deletions() {
        assert_eq!(
            compress_consecutive_text_changes(&[change(2, "c", 2, "")], &[change(1, "b", 1, "")]),
            vec![change(1, "bc", 1, "")]
        );
    }

    #[test]
    fn compresses_repeated_forward_deletions() {
        assert_eq!(
            compress_consecutive_text_changes(&[change(0, "a", 0, "")], &[change(0, "b", 0, "")]),
            vec![change(0, "ab", 0, "")]
        );
    }

    #[test]
    fn edit_stack_element_appends_like_open_monaco_stack_element() {
        let mut element = EditStackElement::new(
            change(0, "", 0, "a"),
            selection(0..0),
            selection(1..1),
            0,
            1,
        );

        element.append(change(1, "", 1, "b"), selection(2..2), 2);

        assert_eq!(element.changes, vec![change(0, "", 0, "ab")]);
        assert_eq!(element.before_selection, selection(0..0));
        assert_eq!(element.after_selection, selection(2..2));
        assert_eq!(element.before_revision, 0);
        assert_eq!(element.after_revision, 2);
    }

    #[test]
    fn groups_only_adjacent_single_character_typing_and_deleting() {
        assert!(should_group_edit("abc", &(1..1), "x"));
        assert!(should_group_edit("abc", &(1..2), ""));
        assert!(!should_group_edit("abc", &(1..1), "xy"));
        assert!(!should_group_edit("abc", &(1..1), "\n"));
        assert!(!should_group_edit("abc", &(1..2), "x"));
    }

    #[test]
    fn line_range_for_empty_selection_includes_newline() {
        assert_eq!(line_range_for_selection("one\ntwo\n", &(1..1)), 0..4);
        assert_eq!(line_range_for_selection("one\ntwo", &(5..5)), 4..7);
        assert_eq!(line_range_for_selection("one\n", &(4..4)), 3..4);
    }

    #[test]
    fn line_range_for_selection_covers_touched_lines() {
        assert_eq!(line_range_for_selection("one\ntwo\nthree", &(1..6)), 0..8);
        assert_eq!(line_range_for_selection("one\ntwo\nthree", &(0..4)), 0..4);
    }

    #[test]
    fn duplicate_text_adds_missing_line_break_at_file_edges() {
        assert_eq!(
            duplicate_text_for_line_range("one", &(0..3), false),
            "\none"
        );
        assert_eq!(duplicate_text_for_line_range("one", &(0..3), true), "one\n");
        assert_eq!(
            duplicate_text_for_line_range("one\n", &(0..4), false),
            "one\n"
        );
    }
}

/// Global cache that hands out (or creates) the `Buffer` entity for a given
/// path so all editor tabs viewing the same file share state. Dual-keyed
/// (`PathBuf` → `BufferId` → `Entity<Buffer>`) so subsystems that don't have
/// a path — scratch buffers, multi-root collisions, persisted analyses — can
/// look buffers up by their stable id.
#[derive(Default)]
pub struct BufferStore {
    next_id: u64,
    by_path: HashMap<PathBuf, BufferId>,
    by_id: HashMap<BufferId, Entity<Buffer>>,
}

impl BufferStore {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// Return the existing buffer for `path`, opening (and caching) one if
    /// none exists yet.
    pub fn open(path: PathBuf, cx: &mut App) -> Entity<Buffer> {
        if let Some(existing) = cx
            .try_global::<Self>()
            .and_then(|s| s.by_path.get(&path).and_then(|id| s.by_id.get(id)).cloned())
        {
            return existing;
        }
        let id = cx.update_global::<Self, _>(|store, _| {
            let id = BufferId(store.next_id);
            store.next_id += 1;
            id
        });
        let path_for_buffer = path.clone();
        let entity = cx.new(move |cx| Buffer::new(id, path_for_buffer, cx));
        cx.update_global::<Self, _>(|store, _| {
            store.by_path.insert(path, id);
            store.by_id.insert(id, entity.clone());
        });
        entity
    }

    pub fn get(id: BufferId, cx: &App) -> Option<Entity<Buffer>> {
        cx.try_global::<Self>()
            .and_then(|s| s.by_id.get(&id).cloned())
    }

    pub fn is_path_dirty(path: &Path, cx: &App) -> bool {
        cx.try_global::<Self>()
            .and_then(|store| {
                store
                    .by_path
                    .get(path)
                    .and_then(|id| store.by_id.get(id))
                    .cloned()
            })
            .is_some_and(|buffer| buffer.read(cx).is_dirty())
    }

    pub fn save_path(path: &Path, cx: &mut App) -> std::io::Result<bool> {
        let buffer = cx.try_global::<Self>().and_then(|store| {
            store
                .by_path
                .get(path)
                .and_then(|id| store.by_id.get(id))
                .cloned()
        });
        let Some(buffer) = buffer else {
            return Ok(false);
        };
        buffer.update(cx, |buffer, cx| buffer.save(cx))?;
        Ok(true)
    }

    pub fn reload_paths(paths: impl IntoIterator<Item = PathBuf>, cx: &mut App) {
        let buffers = cx
            .try_global::<Self>()
            .map(|store| {
                paths
                    .into_iter()
                    .filter_map(|path| {
                        store
                            .by_path
                            .get(&path)
                            .and_then(|id| store.by_id.get(id))
                            .cloned()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for buffer in buffers {
            buffer.update(cx, |buffer, cx| buffer.reload_from_disk(cx));
        }
    }
}

impl Global for BufferStore {}

/// Global cache of per-tab [`EditorView`]s keyed by tab id. Tabs created in a
/// `PaneTree` get unique ids that persist across pane moves, so a single
/// `usize` is enough to identify the view for a tab's lifetime.
#[derive(Default)]
pub struct EditorViewStore {
    views: HashMap<usize, Entity<EditorView>>,
}

impl EditorViewStore {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// Return the editor view for `tab_id`, creating one sized for `buffer`'s
    /// row count if it doesn't exist yet.
    pub fn for_tab(tab_id: usize, buffer: &Entity<Buffer>, cx: &mut App) -> Entity<EditorView> {
        if let Some(existing) = cx
            .try_global::<Self>()
            .and_then(|s| s.views.get(&tab_id).cloned())
        {
            return existing;
        }
        let row_count = buffer.read(cx).row_count();
        let entity = cx.new(|cx| EditorView::new(row_count, cx));
        cx.update_global::<Self, _>(|store, _| {
            store.views.insert(tab_id, entity.clone());
        });
        entity
    }

    /// Drop the cached view for `tab_id`. Call when a tab is closed so its
    /// scroll state isn't carried into a future tab that reuses the id.
    pub fn drop_tab(tab_id: usize, cx: &mut App) {
        if cx.try_global::<Self>().is_none() {
            return;
        }
        cx.update_global::<Self, _>(|store, _| {
            store.views.remove(&tab_id);
        });
    }
}

impl Global for EditorViewStore {}

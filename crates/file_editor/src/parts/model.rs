use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, EntityId, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Pixels, TextLayout, UTF16Selection, UniformListScrollHandle, Window,
    actions,
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

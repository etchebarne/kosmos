use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{App, Context, EventEmitter, FocusHandle, Focusable, actions};
use language::LanguageId;
use settings::{ActiveSettings, SettingValue};

actions!(file_editor, [Save]);

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
/// through gpui's [`EventEmitter`] so per-buffer subsystems can subscribe
/// without polling.
#[derive(Clone, Debug)]
pub enum BufferEvent {
    Edited { edits: Vec<TextEdit> },
    LanguageChanged,
}

pub const SOFT_WRAP_SETTING_ID: &str = "editor.soft_wrap";

/// Resolve `editor.soft_wrap` from the global settings, falling back to the
/// default declared in `settings::registry::EDITOR`.
pub fn soft_wrap_enabled(cx: &App) -> bool {
    cx.settings()
        .get(SOFT_WRAP_SETTING_ID)
        .and_then(SettingValue::as_bool)
        .unwrap_or(false)
}

/// In-memory view of a file open in an editor tab. Shared across all tabs
/// viewing the same path; per-tab editor state lives in the UI component store.
pub struct Buffer {
    id: BufferId,
    path: PathBuf,
    language: Option<LanguageId>,
    content: String,
    saved_content: String,
    dirty: bool,
    focus_handle: FocusHandle,
}

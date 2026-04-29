use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext, BorrowAppContext, Context, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable, Global, ListAlignment, ListState, Pixels, UniformListScrollHandle, px,
};
use language::LanguageId;
use settings::{ActiveSettings, SettingValue};

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

const LIST_OVERDRAW_PX: f32 = 200.0;

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
    line_starts: Vec<usize>,
    /// Index of the line with the most characters. Used by `uniform_list` as
    /// the row to measure when sizing the horizontal extent of the editor.
    longest_line_index: usize,
    focus_handle: FocusHandle,
}

impl Buffer {
    fn new(id: BufferId, path: PathBuf, cx: &mut Context<Self>) -> Self {
        let content = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let (line_starts, longest_line_index) = analyze_content(&content);
        let language = language::from_path(&path);
        Self {
            id,
            path,
            language,
            content,
            line_starts,
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
}

impl Focusable for Buffer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<BufferEvent> for Buffer {}

/// Per-tab editor state: scroll handles and list measurement caches. Each
/// editor tab gets its own so two tabs viewing the same buffer scroll
/// independently.
pub struct EditorView {
    row_count: usize,
    uniform_scroll: UniformListScrollHandle,
    /// Backing state for the variable-height `list` element used in soft-wrap
    /// mode. Constructed lazily on first use because building the underlying
    /// SumTree is O(row_count) — for a 34k-line file we don't want to pay
    /// that on tab open if soft-wrap is off (the default).
    list_state: Option<ListState>,
    list_state_soft_wrap: bool,
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
}

impl EditorView {
    pub fn new(row_count: usize) -> Self {
        Self {
            row_count,
            uniform_scroll: UniformListScrollHandle::new(),
            list_state: None,
            list_state_soft_wrap: false,
            observed_external: None,
            cached_longest_width: Cell::new(None),
            cached_longest_rem: Cell::new(None),
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

    /// Hand out the `ListState` for soft-wrap rendering, constructing it on
    /// first use. Forces a full re-measurement only when the wrap mode
    /// toggles. Width changes are intentionally **not** re-measured —
    /// gpui's `list` self-invalidates per-row heights on width change
    /// (list.rs:1025-1038) and re-measures visible rows incrementally as
    /// the user scrolls. Re-running `force_remeasure` here would walk all
    /// 34k rows synchronously on the main thread, freezing the UI for a
    /// second or two on every pane-resize release. The trade-off is that
    /// the scrollbar size is approximate right after a resize and grows
    /// toward accurate as the user scrolls through the file.
    pub fn list_state_for(&mut self, soft_wrap: bool) -> ListState {
        let list_state = self.list_state.get_or_insert_with(|| {
            // measure_all = list pre-measures every row up front so the
            // scrollbar size doesn't shift as more rows scroll into view.
            ListState::new(self.row_count, ListAlignment::Top, px(LIST_OVERDRAW_PX)).measure_all()
        });
        if self.list_state_soft_wrap != soft_wrap {
            Self::force_remeasure(list_state, self.row_count);
            self.list_state_soft_wrap = soft_wrap;
        }
        list_state.clone()
    }

    /// Reset the list state so the next layout pre-measures every row, while
    /// preserving the user's logical scroll position (item index + offset
    /// within that item) — `ListState::reset` clears it otherwise.
    fn force_remeasure(list_state: &ListState, row_count: usize) {
        let saved = list_state.logical_scroll_top();
        list_state.reset(row_count);
        list_state.scroll_to(saved);
    }

    /// Read-only snapshot of the current `ListState`. Used by overlays
    /// (e.g. the scrollbar) that want to inspect or drive scroll position
    /// without disturbing the cached row heights. Returns `None` when the
    /// list has never been used (soft-wrap has stayed off the whole time).
    pub fn list_state_snapshot(&self) -> Option<ListState> {
        self.list_state.clone()
    }
}

/// Single pass over `content` that produces both the line-start byte offsets
/// and the index of the line with the most characters.
fn analyze_content(content: &str) -> (Vec<usize>, usize) {
    let mut starts = Vec::with_capacity(content.bytes().filter(|b| *b == b'\n').count() + 1);
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
            starts.push(byte_idx + 1);
            current_line_index += 1;
            current_chars = 0;
        } else {
            current_chars += 1;
        }
    }
    if current_chars > longest_chars {
        longest_index = current_line_index;
    }
    (starts, longest_index)
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
        if let Some(existing) = cx.try_global::<Self>().and_then(|s| {
            s.by_path
                .get(&path)
                .and_then(|id| s.by_id.get(id))
                .cloned()
        }) {
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
        let entity = cx.new(|_| EditorView::new(row_count));
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

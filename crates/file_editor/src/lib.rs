use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext, BorrowAppContext, Context, Entity, FocusHandle, Focusable, Global,
    ListAlignment, ListState, Pixels, UniformListScrollHandle, px,
};
use settings::{ActiveSettings, SettingValue};

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
    path: PathBuf,
    content: String,
    line_starts: Vec<usize>,
    /// Index of the line with the most characters. Used by `uniform_list` as
    /// the row to measure when sizing the horizontal extent of the editor.
    longest_line_index: usize,
    focus_handle: FocusHandle,
}

impl Buffer {
    pub fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let content = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let (line_starts, longest_line_index) = analyze_content(&content);
        Self {
            path,
            content,
            line_starts,
            longest_line_index,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
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

/// Per-tab editor state: scroll handles and list measurement caches. Each
/// editor tab gets its own so two tabs viewing the same buffer scroll
/// independently.
pub struct EditorView {
    row_count: usize,
    uniform_scroll: UniformListScrollHandle,
    /// Backing state for the variable-height `list` element used in soft-wrap
    /// mode. Created up front; reset whenever the wrap mode toggles so cached
    /// row heights don't go stale.
    list_state: ListState,
    list_state_soft_wrap: bool,
    /// Last viewport width we saw the list rendered at. gpui's `list` element
    /// invalidates cached item heights when its width changes but does NOT
    /// re-trigger `measure_all`, so we have to detect the change ourselves
    /// and force a fresh full pre-measurement.
    list_state_known_width: Option<Pixels>,
}

impl EditorView {
    pub fn new(row_count: usize) -> Self {
        // measure_all = list pre-measures every row up front so the scrollbar
        // size doesn't shift as more rows scroll into view.
        let list_state =
            ListState::new(row_count, ListAlignment::Top, px(LIST_OVERDRAW_PX)).measure_all();
        Self {
            row_count,
            uniform_scroll: UniformListScrollHandle::new(),
            list_state,
            list_state_soft_wrap: false,
            list_state_known_width: None,
        }
    }

    pub fn uniform_scroll(&self) -> UniformListScrollHandle {
        self.uniform_scroll.clone()
    }

    /// Hand out the `ListState` for soft-wrap rendering. Triggers a full
    /// re-measurement when either the wrap mode toggles or the rendered
    /// width changes — both invalidate cached row heights, but gpui's
    /// `list` only re-runs `measure_all` if `has_measured` is back to false.
    pub fn list_state_for(&mut self, soft_wrap: bool) -> ListState {
        if self.list_state_soft_wrap != soft_wrap {
            self.force_remeasure();
            self.list_state_soft_wrap = soft_wrap;
            self.list_state_known_width = None;
        }
        let current_width = self.list_state.viewport_bounds().size.width;
        if current_width > px(0.0) && Some(current_width) != self.list_state_known_width {
            self.force_remeasure();
            self.list_state_known_width = Some(current_width);
        }
        self.list_state.clone()
    }

    /// Reset the list state so the next layout pre-measures every row, while
    /// preserving the user's logical scroll position (item index + offset
    /// within that item) — `ListState::reset` clears it otherwise.
    fn force_remeasure(&mut self) {
        let saved = self.list_state.logical_scroll_top();
        self.list_state.reset(self.row_count);
        self.list_state.scroll_to(saved);
    }

    /// Read-only snapshot of the current `ListState`. Used by overlays
    /// (e.g. the scrollbar) that want to inspect or drive scroll position
    /// without disturbing the cached row heights.
    pub fn list_state_snapshot(&self) -> ListState {
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
/// path so all editor tabs viewing the same file share state.
#[derive(Default)]
pub struct BufferStore {
    buffers: HashMap<PathBuf, Entity<Buffer>>,
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
            .and_then(|s| s.buffers.get(&path).cloned())
        {
            return existing;
        }
        let path_for_buffer = path.clone();
        let entity = cx.new(move |cx| Buffer::new(path_for_buffer, cx));
        cx.update_global::<Self, _>(|store, _| {
            store.buffers.insert(path, entity.clone());
        });
        entity
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

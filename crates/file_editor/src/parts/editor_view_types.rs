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

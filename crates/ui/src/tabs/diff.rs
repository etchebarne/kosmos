use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Entity, Global, HighlightStyle, IntoElement, ListSizingBehavior,
    ScrollStrategy, SharedString, StyledText, Task, Window, div, prelude::*, px, rems, size,
};
use gpui_component::{
    VirtualListScrollHandle, WindowExt,
    highlighter::{HighlightTheme, SyntaxHighlighter},
    input::Rope,
    notification::Notification,
    v_virtual_list,
};
use icons::{Icon, IconName};
use kosmos_git::{
    ConflictBlock, ConflictLine, ConflictResolution, DiffLine, DiffLineKind, FileChangeKind,
    RepositoryDiff,
};
use settings::{ActiveSettings, SettingValue};
use tabs::Tab;
use theme::ActiveTheme;

use file_editor::BufferStore;

use crate::delegate::{PaneDelegate, SettingsDelegate};

const FONT_FAMILY: &str = "DejaVu Sans Mono";
const DIFF_FILE_ROW_HEIGHT_REM: f32 = 2.375;
const DIFF_ROW_HEIGHT_REM: f32 = 1.25;
const DIFF_GAP_ROW_HEIGHT_REM: f32 = 2.0;
const LINE_NUMBER_WIDTH_REM: f32 = 3.0;
const SIGN_WIDTH_REM: f32 = 1.25;
const CARD_MARGIN_X_REM: f32 = 0.5;
const DIFF_CODE_TEXT_SIZE_REM: f32 = 0.8125;
const DIFF_MONO_CHAR_WIDTH_FACTOR: f32 = 0.62;

type LineHighlightKey = (u8, Option<usize>, Option<usize>);
type LineHighlights = Vec<(Range<usize>, HighlightStyle)>;

struct DiffUiState {
    root: Option<PathBuf>,
    diff: Option<RepositoryDiff>,
    loading: bool,
    error: Option<String>,
    generation: u64,
    task: Option<Task<()>>,
    scroll_handle: VirtualListScrollHandle,
    pending_focus: Option<String>,
    active_file: Option<String>,
    highlight_cache: Rc<DiffHighlightCache>,
    wrap_width_px: Option<f32>,
}

impl Default for DiffUiState {
    fn default() -> Self {
        Self {
            root: None,
            diff: None,
            loading: false,
            error: None,
            generation: 0,
            task: None,
            scroll_handle: VirtualListScrollHandle::new(),
            pending_focus: None,
            active_file: None,
            highlight_cache: Rc::default(),
            wrap_width_px: None,
        }
    }
}

impl Global for DiffUiState {}

#[derive(Clone, Default)]
struct DiffHighlightCache {
    fingerprint: Option<u64>,
    theme_id: &'static str,
    lines: HashMap<String, HashMap<LineHighlightKey, LineHighlights>>,
}

impl DiffHighlightCache {
    fn matches(&self, fingerprint: u64, theme_id: &'static str) -> bool {
        self.fingerprint == Some(fingerprint) && self.theme_id == theme_id
    }

    fn highlights(&self, path: &str, key: &LineHighlightKey) -> Option<&[LineHighlightsItem]> {
        self.lines
            .get(path)
            .and_then(|lines| lines.get(key))
            .map(Vec::as_slice)
    }
}

type LineHighlightsItem = (Range<usize>, HighlightStyle);

#[derive(Clone)]
enum DiffRow {
    File {
        path: String,
        old_path: Option<String>,
        kind: FileChangeKind,
        insertions: usize,
        deletions: usize,
        binary: bool,
    },
    Unchanged {
        path: String,
        lines: usize,
    },
    Line {
        path: String,
        can_open: bool,
        line: DiffLine,
    },
    ConflictActions {
        path: String,
        start_line: usize,
        current_label: String,
        incoming_label: String,
    },
    ConflictMarker {
        path: String,
        line: usize,
        text: String,
        side: Option<ConflictSide>,
    },
    ConflictLine {
        path: String,
        side: ConflictSide,
        line: ConflictLine,
    },
    Message {
        path: String,
        text: String,
    },
}

#[derive(Clone, Copy)]
enum HighlightSide {
    Old,
    New,
}

#[derive(Clone, Copy)]
enum ConflictSide {
    Current,
    Incoming,
}

pub fn render<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    workspace_path: &Path,
    tab: &Tab,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let root = tab.path.as_deref().unwrap_or(workspace_path).to_path_buf();
    ensure_diff(&root, cx);

    let (diff, loading, error, scroll_handle, active_file) = {
        let state = cx.global::<DiffUiState>();
        (
            state.diff.clone(),
            state.loading,
            state.error.clone(),
            state.scroll_handle.clone(),
            state.active_file.clone(),
        )
    };
    let rows = diff.as_ref().map(flatten_diff).unwrap_or_default();
    apply_pending_focus(&root, &rows, cx);
    let open_root = diff
        .as_ref()
        .map(|diff| diff.work_dir.clone())
        .unwrap_or_else(|| root.clone());
    let theme = *cx.theme();
    let highlight_cache = diff
        .as_ref()
        .map(|diff| ensure_highlight_cache(diff, theme.id, cx))
        .unwrap_or_default();
    let soft_wrap = diff_soft_wrap_enabled(cx);
    let rows = Rc::new(rows);
    let delegate = cx.entity().clone();

    div()
        .size_full()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .child(match (loading, error, diff) {
            (true, _, None) => centered_state("Loading diff", cx),
            (_, Some(error), None) => centered_state(error, cx),
            (_, _, Some(diff)) if diff.is_empty() => centered_state("No changes", cx),
            (_, _, _) if rows.is_empty() => centered_state("No textual changes", cx),
            _ => diff_rows(
                open_root,
                rows,
                scroll_handle,
                highlight_cache,
                delegate,
                active_file,
                soft_wrap,
                window,
                cx,
            ),
        })
        .into_any_element()
}

pub fn request_focus(root: PathBuf, file_path: String, cx: &mut App) {
    ensure_state(cx);
    cx.update_global::<DiffUiState, _>(|state, _| {
        if state.root.as_ref() != Some(&root) {
            state.root = Some(root);
            state.diff = None;
            state.error = None;
            state.loading = false;
            state.generation = state.generation.wrapping_add(1);
        }
        state.pending_focus = Some(file_path.clone());
        state.active_file = Some(file_path);
    });
    cx.refresh_windows();
}

fn ensure_state(cx: &mut App) {
    if cx.try_global::<DiffUiState>().is_none() {
        cx.set_global(DiffUiState::default());
    }
}

fn ensure_diff<T: PaneDelegate + SettingsDelegate>(root: &Path, cx: &mut Context<T>) {
    ensure_state(cx);
    let needs_refresh = {
        let state = cx.global::<DiffUiState>();
        state.root.as_deref() != Some(root)
            || (!state.loading && state.diff.is_none() && state.error.is_none())
    };
    if needs_refresh {
        refresh_diff(root.to_path_buf(), false, cx);
    }
}

pub fn refresh_if_loaded<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    notify_now: bool,
    cx: &mut Context<T>,
) {
    if should_refresh_loaded(&root, cx) {
        refresh_diff(root, notify_now, cx);
    }
}

pub fn refresh_if_loaded_app(root: PathBuf, notify_now: bool, cx: &mut App) {
    if !should_refresh_loaded(&root, cx) {
        return;
    }

    let generation = cx.update_global::<DiffUiState, _>(|state, _| {
        state.loading = true;
        state.generation = state.generation.wrapping_add(1);
        state.generation
    });

    if notify_now {
        cx.refresh_windows();
    }

    let task_root = root.clone();
    let task = cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move { RepositoryDiff::discover(task_root) })
            .await;
        let _ = cx.update(|cx| {
            apply_diff_result(&root, generation, result, cx);
            cx.refresh_windows();
        });
    });

    cx.update_global::<DiffUiState, _>(|state, _| {
        state.task = Some(task);
    });
}

fn should_refresh_loaded(root: &Path, cx: &App) -> bool {
    cx.try_global::<DiffUiState>().is_some_and(|state| {
        state.root.as_deref() == Some(root)
            && (state.diff.is_some()
                || state.error.is_some()
                || state.loading
                || state.active_file.is_some()
                || state.pending_focus.is_some())
    })
}

fn refresh_diff<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    notify_now: bool,
    cx: &mut Context<T>,
) {
    let generation = cx.update_global::<DiffUiState, _>(|state, _| {
        if state.root.as_ref() != Some(&root) {
            state.diff = None;
            state.error = None;
        }
        state.root = Some(root.clone());
        state.loading = true;
        state.generation = state.generation.wrapping_add(1);
        state.generation
    });

    if notify_now {
        cx.notify();
    }

    let task_root = root.clone();
    let task = cx.spawn(async move |this, cx| {
        let result = cx
            .background_executor()
            .spawn(async move { RepositoryDiff::discover(task_root) })
            .await;
        let _ = this.update(cx, |_, cx| {
            apply_diff_result(&root, generation, result, cx);
            cx.notify();
        });
    });

    cx.update_global::<DiffUiState, _>(|state, _| {
        state.task = Some(task);
    });
}

fn apply_diff_result(
    root: &Path,
    generation: u64,
    result: Result<RepositoryDiff, kosmos_git::Error>,
    cx: &mut App,
) {
    cx.update_global::<DiffUiState, _>(|state, _| {
        if state.generation != generation || state.root.as_deref() != Some(root) {
            return;
        }
        state.loading = false;
        match result {
            Ok(diff) => {
                state.diff = Some(diff);
                state.error = None;
            }
            Err(error) => {
                state.diff = None;
                state.error = Some(error.to_string());
            }
        }
    });
}

fn apply_pending_focus<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    rows: &[DiffRow],
    cx: &mut Context<T>,
) {
    cx.update_global::<DiffUiState, _>(|state, _| {
        if state.root.as_deref() != Some(root) {
            return;
        }
        let Some(path) = state.pending_focus.clone() else {
            return;
        };
        let Some(ix) = rows.iter().position(|row| row.path() == path.as_str()) else {
            return;
        };
        state.scroll_handle.scroll_to_item(ix, ScrollStrategy::Top);
        state.pending_focus = None;
        state.active_file = Some(path);
    });
}

fn diff_soft_wrap_enabled<T>(cx: &mut Context<T>) -> bool {
    cx.settings()
        .get(file_editor::SOFT_WRAP_SETTING_ID)
        .and_then(SettingValue::as_bool)
        .unwrap_or(false)
}

fn ensure_highlight_cache<T: PaneDelegate + SettingsDelegate>(
    diff: &RepositoryDiff,
    theme_id: &'static str,
    cx: &mut Context<T>,
) -> Rc<DiffHighlightCache> {
    ensure_state(cx);
    let fingerprint = diff_fingerprint(diff);
    if let Some(cache) = {
        let state = cx.global::<DiffUiState>();
        state
            .highlight_cache
            .matches(fingerprint, theme_id)
            .then(|| state.highlight_cache.clone())
    } {
        return cache;
    }

    let highlight_theme = gpui_component::Theme::global(cx).highlight_theme.clone();
    let cache = Rc::new(build_diff_highlight_cache(
        diff,
        fingerprint,
        theme_id,
        &highlight_theme,
    ));
    cx.update_global::<DiffUiState, _>(|state, _| {
        state.highlight_cache = cache.clone();
    });
    cache
}

fn build_diff_highlight_cache(
    diff: &RepositoryDiff,
    fingerprint: u64,
    theme_id: &'static str,
    highlight_theme: &HighlightTheme,
) -> DiffHighlightCache {
    let mut files = HashMap::new();

    for file in &diff.files {
        if file.binary || (file.hunks.is_empty() && file.conflicts.is_empty()) {
            continue;
        }

        let language = language::from_path(Path::new(&file.path))
            .map(|language| component_highlighter_language(language.as_str()).to_string())
            .unwrap_or_else(|| "plaintext".to_string());
        let mut line_highlights = HashMap::new();
        highlight_file_side(
            file,
            HighlightSide::Old,
            &language,
            highlight_theme,
            &mut line_highlights,
        );
        highlight_file_side(
            file,
            HighlightSide::New,
            &language,
            highlight_theme,
            &mut line_highlights,
        );
        highlight_conflict_side(
            file,
            ConflictSide::Current,
            &language,
            highlight_theme,
            &mut line_highlights,
        );
        highlight_conflict_side(
            file,
            ConflictSide::Incoming,
            &language,
            highlight_theme,
            &mut line_highlights,
        );

        if !line_highlights.is_empty() {
            files.insert(file.path.clone(), line_highlights);
        }
    }

    DiffHighlightCache {
        fingerprint: Some(fingerprint),
        theme_id,
        lines: files,
    }
}

fn highlight_conflict_side(
    file: &kosmos_git::FileDiff,
    side: ConflictSide,
    language: &str,
    highlight_theme: &HighlightTheme,
    line_highlights: &mut HashMap<LineHighlightKey, LineHighlights>,
) {
    let mut source = String::new();
    let mut line_ranges = Vec::new();

    for conflict in &file.conflicts {
        let lines = match side {
            ConflictSide::Current => &conflict.current,
            ConflictSide::Incoming => &conflict.incoming,
        };
        for line in lines {
            let start = source.len();
            source.push_str(&line.text);
            let end = source.len();
            line_ranges.push((conflict_line_highlight_key(side, line.line), start..end));
            source.push('\n');
        }
    }

    if source.is_empty() {
        return;
    }

    let rope = Rope::from_str(&source);
    let mut highlighter = SyntaxHighlighter::new(language);
    let _ = highlighter.update(None, &rope, None);
    let styles = highlighter.styles(&(0..source.len()), highlight_theme);

    for (key, range) in line_ranges {
        let highlights = highlights_for_line(&styles, range);
        if !highlights.is_empty() {
            line_highlights.insert(key, highlights);
        }
    }
}

fn highlight_file_side(
    file: &kosmos_git::FileDiff,
    side: HighlightSide,
    language: &str,
    highlight_theme: &HighlightTheme,
    line_highlights: &mut HashMap<LineHighlightKey, LineHighlights>,
) {
    let mut source = String::new();
    let mut line_ranges = Vec::new();

    for hunk in &file.hunks {
        for line in &hunk.lines {
            let include = match side {
                HighlightSide::Old => line.old_line.is_some(),
                HighlightSide::New => line.new_line.is_some(),
            };
            if !include {
                continue;
            }

            let start = source.len();
            source.push_str(&line.text);
            let end = source.len();
            line_ranges.push((line_highlight_key(line), start..end));
            source.push('\n');
        }
    }

    if source.is_empty() {
        return;
    }

    let rope = Rope::from_str(&source);
    let mut highlighter = SyntaxHighlighter::new(language);
    let _ = highlighter.update(None, &rope, None);
    let styles = highlighter.styles(&(0..source.len()), highlight_theme);

    for (key, range) in line_ranges {
        let highlights = highlights_for_line(&styles, range);
        if !highlights.is_empty() {
            line_highlights.insert(key, highlights);
        }
    }
}

fn highlights_for_line(styles: &[LineHighlightsItem], line_range: Range<usize>) -> LineHighlights {
    if line_range.is_empty() {
        return Vec::new();
    }

    styles
        .iter()
        .filter_map(|(range, style)| {
            let start = range.start.max(line_range.start);
            let end = range.end.min(line_range.end);
            (start < end).then(|| {
                (
                    start - line_range.start..end - line_range.start,
                    style.clone(),
                )
            })
        })
        .collect()
}

fn highlighted_code_text(text: String, highlights: Option<&[LineHighlightsItem]>) -> AnyElement {
    match highlights {
        Some(highlights) if !highlights.is_empty() => StyledText::new(text)
            .with_highlights(highlights.iter().cloned())
            .into_any_element(),
        _ => StyledText::new(text).into_any_element(),
    }
}

fn diff_fingerprint(diff: &RepositoryDiff) -> u64 {
    let mut hasher = DefaultHasher::new();
    diff.work_dir.hash(&mut hasher);
    diff.files.len().hash(&mut hasher);
    for file in &diff.files {
        file.old_path.hash(&mut hasher);
        file.path.hash(&mut hasher);
        file_change_kind_key(file.kind).hash(&mut hasher);
        file.binary.hash(&mut hasher);
        file.hunks.len().hash(&mut hasher);
        for hunk in &file.hunks {
            hunk.old_start.hash(&mut hasher);
            hunk.old_lines.hash(&mut hasher);
            hunk.new_start.hash(&mut hasher);
            hunk.new_lines.hash(&mut hasher);
            hunk.header.hash(&mut hasher);
            hunk.lines.len().hash(&mut hasher);
            for line in &hunk.lines {
                diff_line_kind_key(line.kind).hash(&mut hasher);
                line.old_line.hash(&mut hasher);
                line.new_line.hash(&mut hasher);
                line.text.hash(&mut hasher);
            }
        }
        file.conflicts.len().hash(&mut hasher);
        for conflict in &file.conflicts {
            conflict.start_line.hash(&mut hasher);
            conflict.separator_line.hash(&mut hasher);
            conflict.end_line.hash(&mut hasher);
            conflict.current_label.hash(&mut hasher);
            conflict.incoming_label.hash(&mut hasher);
            for line in &conflict.current {
                line.line.hash(&mut hasher);
                line.text.hash(&mut hasher);
            }
            for line in &conflict.incoming {
                line.line.hash(&mut hasher);
                line.text.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn file_change_kind_key(kind: FileChangeKind) -> u8 {
    match kind {
        FileChangeKind::Created => 0,
        FileChangeKind::Modified => 1,
        FileChangeKind::Deleted => 2,
        FileChangeKind::Renamed => 3,
        FileChangeKind::Conflicted => 4,
    }
}

fn line_highlight_key(line: &DiffLine) -> LineHighlightKey {
    (diff_line_kind_key(line.kind), line.old_line, line.new_line)
}

fn conflict_line_highlight_key(side: ConflictSide, line: usize) -> LineHighlightKey {
    let side_key = match side {
        ConflictSide::Current => 3,
        ConflictSide::Incoming => 4,
    };
    (side_key, None, Some(line))
}

fn diff_line_kind_key(kind: DiffLineKind) -> u8 {
    match kind {
        DiffLineKind::Context => 0,
        DiffLineKind::Added => 1,
        DiffLineKind::Removed => 2,
    }
}

fn component_highlighter_language(language: &str) -> &str {
    match language {
        "shellscript" => "bash",
        "typescriptreact" => "tsx",
        "javascriptreact" => "javascript",
        other => other,
    }
}

fn accept_conflict<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    path: String,
    start_line: usize,
    resolution: ConflictResolution,
    cx: &mut Context<T>,
) {
    let absolute_path = root.join(&path);
    match resolve_conflict_at_path(&absolute_path, start_line, resolution, cx) {
        Ok(()) => {
            push_diff_notification(cx, Notification::success("Conflict updated"));
            refresh_diff(root, true, cx);
        }
        Err(error) => {
            push_diff_notification(
                cx,
                Notification::error(error).title("Resolve conflict failed"),
            );
        }
    }
}

fn resolve_conflict_at_path<T: PaneDelegate + SettingsDelegate>(
    path: &Path,
    start_line: usize,
    resolution: ConflictResolution,
    cx: &mut Context<T>,
) -> Result<(), String> {
    let content = BufferStore::content_for_path(path, cx)
        .or_else(|| std::fs::read_to_string(path).ok())
        .ok_or_else(|| format!("Could not read {}", path.display()))?;
    let resolved = kosmos_git::resolve_conflict_content(&content, start_line, resolution)
        .ok_or_else(|| "That conflict block no longer exists".to_string())?;

    BufferStore::write_path_content(path, resolved, cx)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))
}

fn push_diff_notification(cx: &mut App, notification: Notification) {
    let Some(window_handle) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return;
    };

    let _ = window_handle.update(cx, move |_, window, cx| {
        window.push_notification(notification, cx);
    });
}

fn diff_rows<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    root: PathBuf,
    rows: Rc<Vec<DiffRow>>,
    scroll_handle: VirtualListScrollHandle,
    highlight_cache: Rc<DiffHighlightCache>,
    delegate: Entity<T>,
    active_file: Option<String>,
    soft_wrap: bool,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let rem_size = window.rem_size();
    let measured_width = cx.global::<DiffUiState>().wrap_width_px;
    let wrap_columns = diff_wrap_columns(window, measured_width);
    let item_sizes = Rc::new(
        rows.iter()
            .map(|row| {
                size(
                    px(0.0),
                    rems(diff_row_height(row, soft_wrap, wrap_columns)).to_pixels(rem_size),
                )
            })
            .collect::<Vec<_>>(),
    );

    let list = v_virtual_list(cx.entity().clone(), "diff-rows", item_sizes, {
        move |_, range, _window, cx| {
            range
                .map(|ix| {
                    render_diff_row(
                        ix,
                        rows[ix].clone(),
                        root.clone(),
                        highlight_cache.clone(),
                        delegate.clone(),
                        active_file.clone(),
                        soft_wrap,
                        wrap_columns,
                        cx,
                    )
                })
                .collect::<Vec<_>>()
        }
    })
    .flex_grow()
    .size_full()
    .track_scroll(&scroll_handle)
    .with_sizing_behavior(ListSizingBehavior::Auto);

    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .child(list)
        .on_children_prepainted(|bounds, _, cx| {
            let Some(bounds) = bounds.first().copied() else {
                return;
            };
            update_diff_wrap_width(f32::from(bounds.size.width), cx);
        })
        .into_any_element()
}

fn diff_row_height(row: &DiffRow, soft_wrap: bool, wrap_columns: usize) -> f32 {
    match row {
        DiffRow::File { .. } => DIFF_FILE_ROW_HEIGHT_REM,
        DiffRow::Unchanged { .. } => DIFF_GAP_ROW_HEIGHT_REM,
        DiffRow::ConflictActions { .. } => DIFF_ROW_HEIGHT_REM,
        DiffRow::ConflictMarker { text, .. } => diff_text_row_height(text, soft_wrap, wrap_columns),
        DiffRow::ConflictLine { line, .. } => {
            diff_text_row_height(&line.text, soft_wrap, wrap_columns)
        }
        DiffRow::Message { text, .. } => diff_text_row_height(text, soft_wrap, wrap_columns),
        DiffRow::Line { line, .. } => diff_text_row_height(&line.text, soft_wrap, wrap_columns),
    }
}

fn diff_text_row_height(text: &str, soft_wrap: bool, wrap_columns: usize) -> f32 {
    DIFF_ROW_HEIGHT_REM * diff_wrap_rows(text, soft_wrap, wrap_columns) as f32
}

fn diff_wrap_rows(text: &str, soft_wrap: bool, wrap_columns: usize) -> usize {
    if !soft_wrap {
        return 1;
    }

    let limit = wrap_columns.max(1);
    let mut rows = 1;
    let mut width = 0;

    for ch in text.chars() {
        let ch_width = if ch == '\t' { 4 } else { 1 };
        if width > 0 && width + ch_width > limit {
            rows += 1;
            width = 0;
        }
        width += ch_width;
    }

    rows
}

fn update_diff_wrap_width(width_px: f32, cx: &mut App) {
    let changed = cx.update_global::<DiffUiState, _>(|state, _| {
        let width_px = width_px.round();
        let changed = state
            .wrap_width_px
            .is_none_or(|current| (current - width_px).abs() >= 1.0);
        if changed {
            state.wrap_width_px = Some(width_px);
        }
        changed
    });

    if changed {
        cx.refresh_windows();
    }
}

#[derive(Clone)]
struct DiffTextSegment {
    text: String,
    range: Range<usize>,
}

fn wrap_diff_text(text: &str, soft_wrap: bool, wrap_columns: usize) -> Vec<DiffTextSegment> {
    if !soft_wrap || text.is_empty() {
        return vec![DiffTextSegment {
            text: text.to_string(),
            range: 0..text.len(),
        }];
    }

    let limit = wrap_columns.max(1);
    let mut segments = Vec::new();
    let mut start = 0;
    let mut width = 0;

    for (ix, ch) in text.char_indices() {
        let ch_width = if ch == '\t' { 4 } else { 1 };
        if width > 0 && width + ch_width > limit {
            segments.push(DiffTextSegment {
                text: text[start..ix].to_string(),
                range: start..ix,
            });
            start = ix;
            width = 0;
        }
        width += ch_width;
    }

    segments.push(DiffTextSegment {
        text: text[start..].to_string(),
        range: start..text.len(),
    });
    segments
}

fn highlights_for_segment(
    highlights: Option<&[LineHighlightsItem]>,
    segment: &Range<usize>,
) -> LineHighlights {
    let Some(highlights) = highlights else {
        return Vec::new();
    };

    highlights
        .iter()
        .filter_map(|(range, style)| {
            let start = range.start.max(segment.start);
            let end = range.end.min(segment.end);
            (start < end).then(|| (start - segment.start..end - segment.start, style.clone()))
        })
        .collect()
}

fn diff_wrap_columns(window: &Window, measured_width: Option<f32>) -> usize {
    let rem_size = window.rem_size();
    let reserved =
        rems(CARD_MARGIN_X_REM * 2.0 + LINE_NUMBER_WIDTH_REM + SIGN_WIDTH_REM + 0.125 + 1.0)
            .to_pixels(rem_size);
    let width = measured_width
        .map(px)
        .unwrap_or_else(|| window.bounds().size.width);
    let available = (width - reserved).max(px(120.0));
    let char_width = f32::from(rem_size) * DIFF_CODE_TEXT_SIZE_REM * DIFF_MONO_CHAR_WIDTH_FACTOR;

    (f32::from(available) / char_width).floor().max(20.0) as usize
}

fn render_diff_row<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    row: DiffRow,
    root: PathBuf,
    highlight_cache: Rc<DiffHighlightCache>,
    delegate: Entity<T>,
    active_file: Option<String>,
    soft_wrap: bool,
    wrap_columns: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    match row {
        DiffRow::File {
            path,
            old_path,
            kind,
            insertions,
            deletions,
            binary,
        } => render_file_row(
            ix,
            path,
            old_path,
            kind,
            insertions,
            deletions,
            binary,
            root,
            delegate,
            active_file,
            cx,
        ),
        DiffRow::Unchanged { lines, .. } => render_unchanged_row(ix, lines, cx),
        DiffRow::Line {
            path,
            can_open,
            line,
        } => render_line_row(
            ix,
            path,
            can_open,
            line,
            root,
            highlight_cache,
            delegate,
            soft_wrap,
            wrap_columns,
            cx,
        ),
        DiffRow::ConflictActions {
            path,
            start_line,
            current_label,
            incoming_label,
        } => render_conflict_actions_row(
            ix,
            root,
            path,
            start_line,
            current_label,
            incoming_label,
            cx,
        ),
        DiffRow::ConflictMarker {
            line, text, side, ..
        } => render_conflict_marker_row(ix, line, text, side, soft_wrap, wrap_columns, cx),
        DiffRow::ConflictLine { path, side, line } => render_conflict_line_row(
            ix,
            path,
            side,
            line,
            root,
            highlight_cache,
            delegate,
            soft_wrap,
            wrap_columns,
            cx,
        ),
        DiffRow::Message { text, .. } => render_message_row(ix, text, soft_wrap, wrap_columns, cx),
    }
}

fn render_file_row<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    path: String,
    old_path: Option<String>,
    kind: FileChangeKind,
    insertions: usize,
    deletions: usize,
    binary: bool,
    root: PathBuf,
    delegate: Entity<T>,
    active_file: Option<String>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let display_path = old_path
        .as_ref()
        .filter(|old_path| old_path.as_str() != path.as_str())
        .map(|old_path| format!("{old_path} -> {path}"))
        .unwrap_or_else(|| path.clone());
    let icon = icon_for_path(Path::new(&path));
    let absolute_path = root.join(&path);
    let can_open = kind != FileChangeKind::Deleted && absolute_path.exists();
    let open_path = absolute_path.clone();
    let is_active = active_file.as_deref() == Some(path.as_str());

    div()
        .id(("diff-file", ix))
        .mx(rems(CARD_MARGIN_X_REM))
        .h(rems(DIFF_FILE_ROW_HEIGHT_REM))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .border_1()
        .border_color(theme.border_subtle)
        .bg(if is_active {
            theme.bg_selected
        } else {
            theme.bg_elevated
        })
        .hover(move |this| this.bg(theme.bg_hover))
        .when(can_open, |this| {
            this.cursor_pointer().on_click(move |_, _, cx| {
                let path = open_path.clone();
                delegate.update(cx, |delegate, cx| delegate.open_file(path, cx));
            })
        })
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(icon)
                        .size(14.0)
                        .color(file_icon_color(kind, theme)),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_sm()
                        .text_color(theme.text_emphasis)
                        .child(display_path),
                )
                .when(kind != FileChangeKind::Modified || binary, |this| {
                    this.child(change_kind_tag(kind, binary, theme))
                }),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .child(diff_stats(insertions, deletions, theme)),
        )
        .into_any_element()
}

fn render_unchanged_row<T: 'static>(ix: usize, lines: usize, cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(("diff-unchanged", ix))
        .mx(rems(CARD_MARGIN_X_REM))
        .h(rems(DIFF_GAP_ROW_HEIGHT_REM))
        .w_full()
        .flex()
        .items_center()
        .overflow_hidden()
        .bg(gpui::Hsla::from(theme.bg_hover).opacity(0.65))
        .text_color(theme.text_subtle)
        .child(
            div()
                .h_full()
                .w(rems(2.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_r_1()
                .border_color(theme.border_subtle)
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size(13.0)
                        .color(theme.text_muted),
                ),
        )
        .child(
            div()
                .px_3()
                .text_xs()
                .child(format!("{lines} unmodified line{}", plural(lines))),
        )
        .into_any_element()
}

fn render_conflict_actions_row<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    root: PathBuf,
    path: String,
    start_line: usize,
    current_label: String,
    incoming_label: String,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(("diff-conflict-actions", ix))
        .mx(rems(CARD_MARGIN_X_REM))
        .h(rems(DIFF_ROW_HEIGHT_REM))
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .overflow_hidden()
        .bg(theme.bg_hover)
        .px_3()
        .text_xs()
        .text_color(theme.text_subtle)
        .child(conflict_action_link(
            "Accept current change",
            root.clone(),
            path.clone(),
            start_line,
            ConflictResolution::Current,
            cx,
        ))
        .child(conflict_action_separator(theme))
        .child(conflict_action_link(
            "Accept incoming change",
            root.clone(),
            path.clone(),
            start_line,
            ConflictResolution::Incoming,
            cx,
        ))
        .child(conflict_action_separator(theme))
        .child(conflict_action_link(
            "Accept both",
            root,
            path,
            start_line,
            ConflictResolution::Both,
            cx,
        ))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(theme.text_muted)
                .child(format!("{current_label} -> {incoming_label}")),
        )
        .into_any_element()
}

fn conflict_action_link<T: PaneDelegate + SettingsDelegate>(
    label: &'static str,
    root: PathBuf,
    path: String,
    start_line: usize,
    resolution: ConflictResolution,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = format!(
        "diff-conflict-action-{}-{}-{}",
        conflict_resolution_key(resolution),
        start_line,
        path
    );
    div()
        .id(id)
        .flex_none()
        .cursor_pointer()
        .text_color(theme.text_muted)
        .hover(move |this| this.text_color(theme.text_emphasis))
        .on_click(cx.listener(move |_, _, _, cx| {
            accept_conflict(root.clone(), path.clone(), start_line, resolution, cx);
        }))
        .child(label)
        .into_any_element()
}

fn conflict_resolution_key(resolution: ConflictResolution) -> &'static str {
    match resolution {
        ConflictResolution::Current => "current",
        ConflictResolution::Incoming => "incoming",
        ConflictResolution::Both => "both",
    }
}

fn conflict_action_separator(theme: theme::Theme) -> AnyElement {
    div()
        .flex_none()
        .text_color(theme.border_strong)
        .child("|")
        .into_any_element()
}

fn render_conflict_marker_row<T: 'static>(
    ix: usize,
    line: usize,
    text: String,
    side: Option<ConflictSide>,
    soft_wrap: bool,
    wrap_columns: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let segments = wrap_diff_text(&text, soft_wrap, wrap_columns);
    let segment_count = segments.len().max(1);
    let (bg, color) = match side {
        Some(ConflictSide::Current) => (
            gpui::Hsla::from(theme.success).opacity(0.22),
            theme.text_emphasis,
        ),
        Some(ConflictSide::Incoming) => (
            gpui::Hsla::from(theme.accent).opacity(0.22),
            theme.text_emphasis,
        ),
        None => (gpui::Hsla::from(theme.bg_hover), theme.text_subtle),
    };

    div()
        .id(("diff-conflict-marker", ix))
        .mx(rems(CARD_MARGIN_X_REM))
        .h(rems(DIFF_ROW_HEIGHT_REM * segment_count as f32))
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(bg)
        .font_family(FONT_FAMILY)
        .text_size(rems(DIFF_CODE_TEXT_SIZE_REM))
        .text_color(color)
        .children(
            segments
                .into_iter()
                .enumerate()
                .map(|(segment_ix, segment)| {
                    div()
                        .h(rems(DIFF_ROW_HEIGHT_REM))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .child(diff_line_marker(DiffLineKind::Context, theme))
                        .child(line_number_cell((segment_ix == 0).then_some(line), theme))
                        .child(div().w(rems(SIGN_WIDTH_REM)).flex_none())
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(segment.text),
                        )
                }),
        )
        .into_any_element()
}

fn render_conflict_line_row<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    path: String,
    side: ConflictSide,
    line: ConflictLine,
    root: PathBuf,
    highlight_cache: Rc<DiffHighlightCache>,
    delegate: Entity<T>,
    soft_wrap: bool,
    wrap_columns: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let bg = match side {
        ConflictSide::Current => gpui::Hsla::from(theme.success).opacity(0.12),
        ConflictSide::Incoming => gpui::Hsla::from(theme.accent).opacity(0.12),
    };
    let side_color = match side {
        ConflictSide::Current => theme.success,
        ConflictSide::Incoming => theme.accent,
    };
    let line_number = line.line;
    let key = conflict_line_highlight_key(side, line_number);
    let highlights = highlight_cache.highlights(&path, &key);
    let segments = wrap_diff_text(&line.text, soft_wrap, wrap_columns);
    let segment_count = segments.len().max(1);
    let absolute_path = root.join(path);

    div()
        .id(("diff-conflict-line", ix))
        .mx(rems(CARD_MARGIN_X_REM))
        .h(rems(DIFF_ROW_HEIGHT_REM * segment_count as f32))
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(bg)
        .font_family(FONT_FAMILY)
        .text_size(rems(DIFF_CODE_TEXT_SIZE_REM))
        .text_color(theme.text)
        .hover(move |this| this.bg(theme.bg_hover))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            let path = absolute_path.clone();
            delegate.update(cx, |delegate, cx| {
                delegate.open_file_at(path, line_number, 0, cx)
            });
        })
        .children(
            segments
                .into_iter()
                .enumerate()
                .map(|(segment_ix, segment)| {
                    let segment_highlights = highlights_for_segment(highlights, &segment.range);
                    div()
                        .h(rems(DIFF_ROW_HEIGHT_REM))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .child(diff_line_marker(DiffLineKind::Context, theme))
                        .child(line_number_cell(
                            (segment_ix == 0).then_some(line_number),
                            theme,
                        ))
                        .child(
                            div()
                                .w(rems(SIGN_WIDTH_REM))
                                .flex_none()
                                .text_color(side_color)
                                .child(if segment_ix == 0 { "|" } else { " " }),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(highlighted_code_text(
                                    segment.text,
                                    Some(&segment_highlights),
                                )),
                        )
                }),
        )
        .into_any_element()
}

fn render_line_row<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    path: String,
    can_open: bool,
    line: DiffLine,
    root: PathBuf,
    highlight_cache: Rc<DiffHighlightCache>,
    delegate: Entity<T>,
    soft_wrap: bool,
    wrap_columns: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let (sign, text_color, bg) = match line.kind {
        DiffLineKind::Context => (' ', theme.text, gpui::Hsla::from(theme.bg_surface)),
        DiffLineKind::Added => (
            '+',
            theme.text,
            gpui::Hsla::from(theme.success).opacity(0.12),
        ),
        DiffLineKind::Removed => (
            '-',
            theme.text,
            gpui::Hsla::from(theme.danger).opacity(0.12),
        ),
    };
    let target_line = line.new_line;
    let line_highlight_key = line_highlight_key(&line);
    let absolute_path = root.join(&path);
    let display_line = line.new_line.or(line.old_line);
    let highlights = highlight_cache.highlights(&path, &line_highlight_key);
    let segments = wrap_diff_text(&line.text, soft_wrap, wrap_columns);
    let segment_count = segments.len().max(1);

    div()
        .id(("diff-line", ix))
        .mx(rems(CARD_MARGIN_X_REM))
        .h(rems(DIFF_ROW_HEIGHT_REM * segment_count as f32))
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(bg)
        .font_family(FONT_FAMILY)
        .text_size(rems(DIFF_CODE_TEXT_SIZE_REM))
        .text_color(text_color)
        .hover(move |this| this.bg(theme.bg_hover))
        .when(can_open && target_line.is_some(), |this| {
            this.cursor_pointer().on_click(move |_, _, cx| {
                let Some(line) = target_line else {
                    return;
                };
                let path = absolute_path.clone();
                delegate.update(cx, |delegate, cx| delegate.open_file_at(path, line, 0, cx));
            })
        })
        .children(
            segments
                .into_iter()
                .enumerate()
                .map(|(segment_ix, segment)| {
                    let segment_highlights = highlights_for_segment(highlights, &segment.range);
                    div()
                        .h(rems(DIFF_ROW_HEIGHT_REM))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .child(diff_line_marker(line.kind, theme))
                        .child(line_number_cell(
                            (segment_ix == 0).then_some(display_line).flatten(),
                            theme,
                        ))
                        .child(
                            div()
                                .w(rems(SIGN_WIDTH_REM))
                                .flex_none()
                                .text_color(match line.kind {
                                    DiffLineKind::Added => theme.success,
                                    DiffLineKind::Removed => theme.danger,
                                    DiffLineKind::Context => theme.text_subtle,
                                })
                                .child(if segment_ix == 0 {
                                    sign.to_string()
                                } else {
                                    " ".to_string()
                                }),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(highlighted_code_text(
                                    segment.text,
                                    Some(&segment_highlights),
                                )),
                        )
                }),
        )
        .into_any_element()
}

fn render_message_row<T: 'static>(
    ix: usize,
    text: String,
    soft_wrap: bool,
    wrap_columns: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let segments = wrap_diff_text(&text, soft_wrap, wrap_columns);
    let segment_count = segments.len().max(1);
    div()
        .id(("diff-message", ix))
        .mx(rems(CARD_MARGIN_X_REM))
        .h(rems(DIFF_ROW_HEIGHT_REM * segment_count as f32))
        .w_full()
        .flex()
        .flex_col()
        .px_3()
        .rounded(rems(0.375))
        .bg(gpui::Hsla::from(theme.bg_hover).opacity(0.35))
        .text_sm()
        .text_color(theme.text_subtle)
        .overflow_hidden()
        .children(segments.into_iter().map(|segment| {
            div()
                .h(rems(DIFF_ROW_HEIGHT_REM))
                .w_full()
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(segment.text)
        }))
        .into_any_element()
}

fn line_number_cell(line: Option<usize>, theme: theme::Theme) -> AnyElement {
    div()
        .w(rems(LINE_NUMBER_WIDTH_REM))
        .h_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .pr_2()
        .text_color(theme.text_subtle)
        .child(line.map(|line| line.to_string()).unwrap_or_default())
        .into_any_element()
}

fn diff_line_marker(kind: DiffLineKind, theme: theme::Theme) -> AnyElement {
    let color = match kind {
        DiffLineKind::Added => gpui::Hsla::from(theme.success),
        DiffLineKind::Removed => gpui::Hsla::from(theme.danger),
        DiffLineKind::Context => gpui::Hsla::from(theme.bg_surface),
    };
    div()
        .h_full()
        .w(rems(0.125))
        .flex_none()
        .bg(color)
        .into_any_element()
}

fn centered_state<T: PaneDelegate + SettingsDelegate>(
    message: impl Into<SharedString>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(theme.text_subtle)
        .child(Icon::new(IconName::Diff).size(28.0).color(theme.text_muted))
        .child(div().text_sm().child(message.into()))
        .into_any_element()
}

fn flatten_diff(diff: &RepositoryDiff) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for file in &diff.files {
        let (insertions, deletions) = file_stats(file);
        rows.push(DiffRow::File {
            path: file.path.clone(),
            old_path: file.old_path.clone(),
            kind: file.kind,
            insertions,
            deletions,
            binary: file.binary,
        });

        if file.binary {
            rows.push(DiffRow::Message {
                path: file.path.clone(),
                text: "Binary file changed".to_string(),
            });
            continue;
        }
        if !file.conflicts.is_empty() {
            for conflict in &file.conflicts {
                push_conflict_rows(&mut rows, &file.path, conflict);
            }
            continue;
        }
        if file.hunks.is_empty() {
            rows.push(DiffRow::Message {
                path: file.path.clone(),
                text: "No textual diff".to_string(),
            });
            continue;
        }

        let can_open = file.kind != FileChangeKind::Deleted;
        let mut last_new_end = 0;
        for hunk in &file.hunks {
            let unchanged_lines = hunk.new_start.saturating_sub(last_new_end + 1);
            if unchanged_lines > 0 {
                rows.push(DiffRow::Unchanged {
                    path: file.path.clone(),
                    lines: unchanged_lines,
                });
            }
            rows.extend(hunk.lines.iter().cloned().map(|line| DiffRow::Line {
                path: file.path.clone(),
                can_open,
                line,
            }));
            last_new_end = hunk.new_start + hunk.new_lines.saturating_sub(1);
        }
    }
    rows
}

fn push_conflict_rows(rows: &mut Vec<DiffRow>, path: &str, conflict: &ConflictBlock) {
    rows.push(DiffRow::ConflictActions {
        path: path.to_string(),
        start_line: conflict.start_line,
        current_label: conflict.current_label.clone(),
        incoming_label: conflict.incoming_label.clone(),
    });
    rows.push(DiffRow::ConflictMarker {
        path: path.to_string(),
        line: conflict.start_line,
        text: format!("<<<<<<< {} (Current Change)", conflict.current_label),
        side: Some(ConflictSide::Current),
    });
    rows.extend(
        conflict
            .current
            .iter()
            .cloned()
            .map(|line| DiffRow::ConflictLine {
                path: path.to_string(),
                side: ConflictSide::Current,
                line,
            }),
    );
    rows.push(DiffRow::ConflictMarker {
        path: path.to_string(),
        line: conflict.separator_line,
        text: "=======".to_string(),
        side: None,
    });
    rows.extend(
        conflict
            .incoming
            .iter()
            .cloned()
            .map(|line| DiffRow::ConflictLine {
                path: path.to_string(),
                side: ConflictSide::Incoming,
                line,
            }),
    );
    rows.push(DiffRow::ConflictMarker {
        path: path.to_string(),
        line: conflict.end_line,
        text: format!(">>>>>>> {} (Incoming Change)", conflict.incoming_label),
        side: Some(ConflictSide::Incoming),
    });
}

impl DiffRow {
    fn path(&self) -> &str {
        match self {
            Self::File { path, .. }
            | Self::Unchanged { path, .. }
            | Self::Line { path, .. }
            | Self::ConflictActions { path, .. }
            | Self::ConflictMarker { path, .. }
            | Self::ConflictLine { path, .. }
            | Self::Message { path, .. } => path,
        }
    }
}

fn file_stats(file: &kosmos_git::FileDiff) -> (usize, usize) {
    file.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .fold((0, 0), |(insertions, deletions), line| match line.kind {
            DiffLineKind::Added => (insertions + 1, deletions),
            DiffLineKind::Removed => (insertions, deletions + 1),
            DiffLineKind::Context => (insertions, deletions),
        })
}

fn diff_stats(insertions: usize, deletions: usize, theme: theme::Theme) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(theme.success)
                .child(format!("+{insertions}")),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.danger)
                .child(format!("-{deletions}")),
        )
        .into_any_element()
}

fn change_kind_tag(kind: FileChangeKind, binary: bool, theme: theme::Theme) -> AnyElement {
    let label = if binary {
        "Binary"
    } else {
        match kind {
            FileChangeKind::Created => "Added",
            FileChangeKind::Modified => "Modified",
            FileChangeKind::Deleted => "Deleted",
            FileChangeKind::Renamed => "Renamed",
            FileChangeKind::Conflicted => "Conflict",
        }
    };
    div()
        .flex_none()
        .rounded(rems(0.25))
        .border_1()
        .border_color(theme.border_subtle)
        .px_1p5()
        .py_0p5()
        .text_xs()
        .text_color(theme.text_subtle)
        .child(label)
        .into_any_element()
}

fn file_icon_color(kind: FileChangeKind, theme: theme::Theme) -> gpui::Rgba {
    match kind {
        FileChangeKind::Created => theme.success,
        FileChangeKind::Deleted => theme.danger,
        FileChangeKind::Renamed | FileChangeKind::Conflicted => theme.accent_secondary,
        FileChangeKind::Modified => theme.text_muted,
    }
}

fn icon_for_path(path: &Path) -> IconName {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(icon) = IconName::for_file_name(name)
    {
        return icon;
    }

    language::from_path(path)
        .and_then(|id| IconName::for_language(id.as_str()))
        .unwrap_or(IconName::File)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

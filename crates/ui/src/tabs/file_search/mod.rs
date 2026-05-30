use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui::{
    AnyElement, AppContext, Context, Entity, FocusHandle, FontWeight, Global, HighlightStyle,
    IntoElement, KeyDownEvent, ListSizingBehavior, ScrollStrategy, SharedString, StyledText,
    Window, div, point, prelude::*, px, rems, size,
};
use gpui_component::{
    Icon as ComponentIcon, IconName as ComponentIconName, Selectable, Sizable, Size,
    VirtualListScrollHandle,
    alert::Alert,
    button::{Button, ButtonGroup},
    input::{IndentInline, Input, InputEvent, InputState, OutdentInline},
    list::ListItem,
    v_virtual_list,
};
use icons::{Icon, IconName};
use settings::{ActiveSettings, SettingValue};

use tabs::registry;
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, SettingsDelegate};

const INITIAL_RESULT_LIMIT: usize = 50;
const RESULT_LIMIT_INCREMENT: usize = 100;
const LOAD_MORE_THRESHOLD: usize = 10;
const RESULT_ROW_HEIGHT_REM: f32 = 3.125;
const CONTENT_RESULT_ROW_HEIGHT_REM: f32 = 3.125;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(80);
const SCAN_REFRESH_INTERVAL: Duration = Duration::from_millis(200);
const PREVIEW_FALLBACK_CENTER_OFFSET: usize = 18;
const PREVIEW_CONTEXT_LINES: usize = 64;
const PREVIEW_REUSE_MARGIN_LINES: usize = 16;
const PREVIEW_PREFETCH_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const PREVIEW_PREFETCH_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const FONT_FAMILY: &str = "DejaVu Sans Mono";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchMode {
    Name,
    Content,
}

impl SearchMode {
    fn toggled(self) -> Self {
        match self {
            Self::Name => Self::Content,
            Self::Content => Self::Name,
        }
    }

    fn from_index(index: usize) -> Self {
        if index == 1 {
            Self::Content
        } else {
            Self::Name
        }
    }

    fn loading_label(self) -> &'static str {
        match self {
            Self::Name => "Searching files...",
            Self::Content => "Searching contents...",
        }
    }
}

#[derive(Clone)]
enum SearchSnapshot {
    Name(Arc<::file_search::FileSearchSnapshot>),
    Content(Arc<::file_search::ContentSearchSnapshot>),
}

impl SearchSnapshot {
    fn query(&self) -> &str {
        match self {
            Self::Name(snapshot) => &snapshot.query,
            Self::Content(snapshot) => &snapshot.query,
        }
    }

    fn is_scanning(&self) -> bool {
        match self {
            Self::Name(snapshot) => snapshot.is_scanning,
            Self::Content(snapshot) => snapshot.is_scanning,
        }
    }

    fn result_count(&self) -> usize {
        match self {
            Self::Name(snapshot) => snapshot.results.len(),
            Self::Content(snapshot) => snapshot.results.len(),
        }
    }

    fn can_load_more(&self, current_limit: usize) -> bool {
        if self.query().trim().is_empty() {
            return false;
        }

        match self {
            Self::Name(snapshot) => snapshot.results.len() < snapshot.total_matched,
            Self::Content(snapshot) => snapshot.results.len() >= current_limit,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreviewTarget {
    path: PathBuf,
    line_number: u64,
    column: usize,
}

#[derive(Clone)]
struct PreviewFile {
    content: Arc<str>,
    language: String,
    line_starts: Arc<[usize]>,
}

#[derive(Clone)]
struct PreviewRequest {
    target: PreviewTarget,
    generation: u64,
}

#[derive(Clone)]
struct PreviewPrefetchFile {
    path: PathBuf,
}

#[derive(Clone)]
struct PreviewPrefetchRequest {
    generation: u64,
    files: Vec<PreviewPrefetchFile>,
}

struct PreviewFileData {
    path: PathBuf,
    file: PreviewFile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreviewWindow {
    path: PathBuf,
    start_line: usize,
    end_line: usize,
    trailing_padding: usize,
}

#[derive(Clone)]
struct PreviewUpdate {
    input: Entity<InputState>,
    content: Option<Arc<str>>,
    language: String,
    line_number_base: usize,
    cursor_line: usize,
    column: usize,
    language_changed: bool,
}

pub struct FileSearchUi {
    input: Entity<InputState>,
    preview_input: Entity<InputState>,
    preview_soft_wrap: bool,
    focus_handle: FocusHandle,
    indices: HashMap<PathBuf, Arc<::file_search::FileSearchIndex>>,
    active_root: Option<PathBuf>,
    active_index: Option<Arc<::file_search::FileSearchIndex>>,
    mode: SearchMode,
    query: String,
    snapshot: Option<SearchSnapshot>,
    error: Option<String>,
    loading: bool,
    search_running: bool,
    loading_more: bool,
    selected_result: usize,
    result_limit: usize,
    generation: u64,
    refresh_scheduled: bool,
    scroll_handle: VirtualListScrollHandle,
    preview_target: Option<PreviewTarget>,
    preview_generation: u64,
    preview_loading: bool,
    preview_content: Option<PreviewFile>,
    preview_language: String,
    preview_error: Option<String>,
    preview_dirty: bool,
    preview_cache: HashMap<PathBuf, PreviewFile>,
    preview_input_file_content: Option<Arc<str>>,
    preview_input_window: Option<PreviewWindow>,
    preview_input_language: String,
    preview_prefetch_generation: u64,
}

#[derive(Clone)]
struct FileSearchView {
    index: Option<Arc<::file_search::FileSearchIndex>>,
    focus_handle: FocusHandle,
    mode: SearchMode,
    query: String,
    snapshot: Option<SearchSnapshot>,
    error: Option<String>,
    loading: bool,
    selected_result: usize,
    scroll_handle: VirtualListScrollHandle,
    preview_input: Entity<InputState>,
    preview_target: Option<PreviewTarget>,
    preview_loading: bool,
    preview_error: Option<String>,
}

#[derive(Clone)]
struct SearchRequest {
    index: Arc<::file_search::FileSearchIndex>,
    mode: SearchMode,
    query: String,
    generation: u64,
    limit: usize,
    kind: SearchKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchKind {
    Initial,
    Refresh,
    LoadMore,
}

enum SearchResult {
    Name(::file_search::FileSearchSnapshot),
    Content(::file_search::ContentSearchSnapshot),
}

struct OpenTarget {
    index: Arc<::file_search::FileSearchIndex>,
    query: String,
    path: PathBuf,
    line: Option<usize>,
    column: Option<usize>,
}

impl FileSearchUi {
    pub fn install<T: 'static>(window: &mut Window, cx: &mut Context<T>) {
        if cx.try_global::<Self>().is_some() {
            return;
        }

        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search files by name or path"));
        let preview_soft_wrap = preview_soft_wrap_enabled(cx);
        let preview_input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("plaintext")
                .multi_line(true)
                .line_number(true)
                .soft_wrap(preview_soft_wrap)
        });
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();

        cx.set_global(Self {
            input,
            preview_input,
            preview_soft_wrap,
            focus_handle: cx.focus_handle().tab_stop(true),
            indices: HashMap::new(),
            active_root: None,
            active_index: None,
            mode: SearchMode::Name,
            query: String::new(),
            snapshot: None,
            error: None,
            loading: false,
            search_running: false,
            loading_more: false,
            selected_result: 0,
            result_limit: INITIAL_RESULT_LIMIT,
            generation: 0,
            refresh_scheduled: false,
            scroll_handle: VirtualListScrollHandle::new(),
            preview_target: None,
            preview_generation: 0,
            preview_loading: false,
            preview_content: None,
            preview_language: "plaintext".to_string(),
            preview_error: None,
            preview_dirty: false,
            preview_cache: HashMap::new(),
            preview_input_file_content: None,
            preview_input_window: None,
            preview_input_language: "plaintext".to_string(),
            preview_prefetch_generation: 0,
        });
    }

    fn input(&self) -> Entity<InputState> {
        self.input.clone()
    }

    fn sync_preview_soft_wrap(&mut self, soft_wrap: bool) -> Option<Entity<InputState>> {
        if self.preview_soft_wrap == soft_wrap {
            return None;
        }

        self.preview_soft_wrap = soft_wrap;
        Some(self.preview_input.clone())
    }

    fn mode(&self) -> SearchMode {
        self.mode
    }

    fn index_for(
        &mut self,
        root: PathBuf,
    ) -> Result<Arc<::file_search::FileSearchIndex>, ::file_search::Error> {
        if let Some(index) = self.indices.get(&root) {
            return Ok(index.clone());
        }

        let index = Arc::new(::file_search::FileSearchIndex::new(root.clone())?);
        self.indices.insert(root, index.clone());
        Ok(index)
    }

    fn prepare_search(&mut self, root: PathBuf, query: String) -> Option<SearchRequest> {
        let root_changed = self.active_root.as_ref() != Some(&root);
        let query_changed = self.query != query;
        if !root_changed
            && !query_changed
            && (self.loading || self.snapshot.is_some() || self.error.is_some())
        {
            return None;
        }

        let index = match self.index_for(root.clone()) {
            Ok(index) => index,
            Err(error) => {
                self.active_root = Some(root);
                self.active_index = None;
                self.query = query;
                self.snapshot = None;
                self.error = Some(error.to_string());
                self.loading = false;
                self.loading_more = false;
                return None;
            }
        };

        self.generation = self.generation.wrapping_add(1);
        self.active_root = Some(root);
        self.active_index = Some(index.clone());
        self.query = query.clone();
        self.snapshot = None;
        self.error = None;
        self.loading = true;
        self.loading_more = false;
        self.refresh_scheduled = false;
        if root_changed || query_changed {
            self.selected_result = 0;
            self.result_limit = INITIAL_RESULT_LIMIT;
            self.scroll_handle = VirtualListScrollHandle::new();
            self.clear_preview();
            self.preview_cache.clear();
        }

        Some(SearchRequest {
            index,
            mode: self.mode,
            query,
            generation: self.generation,
            limit: self.result_limit,
            kind: SearchKind::Initial,
        })
    }

    fn set_mode(&mut self, mode: SearchMode) {
        if self.mode == mode {
            return;
        }

        self.mode = mode;
        self.generation = self.generation.wrapping_add(1);
        self.snapshot = None;
        self.error = None;
        self.loading = false;
        self.loading_more = false;
        self.selected_result = 0;
        self.result_limit = INITIAL_RESULT_LIMIT;
        self.scroll_handle = VirtualListScrollHandle::new();
        self.refresh_scheduled = false;
        self.clear_preview();
        self.preview_cache.clear();
    }

    fn toggle_mode(&mut self) {
        self.set_mode(self.mode.toggled());
    }

    fn view(&self) -> FileSearchView {
        FileSearchView {
            index: self.active_index.clone(),
            focus_handle: self.focus_handle.clone(),
            mode: self.mode,
            query: self.query.clone(),
            snapshot: self.snapshot.clone(),
            error: self.error.clone(),
            loading: self.loading,
            selected_result: self.selected_result,
            scroll_handle: self.scroll_handle.clone(),
            preview_input: self.preview_input.clone(),
            preview_target: self.preview_target.clone(),
            preview_loading: self.preview_loading,
            preview_error: self.preview_error.clone(),
        }
    }

    fn selected_open_target(&self) -> Option<OpenTarget> {
        let index = self.active_index.clone()?;
        let snapshot = self.snapshot.as_ref()?;
        match snapshot {
            SearchSnapshot::Name(snapshot) => {
                let result = snapshot.results.get(self.selected_result)?;
                Some(OpenTarget {
                    index,
                    query: snapshot.query.clone(),
                    path: result.absolute_path.clone(),
                    line: None,
                    column: None,
                })
            }
            SearchSnapshot::Content(snapshot) => {
                let result = snapshot.results.get(self.selected_result)?;
                Some(OpenTarget {
                    index,
                    query: snapshot.query.clone(),
                    path: result.absolute_path.clone(),
                    line: Some(result.line_number as usize),
                    column: Some(byte_column_to_char_column(
                        &result.line_content,
                        result.column,
                    )),
                })
            }
        }
    }

    fn move_selection(&mut self, direction: isize) -> bool {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return false;
        };
        if snapshot.result_count() == 0 {
            return false;
        }

        let last = snapshot.result_count() - 1;
        let next = if direction < 0 {
            self.selected_result.saturating_sub(1)
        } else {
            (self.selected_result + 1).min(last)
        };
        if next == self.selected_result {
            return false;
        }

        self.select_result(next, true)
    }

    fn select_result(&mut self, result: usize, scroll_to_result: bool) -> bool {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return false;
        };
        if result >= snapshot.result_count() || result == self.selected_result {
            return false;
        }

        self.selected_result = result;
        if scroll_to_result {
            self.scroll_handle
                .scroll_to_item(self.selected_result, ScrollStrategy::Nearest);
        }
        self.invalidate_preview_for_navigation();
        true
    }

    fn begin_search(&mut self, generation: u64, kind: SearchKind) -> bool {
        if self.generation != generation || self.search_running {
            match kind {
                SearchKind::Refresh => self.refresh_scheduled = false,
                SearchKind::LoadMore => self.loading_more = false,
                SearchKind::Initial => {}
            }
            return false;
        }

        self.search_running = true;
        true
    }

    fn finish_search(
        &mut self,
        generation: u64,
        result: Result<SearchResult, String>,
        kind: SearchKind,
    ) -> Option<SearchRequest> {
        self.search_running = false;
        match kind {
            SearchKind::Refresh => self.refresh_scheduled = false,
            SearchKind::LoadMore => self.loading_more = false,
            SearchKind::Initial => {}
        }

        if self.generation != generation {
            return self.active_index.clone().map(|index| SearchRequest {
                index,
                mode: self.mode,
                query: self.query.clone(),
                generation: self.generation,
                limit: self.result_limit,
                kind: SearchKind::Initial,
            });
        }

        self.loading = false;
        match result {
            Ok(SearchResult::Name(snapshot)) => {
                self.selected_result = self
                    .selected_result
                    .min(snapshot.results.len().saturating_sub(1));
                self.snapshot = Some(SearchSnapshot::Name(Arc::new(snapshot)));
                if kind != SearchKind::LoadMore {
                    self.clear_preview();
                }
                self.preview_cache.clear();
                self.error = None;
            }
            Ok(SearchResult::Content(snapshot)) => {
                self.selected_result = self
                    .selected_result
                    .min(snapshot.results.len().saturating_sub(1));
                self.retain_preview_cache_for_results(&snapshot.results);
                self.snapshot = Some(SearchSnapshot::Content(Arc::new(snapshot)));
                if kind != SearchKind::LoadMore {
                    self.clear_preview();
                }
                self.error = None;
            }
            Err(error) => {
                if kind != SearchKind::LoadMore {
                    self.snapshot = None;
                }
                self.error = Some(error);
            }
        }

        None
    }

    fn prepare_load_more(&mut self, visible_end: usize) -> Option<SearchRequest> {
        if self.loading || self.loading_more || self.search_running || self.error.is_some() {
            return None;
        }

        let snapshot = self.snapshot.as_ref()?;
        let result_count = snapshot.result_count();
        if result_count == 0 || visible_end.saturating_add(LOAD_MORE_THRESHOLD) < result_count {
            return None;
        }
        if !snapshot.can_load_more(self.result_limit) {
            return None;
        }

        let index = self.active_index.clone()?;
        self.result_limit = self.result_limit.saturating_add(RESULT_LIMIT_INCREMENT);
        self.loading_more = true;

        Some(SearchRequest {
            index,
            mode: self.mode,
            query: self.query.clone(),
            generation: self.generation,
            limit: self.result_limit,
            kind: SearchKind::LoadMore,
        })
    }

    fn selected_preview_target(&self) -> Option<PreviewTarget> {
        let Some(SearchSnapshot::Content(snapshot)) = self.snapshot.as_ref() else {
            return None;
        };
        let result = snapshot.results.get(self.selected_result)?;
        Some(PreviewTarget {
            path: result.absolute_path.clone(),
            line_number: result.line_number,
            column: byte_column_to_char_column(&result.line_content, result.column),
        })
    }

    fn prepare_preview(&mut self) -> Option<PreviewRequest> {
        let target = self.selected_preview_target()?;
        if self.preview_target.as_ref() == Some(&target)
            && (self.preview_loading
                || self.preview_content.is_some()
                || self.preview_error.is_some())
        {
            return None;
        }

        let cached_file = self.preview_cache.get(&target.path).cloned();
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.preview_target = Some(target.clone());
        self.preview_error = None;

        if let Some(file) = cached_file {
            self.preview_loading = false;
            self.preview_language = file.language.clone();
            self.preview_content = Some(file);
            self.preview_dirty = true;
            return None;
        }

        self.preview_loading = true;
        self.preview_content = None;
        self.preview_dirty = false;

        Some(PreviewRequest {
            target,
            generation: self.preview_generation,
        })
    }

    fn prepare_preview_prefetch(&mut self) -> Option<PreviewPrefetchRequest> {
        if self.preview_prefetch_generation == self.generation {
            return None;
        }

        let Some(SearchSnapshot::Content(snapshot)) = self.snapshot.as_ref() else {
            return None;
        };
        if snapshot.results.is_empty() {
            return None;
        }

        self.preview_prefetch_generation = self.generation;

        let mut seen = HashSet::new();
        let mut files = Vec::new();
        let mut total_bytes = 0_u64;
        let selected = self.selected_result.min(snapshot.results.len() - 1);

        for ix in std::iter::once(selected).chain(0..snapshot.results.len()) {
            let result = &snapshot.results[ix];
            if result.is_binary
                || result.size > PREVIEW_PREFETCH_MAX_FILE_BYTES
                || total_bytes.saturating_add(result.size) > PREVIEW_PREFETCH_TOTAL_BYTES
                || self.preview_cache.contains_key(&result.absolute_path)
                || !seen.insert(result.absolute_path.clone())
            {
                continue;
            }

            total_bytes = total_bytes.saturating_add(result.size);
            files.push(PreviewPrefetchFile {
                path: result.absolute_path.clone(),
            });
        }

        if files.is_empty() {
            return None;
        }

        Some(PreviewPrefetchRequest {
            generation: self.generation,
            files,
        })
    }

    fn finish_preview(&mut self, generation: u64, result: Result<PreviewFile, String>) -> bool {
        if self.preview_generation != generation {
            return false;
        }

        self.preview_loading = false;
        match result {
            Ok(file) => {
                if let Some(target) = self.preview_target.as_ref() {
                    self.preview_cache.insert(target.path.clone(), file.clone());
                }
                self.preview_language = file.language.clone();
                self.preview_content = Some(file);
                self.preview_error = None;
                self.preview_dirty = true;
            }
            Err(error) => {
                self.preview_content = None;
                self.preview_error = Some(error);
                self.preview_dirty = false;
            }
        }
        true
    }

    fn finish_preview_prefetch(&mut self, generation: u64, files: Vec<PreviewFileData>) -> bool {
        if self.generation != generation {
            return false;
        }

        let Some(SearchSnapshot::Content(snapshot)) = self.snapshot.as_ref() else {
            return false;
        };
        let result_paths = snapshot
            .results
            .iter()
            .map(|result| result.absolute_path.clone())
            .collect::<HashSet<_>>();
        let mut current_preview = None;

        for file in files {
            if !result_paths.contains(&file.path) {
                continue;
            }

            let entry = PreviewFile {
                content: file.file.content,
                language: file.file.language,
                line_starts: file.file.line_starts,
            };
            if self
                .preview_target
                .as_ref()
                .is_some_and(|target| target.path == file.path)
            {
                current_preview = Some(entry.clone());
            }
            self.preview_cache.insert(file.path, entry);
        }

        let Some(file) = current_preview else {
            return false;
        };
        if !self.preview_loading && self.preview_content.is_some() {
            return false;
        }

        self.preview_loading = false;
        self.preview_language = file.language.clone();
        self.preview_content = Some(file);
        self.preview_error = None;
        self.preview_dirty = true;
        true
    }

    fn take_preview_update(&mut self) -> Option<PreviewUpdate> {
        if !self.preview_dirty {
            return None;
        }
        let target = self.preview_target.as_ref()?;
        let file = self.preview_content.clone()?;
        let line_count = file.line_starts.len().max(1);
        let target_line =
            (target.line_number.saturating_sub(1) as usize).min(line_count.saturating_sub(1));

        if let Some(window) = self
            .reusable_preview_window(target, &file, target_line)
            .cloned()
        {
            self.preview_dirty = false;
            return Some(PreviewUpdate {
                input: self.preview_input.clone(),
                content: None,
                language: file.language,
                line_number_base: window.start_line,
                cursor_line: target_line.saturating_sub(window.start_line),
                column: target.column,
                language_changed: false,
            });
        }

        let start_line = target_line.saturating_sub(PREVIEW_CONTEXT_LINES);
        let end_line = target_line
            .saturating_add(PREVIEW_CONTEXT_LINES + 1)
            .min(line_count);
        let trailing_padding = if end_line == line_count {
            PREVIEW_CONTEXT_LINES
        } else {
            0
        };
        let content = preview_slice(&file, start_line, end_line, trailing_padding);
        let language_changed = self.preview_input_language != file.language;
        let window = PreviewWindow {
            path: target.path.clone(),
            start_line,
            end_line,
            trailing_padding,
        };

        self.preview_dirty = false;
        self.preview_input_file_content = Some(file.content.clone());
        self.preview_input_window = Some(window);
        self.preview_input_language = file.language.clone();

        Some(PreviewUpdate {
            input: self.preview_input.clone(),
            content: Some(content),
            language: file.language,
            line_number_base: start_line,
            cursor_line: target_line.saturating_sub(start_line),
            column: target.column,
            language_changed,
        })
    }

    fn reusable_preview_window(
        &self,
        target: &PreviewTarget,
        file: &PreviewFile,
        target_line: usize,
    ) -> Option<&PreviewWindow> {
        if self.preview_input_language != file.language
            || !self
                .preview_input_file_content
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &file.content))
        {
            return None;
        }

        let window = self.preview_input_window.as_ref()?;
        if window.path != target.path
            || target_line < window.start_line
            || target_line >= window.end_line
        {
            return None;
        }

        let line_count = file.line_starts.len().max(1);
        let before = target_line.saturating_sub(window.start_line);
        let after = window
            .end_line
            .saturating_sub(target_line.saturating_add(1))
            .saturating_add(window.trailing_padding);
        let before_ok = window.start_line == 0 || before >= PREVIEW_REUSE_MARGIN_LINES;
        let after_ok = window.end_line == line_count || after >= PREVIEW_REUSE_MARGIN_LINES;
        if before_ok && after_ok {
            Some(window)
        } else {
            None
        }
    }

    fn clear_preview(&mut self) {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.preview_target = None;
        self.preview_loading = false;
        self.preview_content = None;
        self.preview_error = None;
        self.preview_dirty = false;
    }

    fn invalidate_preview_for_navigation(&mut self) {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.preview_loading = false;
        self.preview_error = None;
        self.preview_dirty = false;
    }

    fn retain_preview_cache_for_results(&mut self, results: &[::file_search::ContentSearchResult]) {
        let result_paths = results
            .iter()
            .map(|result| result.absolute_path.clone())
            .collect::<HashSet<_>>();
        self.preview_cache
            .retain(|path, _| result_paths.contains(path));
    }
}

impl Global for FileSearchUi {}

pub fn render<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    workspace_path: &Path,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    FileSearchUi::install(window, cx);
    let theme = *cx.theme();
    let input = cx.global::<FileSearchUi>().input();
    let mode = cx.global::<FileSearchUi>().mode();
    sync_preview_soft_wrap(window, cx);
    let query = input.read(cx).value().trim().to_string();
    let root = workspace_path.to_path_buf();
    if let Some(request) =
        cx.update_global::<FileSearchUi, _>(|state, _| state.prepare_search(root, query))
    {
        spawn_search(request, SEARCH_DEBOUNCE, cx);
    }
    if matches!(mode, SearchMode::Content) {
        schedule_preview_prefetch(cx);
        schedule_preview_load(cx);
        sync_preview_input(window, cx);
    }

    let view = cx.global::<FileSearchUi>().view();

    if view.query.is_empty()
        && view
            .snapshot
            .as_ref()
            .is_some_and(SearchSnapshot::is_scanning)
    {
        schedule_scan_refresh(cx);
    }
    let focus_handle = view.focus_handle.clone();

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .track_focus(&focus_handle)
        .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
            match event.keystroke.key.as_str() {
                "tab" if !event.keystroke.modifiers.control && !event.keystroke.modifiers.alt => {
                    cx.stop_propagation();
                    toggle_search_mode(cx);
                }
                "up" => {
                    cx.stop_propagation();
                    cx.update_global::<FileSearchUi, _>(|state, _| {
                        state.move_selection(-1);
                    });
                    cx.notify();
                }
                "down" => {
                    cx.stop_propagation();
                    cx.update_global::<FileSearchUi, _>(|state, _| {
                        state.move_selection(1);
                    });
                    cx.notify();
                }
                "enter" => {
                    let Some(target) = cx.global::<FileSearchUi>().selected_open_target() else {
                        return;
                    };
                    cx.stop_propagation();
                    target.index.track_open(&target.query, &target.path);
                    if let Some(line) = target.line {
                        this.open_file_at(target.path, line, target.column.unwrap_or(0), cx);
                    } else {
                        this.open_file(target.path, cx);
                    }
                }
                _ => {}
            }
        }))
        .capture_action(cx.listener(|_, _: &IndentInline, _, cx| {
            cx.stop_propagation();
            toggle_search_mode(cx);
        }))
        .capture_action(cx.listener(|_, _: &OutdentInline, _, cx| {
            cx.stop_propagation();
            toggle_search_mode(cx);
        }))
        .child(header(&input, mode, cx))
        .when_some(view.error.clone(), |this, error| {
            this.child(error_banner(error, cx))
        })
        .child(results(&view, focus_handle, window, cx))
        .into_any_element()
}

fn toggle_search_mode<T: 'static>(cx: &mut Context<T>) {
    cx.update_global::<FileSearchUi, _>(|state, _| state.toggle_mode());
    cx.notify();
}

fn header<T: PaneDelegate + SettingsDelegate>(
    input: &Entity<InputState>,
    mode: SearchMode,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();

    div()
        .flex_none()
        .border_b_1()
        .border_color(theme.border_subtle)
        .p(rems(1.5))
        .child(
            div()
                .flex()
                .w_full()
                .items_center()
                .gap_3()
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(input)
                            .bordered(false)
                            .prefix(
                                ComponentIcon::new(ComponentIconName::Search)
                                    .small()
                                    .text_color(gpui::Hsla::from(theme.text_muted)),
                            )
                            .cleanable(true)
                            .w_full(),
                    ),
                )
                .child(search_mode_switch(mode)),
        )
        .into_any_element()
}

fn search_mode_switch(mode: SearchMode) -> AnyElement {
    ButtonGroup::new("file-search-mode")
        .compact()
        .outline()
        .with_size(Size::Medium)
        .child(
            Button::new("file-search-mode-name")
                .label("Name")
                .selected(mode == SearchMode::Name),
        )
        .child(
            Button::new("file-search-mode-content")
                .label("Content")
                .selected(mode == SearchMode::Content),
        )
        .on_click(|selected, _, cx| {
            let mode = selected
                .last()
                .copied()
                .map(SearchMode::from_index)
                .unwrap_or(SearchMode::Name);
            cx.update_global::<FileSearchUi, _>(|state, _| state.set_mode(mode));
            cx.refresh_windows();
        })
        .into_any_element()
}

fn error_banner<T: PaneDelegate + SettingsDelegate>(
    error: String,
    _cx: &mut Context<T>,
) -> AnyElement {
    Alert::error("file-search-error", SharedString::from(error))
        .banner()
        .with_size(Size::Small)
        .into_any_element()
}

fn results<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    view: &FileSearchView,
    focus_handle: FocusHandle,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let Some(snapshot) = view.snapshot.as_ref() else {
        if view.loading && view.query.is_empty() {
            return centered_state("Preparing file search...", Some(focus_handle), cx);
        }

        if view.loading {
            return centered_state(view.mode.loading_label(), Some(focus_handle), cx);
        }

        return centered_state(
            "Unable to load the file search index",
            Some(focus_handle),
            cx,
        );
    };

    if snapshot.result_count() == 0 {
        if snapshot.is_scanning() {
            let message = match view.mode {
                SearchMode::Name => "Indexing workspace files...",
                SearchMode::Content => "Indexing workspace contents...",
            };
            return centered_state(message, Some(focus_handle), cx);
        }

        if snapshot.query().is_empty() {
            let message = match view.mode {
                SearchMode::Name => "Start typing to search workspace files",
                SearchMode::Content => "Start typing to search file contents",
            };
            return centered_state(message, Some(focus_handle), cx);
        }

        return centered_state(
            format!("No results found for \"{}\"", snapshot.query()),
            Some(focus_handle),
            cx,
        );
    }

    match snapshot {
        SearchSnapshot::Name(snapshot) => {
            name_results(view, focus_handle, snapshot.clone(), window, cx)
        }
        SearchSnapshot::Content(snapshot) => {
            content_results(view, focus_handle, snapshot.clone(), window, cx)
        }
    }
}

fn name_results<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    view: &FileSearchView,
    focus_handle: FocusHandle,
    snapshot: Arc<::file_search::FileSearchSnapshot>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let Some(index) = view.index.clone() else {
        return centered_state(
            "Unable to load the file search index",
            Some(focus_handle),
            cx,
        );
    };

    let rem_size = window.rem_size();
    let item_sizes = Rc::new(
        (0..snapshot.results.len())
            .map(|_| size(px(0.0), rems(RESULT_ROW_HEIGHT_REM).to_pixels(rem_size)))
            .collect::<Vec<_>>(),
    );
    let selected_result = view.selected_result;
    let scroll_handle = view.scroll_handle.clone();

    div()
        .flex_1()
        .min_h_0()
        .track_focus(&focus_handle)
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            window.focus(&focus_handle, cx);
        })
        .child(
            div().size_full().child(
                v_virtual_list(cx.entity().clone(), "file-search-results", item_sizes, {
                    move |_, range, _window, cx| {
                        schedule_load_more(range.end, cx);
                        range
                            .map(|ix| {
                                result_row(
                                    ix,
                                    index.clone(),
                                    &snapshot.query,
                                    snapshot.results[ix].clone(),
                                    ix == selected_result,
                                    cx,
                                )
                            })
                            .collect::<Vec<_>>()
                    }
                })
                .flex_grow()
                .size_full()
                .track_scroll(&scroll_handle)
                .with_sizing_behavior(ListSizingBehavior::Auto),
            ),
        )
        .into_any_element()
}

fn content_results<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    view: &FileSearchView,
    focus_handle: FocusHandle,
    snapshot: Arc<::file_search::ContentSearchSnapshot>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let Some(index) = view.index.clone() else {
        return centered_state(
            "Unable to load the file search index",
            Some(focus_handle),
            cx,
        );
    };

    let rem_size = window.rem_size();
    let item_sizes = Rc::new(
        (0..snapshot.results.len())
            .map(|_| {
                size(
                    px(0.0),
                    rems(CONTENT_RESULT_ROW_HEIGHT_REM).to_pixels(rem_size),
                )
            })
            .collect::<Vec<_>>(),
    );
    let selected_result = view.selected_result;
    let scroll_handle = view.scroll_handle.clone();
    let results_snapshot = snapshot.clone();
    let theme = *cx.theme();

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .track_focus(&focus_handle)
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            window.focus(&focus_handle, cx);
        })
        .child(
            div().flex_1().min_w_0().size_full().child(
                v_virtual_list(cx.entity().clone(), "content-search-results", item_sizes, {
                    move |_, range, _window, cx| {
                        schedule_load_more(range.end, cx);
                        range
                            .map(|ix| {
                                content_result_row(
                                    ix,
                                    index.clone(),
                                    &results_snapshot.query,
                                    results_snapshot.results[ix].clone(),
                                    ix == selected_result,
                                    cx,
                                )
                            })
                            .collect::<Vec<_>>()
                    }
                })
                .flex_grow()
                .size_full()
                .track_scroll(&scroll_handle)
                .with_sizing_behavior(ListSizingBehavior::Auto),
            ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .size_full()
                .border_l_1()
                .border_color(theme.border_subtle)
                .child(content_preview(view, snapshot, window, cx)),
        )
        .into_any_element()
}

fn result_row<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    index: Arc<::file_search::FileSearchIndex>,
    query: &str,
    result: ::file_search::FileSearchResult,
    is_selected: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let query_text = query.to_string();
    let query_for_click = query_text.clone();
    let path = result.absolute_path.clone();
    let icon = icon_for_result(&result);
    let title = if result.name.is_empty() {
        result.relative_path.clone()
    } else {
        result.name.clone()
    };
    let metadata = format_size(result.size);

    ListItem::new(ix)
        .w_full()
        .h(rems(RESULT_ROW_HEIGHT_REM))
        .px(rems(1.5))
        .py_0()
        .when(is_selected, |row| row.bg(theme.bg_selected))
        .on_mouse_enter(move |_, _, cx| {
            let changed =
                cx.update_global::<FileSearchUi, _>(|state, _| state.select_result(ix, false));
            if changed {
                cx.refresh_windows();
            }
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .w_full()
                .child(
                    div()
                        .flex_none()
                        .size(rems(1.75))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size_rem(0.875).color(theme.text_muted)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.text_emphasis)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(highlighted_match_text(title, &query_text, theme.accent)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.text_subtle)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(highlighted_match_text(
                                    result.relative_path.clone(),
                                    &query_text,
                                    theme.accent,
                                )),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .child(metadata),
                ),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            index.track_open(&query_for_click, &path);
            this.open_file(path.clone(), cx);
        }))
        .into_any_element()
}

fn content_result_row<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    index: Arc<::file_search::FileSearchIndex>,
    query: &str,
    result: ::file_search::ContentSearchResult,
    is_selected: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let query_for_click = query.to_string();
    let path = result.absolute_path.clone();
    let icon = icon_for_content_result(&result);
    let title = if result.name.is_empty() {
        result.relative_path.clone()
    } else {
        result.name.clone()
    };
    let column = byte_column_to_char_column(&result.line_content, result.column);
    let metadata = format!("{}:{}", result.line_number, column + 1);

    ListItem::new(ix)
        .w_full()
        .h(rems(CONTENT_RESULT_ROW_HEIGHT_REM))
        .px(rems(1.5))
        .py_0()
        .when(is_selected, |row| row.bg(theme.bg_selected))
        .on_mouse_enter(move |_, _, cx| {
            let changed =
                cx.update_global::<FileSearchUi, _>(|state, _| state.select_result(ix, false));
            if changed {
                cx.refresh_windows();
            }
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .w_full()
                .child(
                    div()
                        .flex_none()
                        .size(rems(1.75))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size_rem(0.875).color(theme.text_muted)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .text_color(theme.text_emphasis)
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(highlighted_match_text(
                                            title,
                                            &query_for_click,
                                            theme.accent,
                                        )),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(theme.text_subtle)
                                        .child(metadata),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.text_subtle)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(result.relative_path.clone()),
                        ),
                ),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            index.track_open(&query_for_click, &path);
            this.open_file_at(path.clone(), result.line_number as usize, column, cx);
        }))
        .into_any_element()
}

fn content_preview<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    view: &FileSearchView,
    snapshot: Arc<::file_search::ContentSearchSnapshot>,
    _window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let selected = snapshot.results.get(view.selected_result);
    let title = selected
        .map(|result| format!("{}:{}", result.relative_path, result.line_number))
        .unwrap_or_else(|| "Preview".to_string());

    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .child(
            div()
                .flex_none()
                .h(rems(2.25))
                .px(rems(1.0))
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme.border_subtle)
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title),
                )
                .when(view.preview_loading, |this| {
                    this.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child("Loading..."),
                    )
                }),
        )
        .when_some(view.preview_error.clone(), |this, error| {
            this.child(error_banner(error, cx))
        })
        .when(view.preview_target.is_none(), |this| {
            this.child(centered_state("Select a content match", None, cx))
        })
        .when(
            view.preview_target.is_some() && view.preview_error.is_none(),
            |this| {
                this.child(
                    Input::new(&view.preview_input)
                        .appearance(false)
                        .bordered(false)
                        .focus_bordered(false)
                        .disabled(true)
                        .font_family(FONT_FAMILY)
                        .text_size(rems(0.875))
                        .size_full(),
                )
            },
        )
        .into_any_element()
}

fn centered_state<T: PaneDelegate + SettingsDelegate>(
    message: impl Into<SharedString>,
    focus_handle: Option<FocusHandle>,
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
        .when_some(focus_handle, |this, focus_handle| {
            this.track_focus(&focus_handle).on_mouse_down(
                gpui::MouseButton::Left,
                move |_, window, cx| {
                    window.focus(&focus_handle, cx);
                },
            )
        })
        .child(
            ComponentIcon::empty()
                .path(super::icon_for_kind(registry::FILE_SEARCH.id).path())
                .text_color(theme.text_muted),
        )
        .child(div().text_sm().child(message.into()))
        .into_any_element()
}

fn icon_for_result(result: &::file_search::FileSearchResult) -> IconName {
    super::icon_for_path(&result.absolute_path).unwrap_or(IconName::File)
}

fn icon_for_content_result(result: &::file_search::ContentSearchResult) -> IconName {
    super::icon_for_path(&result.absolute_path).unwrap_or(IconName::File)
}

fn highlighted_match_text(text: String, query: &str, color: gpui::Rgba) -> AnyElement {
    let ranges = match_ranges(&text, query);
    if ranges.is_empty() {
        return StyledText::new(text).into_any_element();
    }

    let style = HighlightStyle {
        color: Some(gpui::Hsla::from(color)),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };
    StyledText::new(text)
        .with_highlights(ranges.into_iter().map(|range| (range, style)))
        .into_any_element()
}

fn match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    for token in query.split_whitespace().filter_map(clean_query_token) {
        if let Some(range) = case_insensitive_substring(text, token) {
            ranges.push(range);
        } else {
            ranges.extend(fuzzy_match_ranges(text, token));
        }
    }

    merge_ranges(ranges)
}

fn clean_query_token(token: &str) -> Option<&str> {
    let token = token.trim_start_matches('!').trim_start_matches("./");
    if token.is_empty() || token.starts_with("git:") || token.starts_with("status:") {
        None
    } else {
        Some(token)
    }
}

fn case_insensitive_substring(text: &str, pattern: &str) -> Option<Range<usize>> {
    let pattern_chars = pattern.chars().collect::<Vec<_>>();
    if pattern_chars.is_empty() {
        return None;
    }

    let text_chars = char_spans(text);
    if pattern_chars.len() > text_chars.len() {
        return None;
    }

    for start_ix in 0..=text_chars.len() - pattern_chars.len() {
        if pattern_chars
            .iter()
            .enumerate()
            .all(|(offset, ch)| chars_equal(text_chars[start_ix + offset].2, *ch))
        {
            return Some(text_chars[start_ix].0..text_chars[start_ix + pattern_chars.len() - 1].1);
        }
    }

    None
}

fn fuzzy_match_ranges(text: &str, pattern: &str) -> Vec<Range<usize>> {
    let text_chars = char_spans(text);
    let mut ranges = Vec::new();
    let mut start_ix = 0;

    for query_ch in pattern.chars() {
        let Some((matched_ix, (start, end, _))) = text_chars
            .iter()
            .enumerate()
            .skip(start_ix)
            .find(|(_, (_, _, text_ch))| chars_equal(*text_ch, query_ch))
        else {
            return Vec::new();
        };
        ranges.push(*start..*end);
        start_ix = matched_ix + 1;
    }

    ranges
}

fn char_spans(text: &str) -> Vec<(usize, usize, char)> {
    let chars = text.char_indices().collect::<Vec<_>>();
    chars
        .iter()
        .enumerate()
        .map(|(ix, (start, ch))| {
            let end = chars
                .get(ix + 1)
                .map(|(next, _)| *next)
                .unwrap_or(text.len());
            (*start, end, *ch)
        })
        .collect()
}

fn chars_equal(left: char, right: char) -> bool {
    left == right || left.eq_ignore_ascii_case(&right)
}

fn byte_column_to_char_column(line: &str, byte_column: usize) -> usize {
    let byte_column = byte_column.min(line.len());
    let byte_column = if line.is_char_boundary(byte_column) {
        byte_column
    } else {
        line.char_indices()
            .map(|(ix, _)| ix)
            .take_while(|ix| *ix < byte_column)
            .last()
            .unwrap_or(0)
    };
    line[..byte_column].chars().count()
}

fn component_editor_language(language: &str) -> &str {
    match language {
        "shellscript" => "bash",
        "typescriptreact" => "tsx",
        "javascriptreact" => "javascript",
        other => other,
    }
}

fn merge_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }
        merged.push(range);
    }
    merged
}

fn format_size(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let size = size as f64;

    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{} B", size as u64)
    }
}

fn spawn_search<T: PaneDelegate + SettingsDelegate>(
    request: SearchRequest,
    delay: Duration,
    cx: &mut Context<T>,
) {
    cx.spawn(async move |this, cx| {
        if !delay.is_zero() {
            cx.background_executor().timer(delay).await;
        }

        let generation = request.generation;
        let kind = request.kind;
        let should_run = this
            .update(cx, |_, cx| {
                cx.update_global::<FileSearchUi, _>(|state, _| state.begin_search(generation, kind))
            })
            .unwrap_or(false);
        if !should_run {
            return;
        }

        let index = request.index.clone();
        let query = request.query.clone();
        let mode = request.mode;
        let limit = request.limit;
        let result = cx
            .background_executor()
            .spawn(async move {
                match mode {
                    SearchMode::Name => index
                        .search(&query, limit)
                        .map(SearchResult::Name)
                        .map_err(|error| error.to_string()),
                    SearchMode::Content => index
                        .search_content(&query, limit)
                        .map(SearchResult::Content)
                        .map_err(|error| error.to_string()),
                }
            })
            .await;

        let _ = this.update(cx, |_, cx| {
            let next = cx.update_global::<FileSearchUi, _>(|state, _| {
                state.finish_search(generation, result, kind)
            });
            cx.notify();
            if let Some(next) = next {
                spawn_search(next, Duration::ZERO, cx);
            }
        });
    })
    .detach();
}

fn schedule_scan_refresh<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) {
    let request = cx.update_global::<FileSearchUi, _>(|state, _| {
        if state.refresh_scheduled {
            return None;
        }

        let index = state.active_index.clone()?;
        state.refresh_scheduled = true;
        Some(SearchRequest {
            index,
            mode: state.mode,
            query: state.query.clone(),
            generation: state.generation,
            limit: state.result_limit,
            kind: SearchKind::Refresh,
        })
    });

    if let Some(request) = request {
        spawn_search(request, SCAN_REFRESH_INTERVAL, cx);
    }
}

fn sync_preview_soft_wrap<T: PaneDelegate + SettingsDelegate>(
    window: &mut Window,
    cx: &mut Context<T>,
) {
    let soft_wrap = preview_soft_wrap_enabled(cx);
    let input =
        cx.update_global::<FileSearchUi, _>(|state, _| state.sync_preview_soft_wrap(soft_wrap));
    if let Some(input) = input {
        input.update(cx, |input, cx| {
            input.set_soft_wrap(soft_wrap, window, cx);
        });
    }
}

fn preview_soft_wrap_enabled<T>(cx: &mut Context<T>) -> bool {
    cx.settings()
        .get(file_editor::SOFT_WRAP_SETTING_ID)
        .and_then(SettingValue::as_bool)
        .unwrap_or(false)
}

fn schedule_load_more<T: PaneDelegate + SettingsDelegate>(visible_end: usize, cx: &mut Context<T>) {
    let request =
        cx.update_global::<FileSearchUi, _>(|state, _| state.prepare_load_more(visible_end));
    if let Some(request) = request {
        spawn_search(request, Duration::ZERO, cx);
    }
}

fn schedule_preview_prefetch<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) {
    let request = cx.update_global::<FileSearchUi, _>(|state, _| state.prepare_preview_prefetch());
    if let Some(request) = request {
        spawn_preview_prefetch(request, cx);
    }
}

fn schedule_preview_load<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) {
    let request = cx.update_global::<FileSearchUi, _>(|state, _| state.prepare_preview());
    if let Some(request) = request {
        spawn_preview_load(request, cx);
    }
}

fn spawn_preview_load<T: PaneDelegate + SettingsDelegate>(
    request: PreviewRequest,
    cx: &mut Context<T>,
) {
    cx.spawn(async move |this, cx| {
        let generation = request.generation;
        let target = request.target.clone();
        let result = cx
            .background_executor()
            .spawn(async move { read_preview_file(&target.path) })
            .await;

        let _ = this.update(cx, |_, cx| {
            let updated = cx.update_global::<FileSearchUi, _>(|state, _| {
                state.finish_preview(generation, result)
            });
            if updated {
                cx.notify();
            }
        });
    })
    .detach();
}

fn read_preview_file(path: &Path) -> Result<PreviewFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("Unable to read preview: {error}"))?
        .replace("\r\n", "\n");
    let language = language::from_path(path)
        .map(|language| component_editor_language(language.as_str()).to_string())
        .unwrap_or_else(|| "plaintext".to_string());
    let line_starts = line_starts(&content);
    Ok(PreviewFile {
        content: Arc::<str>::from(content),
        language,
        line_starts: Arc::from(line_starts),
    })
}

fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        content
            .bytes()
            .enumerate()
            .filter_map(|(ix, byte)| (byte == b'\n').then_some(ix + 1)),
    );
    starts
}

fn preview_slice(
    file: &PreviewFile,
    start_line: usize,
    end_line: usize,
    trailing_padding: usize,
) -> Arc<str> {
    let line_count = file.line_starts.len().max(1);
    let start_line = start_line.min(line_count.saturating_sub(1));
    let end_line = end_line.max(start_line + 1).min(line_count);
    let start_byte = file.line_starts[start_line].min(file.content.len());
    let end_byte = if end_line < line_count {
        file.line_starts[end_line]
            .saturating_sub(1)
            .min(file.content.len())
    } else {
        file.content.len()
    };

    let mut content = String::from(&file.content[start_byte..end_byte]);
    if trailing_padding > 0 {
        content.push_str(&"\n".repeat(trailing_padding));
    }
    Arc::<str>::from(content)
}

fn spawn_preview_prefetch<T: PaneDelegate + SettingsDelegate>(
    request: PreviewPrefetchRequest,
    cx: &mut Context<T>,
) {
    cx.spawn(async move |this, cx| {
        let generation = request.generation;
        let files = request.files;
        let loaded = cx
            .background_executor()
            .spawn(async move {
                files
                    .into_iter()
                    .filter_map(|file| {
                        let preview_file = read_preview_file(&file.path).ok()?;
                        Some(PreviewFileData {
                            path: file.path,
                            file: preview_file,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .await;

        let _ = this.update(cx, |_, cx| {
            let updated = cx.update_global::<FileSearchUi, _>(|state, _| {
                state.finish_preview_prefetch(generation, loaded)
            });
            if updated {
                cx.notify();
            }
        });
    })
    .detach();
}

fn sync_preview_input<T: PaneDelegate + SettingsDelegate>(
    window: &mut Window,
    cx: &mut Context<T>,
) {
    let Some(update) = cx.update_global::<FileSearchUi, _>(|state, _| state.take_preview_update())
    else {
        return;
    };

    let target_line = update.cursor_line;

    update.input.update(cx, |input, cx| {
        input.set_line_number_start(update.line_number_base, cx);
        if let Some(content) = update.content {
            input.set_value(content.as_ref(), window, cx);
        }
        if update.language_changed {
            input.set_highlighter(update.language, cx);
        }
        input.set_cursor_position_without_focus(
            gpui_component::input::Position::new(target_line as u32, update.column as u32),
            cx,
        );
        if !input.center_cursor_in_view(cx) {
            let line_height = input
                .line_height()
                .unwrap_or_else(|| rems(1.25).to_pixels(window.rem_size()));
            let centered_top_line = target_line.saturating_sub(PREVIEW_FALLBACK_CENTER_OFFSET);
            input.set_scroll_offset(
                point(px(0.0), -(line_height * centered_top_line as f32)),
                cx,
            );
        }
    });
}

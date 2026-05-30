use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui::{
    AnyElement, AppContext, Context, Entity, FocusHandle, FontWeight, Global, HighlightStyle,
    IntoElement, KeyDownEvent, ListSizingBehavior, ScrollStrategy, SharedString, StyledText,
    Window, div, prelude::*, px, rems, size,
};
use gpui_component::{
    Icon as ComponentIcon, IconName as ComponentIconName, Sizable, Size, VirtualListScrollHandle,
    alert::Alert,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    v_virtual_list,
};
use icons::IconName;

use tabs::registry;
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, SettingsDelegate};

const RESULT_LIMIT: usize = 50;
const RESULT_ROW_HEIGHT_REM: f32 = 3.125;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(80);
const SCAN_REFRESH_INTERVAL: Duration = Duration::from_millis(200);

pub struct FileSearchUi {
    input: Entity<InputState>,
    focus_handle: FocusHandle,
    indices: HashMap<PathBuf, Arc<::file_search::FileSearchIndex>>,
    active_root: Option<PathBuf>,
    active_index: Option<Arc<::file_search::FileSearchIndex>>,
    query: String,
    snapshot: Option<Arc<::file_search::FileSearchSnapshot>>,
    error: Option<String>,
    loading: bool,
    search_running: bool,
    selected_result: usize,
    generation: u64,
    refresh_scheduled: bool,
    scroll_handle: VirtualListScrollHandle,
}

#[derive(Clone)]
struct FileSearchView {
    index: Option<Arc<::file_search::FileSearchIndex>>,
    focus_handle: FocusHandle,
    query: String,
    snapshot: Option<Arc<::file_search::FileSearchSnapshot>>,
    error: Option<String>,
    loading: bool,
    selected_result: usize,
    scroll_handle: VirtualListScrollHandle,
}

#[derive(Clone)]
struct SearchRequest {
    index: Arc<::file_search::FileSearchIndex>,
    query: String,
    generation: u64,
}

impl FileSearchUi {
    pub fn install<T: 'static>(window: &mut Window, cx: &mut Context<T>) {
        if cx.try_global::<Self>().is_some() {
            return;
        }

        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search files by name or path"));
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();

        cx.set_global(Self {
            input,
            focus_handle: cx.focus_handle().tab_stop(true),
            indices: HashMap::new(),
            active_root: None,
            active_index: None,
            query: String::new(),
            snapshot: None,
            error: None,
            loading: false,
            search_running: false,
            selected_result: 0,
            generation: 0,
            refresh_scheduled: false,
            scroll_handle: VirtualListScrollHandle::new(),
        });
    }

    fn input(&self) -> Entity<InputState> {
        self.input.clone()
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
        self.refresh_scheduled = false;
        if root_changed || query_changed {
            self.selected_result = 0;
            self.scroll_handle = VirtualListScrollHandle::new();
        }

        Some(SearchRequest {
            index,
            query,
            generation: self.generation,
        })
    }

    fn view(&self) -> FileSearchView {
        FileSearchView {
            index: self.active_index.clone(),
            focus_handle: self.focus_handle.clone(),
            query: self.query.clone(),
            snapshot: self.snapshot.clone(),
            error: self.error.clone(),
            loading: self.loading,
            selected_result: self.selected_result,
            scroll_handle: self.scroll_handle.clone(),
        }
    }

    fn selected_open_target(
        &self,
    ) -> Option<(Arc<::file_search::FileSearchIndex>, String, PathBuf)> {
        let index = self.active_index.clone()?;
        let snapshot = self.snapshot.as_ref()?;
        let result = snapshot.results.get(self.selected_result)?;
        Some((index, snapshot.query.clone(), result.absolute_path.clone()))
    }

    fn move_selection(&mut self, direction: isize) -> bool {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return false;
        };
        if snapshot.results.is_empty() {
            return false;
        }

        let last = snapshot.results.len() - 1;
        let next = if direction < 0 {
            self.selected_result.saturating_sub(1)
        } else {
            (self.selected_result + 1).min(last)
        };
        if next == self.selected_result {
            return false;
        }

        self.selected_result = next;
        self.scroll_handle
            .scroll_to_item(self.selected_result, ScrollStrategy::Nearest);
        true
    }

    fn begin_search(&mut self, generation: u64, is_refresh: bool) -> bool {
        if self.generation != generation || self.search_running {
            if is_refresh {
                self.refresh_scheduled = false;
            }
            return false;
        }

        self.search_running = true;
        true
    }

    fn finish_search(
        &mut self,
        generation: u64,
        result: Result<::file_search::FileSearchSnapshot, String>,
        is_refresh: bool,
    ) -> Option<SearchRequest> {
        self.search_running = false;
        if is_refresh {
            self.refresh_scheduled = false;
        }

        if self.generation != generation {
            return self.active_index.clone().map(|index| SearchRequest {
                index,
                query: self.query.clone(),
                generation: self.generation,
            });
        }

        self.loading = false;
        match result {
            Ok(snapshot) => {
                self.selected_result = self
                    .selected_result
                    .min(snapshot.results.len().saturating_sub(1));
                self.snapshot = Some(Arc::new(snapshot));
                self.error = None;
            }
            Err(error) => {
                self.snapshot = None;
                self.error = Some(error);
            }
        }

        None
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
    let query = input.read(cx).value().trim().to_string();
    let root = workspace_path.to_path_buf();
    if let Some(request) =
        cx.update_global::<FileSearchUi, _>(|state, _| state.prepare_search(root, query))
    {
        spawn_search(request, SEARCH_DEBOUNCE, false, cx);
    }
    let view = cx.global::<FileSearchUi>().view();

    if view.query.is_empty()
        && view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.is_scanning)
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
                    let Some((index, query, path)) =
                        cx.global::<FileSearchUi>().selected_open_target()
                    else {
                        return;
                    };
                    cx.stop_propagation();
                    index.track_open(&query, &path);
                    this.open_file(path, cx);
                }
                _ => {}
            }
        }))
        .child(header(&input, cx))
        .when_some(view.error.clone(), |this, error| {
            this.child(error_banner(error, cx))
        })
        .child(results(&view, focus_handle, window, cx))
        .into_any_element()
}

fn header<T: PaneDelegate + SettingsDelegate>(
    input: &Entity<InputState>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();

    div()
        .flex_none()
        .border_b_1()
        .border_color(theme.border_subtle)
        .p(rems(1.5))
        .child(
            div().w_full().child(
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
            return centered_state("Searching files...", Some(focus_handle), cx);
        }

        return centered_state(
            "Unable to load the file search index",
            Some(focus_handle),
            cx,
        );
    };

    if snapshot.results.is_empty() {
        if snapshot.is_scanning {
            return centered_state("Indexing workspace files...", Some(focus_handle), cx);
        }

        if snapshot.query.is_empty() {
            return centered_state(
                "Start typing to search workspace files",
                Some(focus_handle),
                cx,
            );
        }

        return centered_state(
            format!("No files found for \"{}\"", snapshot.query),
            Some(focus_handle),
            cx,
        );
    }

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
    let snapshot = snapshot.clone();
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
                        .child(
                            ComponentIcon::empty()
                                .path(icon.path())
                                .text_color(theme.text_muted),
                        ),
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
    is_refresh: bool,
    cx: &mut Context<T>,
) {
    cx.spawn(async move |this, cx| {
        if !delay.is_zero() {
            cx.background_executor().timer(delay).await;
        }

        let generation = request.generation;
        let should_run = this
            .update(cx, |_, cx| {
                cx.update_global::<FileSearchUi, _>(|state, _| {
                    state.begin_search(generation, is_refresh)
                })
            })
            .unwrap_or(false);
        if !should_run {
            return;
        }

        let index = request.index.clone();
        let query = request.query.clone();
        let result = cx
            .background_executor()
            .spawn(async move {
                index
                    .search(&query, RESULT_LIMIT)
                    .map_err(|error| error.to_string())
            })
            .await;

        let _ = this.update(cx, |_, cx| {
            let next = cx.update_global::<FileSearchUi, _>(|state, _| {
                state.finish_search(generation, result, is_refresh)
            });
            cx.notify();
            if let Some(next) = next {
                spawn_search(next, Duration::ZERO, false, cx);
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
            query: state.query.clone(),
            generation: state.generation,
        })
    });

    if let Some(request) = request {
        spawn_search(request, SCAN_REFRESH_INTERVAL, true, cx);
    }
}

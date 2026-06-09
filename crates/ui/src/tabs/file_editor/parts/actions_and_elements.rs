use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext, Bounds, ClipboardItem, Context, DragMoveEvent, Empty, Entity,
    EntityId, EntityInputHandler, Focusable, Global, Half, Hsla, InteractiveElement as _,
    IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, Render,
    SharedString, StatefulInteractiveElement as _, Subscription, Task, Window, div,
    linear_color_stop, linear_gradient, prelude::*, px, relative, rems,
};
use gpui_component::ElementExt;
use gpui_component::highlighter::Diagnostic as ComponentDiagnostic;
use gpui_component::input::{
    DocumentColorProvider, HoverProvider, Input, InputEvent, InputState, Rope, RopeExt, TabSize,
};
use lsp_types::{
    Color, ColorInformation, CompletionItem, CompletionItemKind, CompletionResponse,
    CompletionTextEdit, Diagnostic as LspDiagnostic, Documentation, Position as LspPosition,
};

use file_editor::{Buffer, BufferId, BufferStore, soft_wrap_enabled};
use file_tree::ActiveFileTree;
use icons::{Icon, IconName};
use tabs::{Tab, registry};
use theme::{ActiveTheme, Theme};

const COMPONENT_INPUT_KEY_CONTEXT: &str = "Input";
const FONT_FAMILY: &str = "DejaVu Sans Mono";
const FONT_SIZE_REM: f32 = 0.875;
const TAB_SIZE_COLUMNS: usize = 4;
const COMPLETION_MENU_MAX_ITEMS: usize = 50;
const COMPLETION_MENU_VISIBLE_ITEMS: usize = 11;
const COMPLETION_MENU_MIN_WIDTH: Pixels = px(180.0);
const COMPLETION_MENU_MAX_WIDTH: Pixels = px(520.0);
const COMPLETION_MENU_CHAR_WIDTH: f32 = 7.0;
const COMPLETION_CURSOR_CHAR_WIDTH_FACTOR: f32 = 0.62;
const COMPLETION_MENU_HORIZONTAL_PADDING: Pixels = px(48.0);
const MAX_COMPLETION_DETAIL_CHARS: usize = 56;
const COLOR_PICKER_WIDTH: Pixels = px(380.0);
const COLOR_PICKER_SPECTRUM_HEIGHT: Pixels = px(254.0);
const COLOR_PICKER_SLIDER_HEIGHT: Pixels = px(12.0);
const COLOR_PICKER_THUMB_SIZE: Pixels = px(16.0);
const COLOR_PICKER_SLIDER_THUMB_WIDTH: Pixels = px(10.0);
const COLOR_PICKER_SLIDER_THUMB_HEIGHT: Pixels = px(18.0);
const COLOR_SWATCH_SIZE: Pixels = px(12.0);
const COLOR_SWATCH_GAP: Pixels = px(4.0);
const DIAGNOSTIC_REQUEST_DEBOUNCE: Duration = Duration::from_millis(250);
// rust-analyzer can publish cargo-check diagnostics well after didChange.
const DIAGNOSTIC_FOLLOW_UP_DELAY: Duration = Duration::from_millis(100);
const DIAGNOSTIC_FOLLOW_UP_REQUESTS: usize = 30;

#[derive(Default)]
struct ComponentEditorStore {
    inputs: HashMap<usize, ComponentEditorInput>,
    pending_reveals: HashMap<PathBuf, EditorReveal>,
}

#[derive(Clone, Copy)]
struct EditorReveal {
    line: usize,
    column: usize,
}

struct ComponentEditorInput {
    input: Entity<InputState>,
    completions: Entity<ComponentCompletionMenu>,
    color_picker: Entity<ComponentColorPicker>,
    diagnostics: Entity<ComponentDiagnostics>,
    _buffer_observer: Subscription,
    _input_observer: Subscription,
    buffer_id: BufferId,
    soft_wrap: bool,
    root: Option<PathBuf>,
}

#[derive(Clone)]
struct ComponentEditorEntities {
    input: Entity<InputState>,
    completions: Entity<ComponentCompletionMenu>,
    color_picker: Entity<ComponentColorPicker>,
}

#[derive(Clone)]
struct ComponentLspProvider {
    buffer: Entity<Buffer>,
    root: Option<PathBuf>,
}

#[derive(Clone)]
struct ComponentColorProvider {
    buffer: Entity<Buffer>,
}

struct ComponentCompletionMenu {
    input: Entity<InputState>,
    buffer: Entity<Buffer>,
    root: Option<PathBuf>,
    items: Vec<ComponentCompletionItem>,
    replace_range: Range<usize>,
    width: Option<Pixels>,
    selected: usize,
    first_visible: usize,
    request_id: u64,
    suppress_next_request: bool,
}

struct ComponentDiagnostics {
    input: Entity<InputState>,
    buffer: Entity<Buffer>,
    root: Option<PathBuf>,
    request_id: u64,
    last_epoch: u64,
    buffer_was_dirty: bool,
}

struct ComponentColorPicker {
    input: Entity<InputState>,
    buffer: Entity<Buffer>,
    active: Option<ComponentCssColor>,
    active_hue: Option<f32>,
    active_saturation: Option<f32>,
    last_scroll_offset: Option<Point<Pixels>>,
    picker_anchor: Option<(Pixels, Pixels)>,
    overlay_bounds: Option<Bounds<Pixels>>,
    spectrum_bounds: Option<Bounds<Pixels>>,
    hue_bounds: Option<Bounds<Pixels>>,
    alpha_bounds: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ComponentColorDragKind {
    Spectrum,
    Hue,
    Alpha,
}

#[derive(Clone)]
struct ComponentColorDrag {
    entity_id: EntityId,
    kind: ComponentColorDragKind,
}

#[derive(Clone, Copy)]
struct ComponentColorHsva {
    h: f32,
    s: f32,
    v: f32,
    a: f32,
}

impl Render for ComponentColorDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone)]
struct ComponentCompletionItem {
    item: CompletionItem,
    label: String,
    detail: Option<String>,
}

impl ComponentEditorStore {
    fn state_for_tab(
        tab_id: usize,
        buffer: &Entity<Buffer>,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut App,
    ) -> ComponentEditorEntities {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }

        let buffer_id = buffer.read(cx).id();
        let soft_wrap = soft_wrap_enabled(cx);

        if let Some((
            input,
            completions,
            color_picker,
            diagnostics,
            previous_soft_wrap,
            previous_root,
        )) = cx
            .global::<Self>()
            .inputs
            .get(&tab_id)
            .filter(|existing| existing.buffer_id == buffer_id)
            .map(|existing| {
                (
                    existing.input.clone(),
                    existing.completions.clone(),
                    existing.color_picker.clone(),
                    existing.diagnostics.clone(),
                    existing.soft_wrap,
                    existing.root.clone(),
                )
            })
        {
            Self::sync_soft_wrap(tab_id, &input, previous_soft_wrap, soft_wrap, window, cx);
            Self::sync_lsp_state(
                tab_id,
                &input,
                &completions,
                &diagnostics,
                buffer,
                previous_root,
                root,
                cx,
            );
            if Self::sync_from_buffer(&input, buffer, window, cx) {
                ComponentDiagnostics::request(&diagnostics, cx);
            }
            return ComponentEditorEntities {
                input,
                completions,
                color_picker,
            };
        }

        let (initial, language) = {
            let buffer = buffer.read(cx);
            (
                buffer.content().to_string(),
                buffer
                    .language()
                    .map(|language| component_editor_language(language.as_str()).to_string())
                    .unwrap_or_else(|| "plaintext".to_string()),
            )
        };

        let lsp_provider = Rc::new(ComponentLspProvider {
            buffer: buffer.clone(),
            root: root.clone(),
        });
        let color_provider = Rc::new(ComponentColorProvider {
            buffer: buffer.clone(),
        });
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .code_editor(language)
                .multi_line(true)
                .soft_wrap(soft_wrap)
                .tab_size(TabSize {
                    tab_size: TAB_SIZE_COLUMNS,
                    ..Default::default()
                })
                .default_value(initial);
            input.lsp.hover_provider = Some(lsp_provider);
            input.lsp.document_color_provider = Some(color_provider);
            input
        });
        let completions =
            cx.new(|_| ComponentCompletionMenu::new(input.clone(), buffer.clone(), root.clone()));
        let color_picker =
            cx.new(|cx| ComponentColorPicker::new(input.clone(), buffer.clone(), window, cx));
        let buffer_was_dirty = buffer.read(cx).is_dirty();
        let diagnostics = cx.new(|_| {
            ComponentDiagnostics::new(
                input.clone(),
                buffer.clone(),
                root.clone(),
                buffer_was_dirty,
            )
        });
        let diagnostics_for_buffer = diagnostics.clone();
        let buffer_observer = cx.observe(buffer, move |_, cx| {
            diagnostics_for_buffer
                .update(cx, |diagnostics, cx| diagnostics.handle_buffer_change(cx));
        });
        let color_picker_for_input = color_picker.clone();
        let input_observer = cx.observe(&input, move |_, cx| {
            color_picker_for_input.update(cx, |picker, cx| picker.handle_input_update(cx));
        });

        let input_for_sub = input.clone();
        let buffer_for_sub = buffer.clone();
        let completions_for_sub = completions.clone();
        let diagnostics_for_sub = diagnostics.clone();
        cx.subscribe(&input, move |_, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let value = input_for_sub.read(cx).value().to_string();
            buffer_for_sub.update(cx, |buffer, cx| {
                let current_len = buffer.content().len();
                if buffer.content() != value {
                    buffer.replace_range(0..current_len, &value, cx);
                }
            });
            ComponentCompletionMenu::request(&completions_for_sub, cx);
            ComponentDiagnostics::request(&diagnostics_for_sub, cx);
        })
        .detach();

        cx.update_global::<Self, _>(|store, _| {
            store.inputs.insert(
                tab_id,
                ComponentEditorInput {
                    input: input.clone(),
                    completions: completions.clone(),
                    color_picker: color_picker.clone(),
                    diagnostics: diagnostics.clone(),
                    _buffer_observer: buffer_observer,
                    _input_observer: input_observer,
                    buffer_id,
                    soft_wrap,
                    root,
                },
            );
        });

        ComponentDiagnostics::request(&diagnostics, cx);

        ComponentEditorEntities {
            input,
            completions,
            color_picker,
        }
    }

    fn drop_tab(tab_id: usize, cx: &mut App) {
        if cx.try_global::<Self>().is_none() {
            return;
        }
        cx.update_global::<Self, _>(|store, _| {
            store.inputs.remove(&tab_id);
        });
    }

    fn request_reveal(path: PathBuf, line: usize, column: usize, cx: &mut App) {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        cx.update_global::<Self, _>(|store, _| {
            store
                .pending_reveals
                .insert(path, EditorReveal { line, column });
        });
    }

    fn take_pending_reveal(path: &Path, cx: &mut App) -> Option<EditorReveal> {
        cx.try_global::<Self>()?;
        cx.update_global::<Self, _>(|store, _| store.pending_reveals.remove(path))
    }

    fn sync_soft_wrap(
        tab_id: usize,
        input: &Entity<InputState>,
        previous: bool,
        current: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if previous == current {
            return;
        }

        input.update(cx, |input, cx| input.set_soft_wrap(current, window, cx));
        cx.update_global::<Self, _>(|store, _| {
            if let Some(existing) = store.inputs.get_mut(&tab_id) {
                existing.soft_wrap = current;
            }
        });
    }

    fn sync_lsp_state(
        tab_id: usize,
        input: &Entity<InputState>,
        completions: &Entity<ComponentCompletionMenu>,
        diagnostics: &Entity<ComponentDiagnostics>,
        buffer: &Entity<Buffer>,
        previous_root: Option<PathBuf>,
        current_root: Option<PathBuf>,
        cx: &mut App,
    ) {
        if previous_root == current_root {
            return;
        }

        let lsp_provider = Rc::new(ComponentLspProvider {
            buffer: buffer.clone(),
            root: current_root.clone(),
        });
        input.update(cx, |input, _| {
            input.lsp.hover_provider = Some(lsp_provider);
        });
        completions.update(cx, |menu, cx| {
            menu.buffer = buffer.clone();
            menu.root = current_root.clone();
            menu.clear(cx);
        });
        diagnostics.update(cx, |diagnostics, cx| {
            diagnostics.buffer = buffer.clone();
            diagnostics.root = current_root.clone();
            diagnostics.clear(cx);
        });
        cx.update_global::<Self, _>(|store, _| {
            if let Some(existing) = store.inputs.get_mut(&tab_id) {
                existing.root = current_root;
            }
        });
        ComponentDiagnostics::request(diagnostics, cx);
    }

    fn sync_from_buffer(
        input: &Entity<InputState>,
        buffer: &Entity<Buffer>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let buffer_text = buffer.read(cx).content().to_string();
        let input_state = input.read(cx);
        if input_state.has_marked_text() {
            return false;
        }

        if input_state.value().as_ref() != buffer_text.as_str() {
            input.update(cx, |input, cx| input.set_value(buffer_text, window, cx));
            return true;
        }
        false
    }
}

impl Global for ComponentEditorStore {}

impl ComponentCompletionMenu {
    fn new(input: Entity<InputState>, buffer: Entity<Buffer>, root: Option<PathBuf>) -> Self {
        Self {
            input,
            buffer,
            root,
            items: Vec::new(),
            replace_range: 0..0,
            width: None,
            selected: 0,
            first_visible: 0,
            request_id: 0,
            suppress_next_request: false,
        }
    }

    fn request(menu: &Entity<Self>, cx: &mut App) {
        let pending = menu.update(cx, |menu, cx| {
            if menu.suppress_next_request {
                menu.suppress_next_request = false;
                menu.clear(cx);
                return None;
            }

            menu.request_id += 1;
            let request_id = menu.request_id;

            let content = menu.input.read(cx).value().to_string();
            let offset = menu.input.read(cx).cursor();
            let raw_query = completion_raw_query_for_offset(&content, offset);
            if !completion_should_request(&raw_query) {
                menu.clear(cx);
                return None;
            }

            let query = completion_filter_query(&raw_query);
            let replace_range = offset.saturating_sub(query.len())..offset;
            menu.replace_range = replace_range.clone();
            let position = Rope::from(content.as_str()).offset_to_position(offset);
            let request = {
                let buffer = menu.buffer.read(cx);
                let Some(language) = buffer.language() else {
                    menu.clear(cx);
                    return None;
                };
                let language_id = language.as_str().to_string();
                if !lsp::has_installed_server(&language_id) {
                    menu.clear(cx);
                    return None;
                }
                let path = buffer.path().to_path_buf();
                let Some(root) = menu
                    .root
                    .clone()
                    .or_else(|| path.parent().map(Path::to_path_buf))
                else {
                    menu.clear(cx);
                    return None;
                };

                lsp::CompletionRequest {
                    root,
                    path,
                    language_id,
                    content,
                    position: lsp::Position {
                        line: position.line,
                        character: position.character,
                    },
                }
            };

            Some((request_id, request, query, replace_range))
        });

        let Some((request_id, request, query, replace_range)) = pending else {
            return;
        };

        let menu = menu.clone();
        cx.spawn(async move |cx| {
            let response = cx
                .background_executor()
                .spawn(async move { lsp::completion(request) })
                .await;
            let _ = menu.update(cx, |menu, cx| {
                if menu.request_id != request_id {
                    return;
                }

                let mut response = match response {
                    Ok(Some(response)) => response,
                    Ok(None) | Err(_) => empty_completion_response(),
                };
                enhance_completion_response(&mut response);
                let response = rank_completion_response(response, &query);
                menu.set_response(response, replace_range, cx);
            });
        })
        .detach();
    }

    fn set_response(
        &mut self,
        response: CompletionResponse,
        replace_range: Range<usize>,
        cx: &mut Context<Self>,
    ) {
        self.items = completion_response_items(response)
            .into_iter()
            .take(COMPLETION_MENU_MAX_ITEMS)
            .map(ComponentCompletionItem::new)
            .collect();
        self.replace_range = replace_range;
        if self.items.is_empty() {
            self.width = None;
        } else {
            let desired_width = completion_menu_desired_width(&self.items);
            self.width = Some(
                self.width
                    .map_or(desired_width, |width| width.max(desired_width)),
            );
        }
        self.selected = 0;
        self.first_visible = 0;
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.clear_without_notify();
        cx.notify();
    }

    fn clear_without_notify(&mut self) {
        self.request_id += 1;
        self.items.clear();
        self.width = None;
        self.selected = 0;
        self.first_visible = 0;
    }

    fn is_open(&self) -> bool {
        !self.items.is_empty()
    }

    fn select_previous(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_open() {
            return false;
        }
        self.selected = self.selected.saturating_sub(1);
        self.keep_selected_visible();
        cx.notify();
        true
    }

    fn select_next(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_open() {
            return false;
        }
        self.selected = (self.selected + 1).min(self.items.len().saturating_sub(1));
        self.keep_selected_visible();
        cx.notify();
        true
    }

    fn scroll(&mut self, lines: i32, cx: &mut Context<Self>) -> bool {
        if !self.is_open() || self.items.len() <= COMPLETION_MENU_VISIBLE_ITEMS || lines == 0 {
            return false;
        }

        let max_first = self
            .items
            .len()
            .saturating_sub(COMPLETION_MENU_VISIBLE_ITEMS);
        self.first_visible =
            (self.first_visible as i32 + lines).clamp(0, max_first as i32) as usize;
        self.selected = self
            .selected
            .clamp(self.first_visible, self.last_visible().saturating_sub(1));
        cx.notify();
        true
    }

    fn keep_selected_visible(&mut self) {
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.last_visible() {
            self.first_visible = self.selected + 1 - COMPLETION_MENU_VISIBLE_ITEMS;
        }
    }

    fn last_visible(&self) -> usize {
        (self.first_visible + COMPLETION_MENU_VISIBLE_ITEMS).min(self.items.len())
    }

    fn hide(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_open() {
            return false;
        }
        self.clear(cx);
        true
    }

    fn accept_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.items.get(self.selected).cloned() else {
            return false;
        };
        self.suppress_next_request = true;
        self.apply_completion(&entry.item, window, cx);
        self.clear(cx);
        true
    }

    fn accept_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.selected = index;
        self.keep_selected_visible();
        self.accept_selected(window, cx)
    }

    fn apply_completion(&self, item: &CompletionItem, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.input.read(cx).value().to_string();
        let (range, new_text) =
            completion_edit_for_item(item, &content, self.replace_range.clone());
        let utf16_range = component_range_to_utf16(&content, &range);
        self.input.update(cx, |input, cx| {
            EntityInputHandler::replace_text_in_range(
                input,
                Some(utf16_range),
                &new_text,
                window,
                cx,
            );
        });
    }
}

impl ComponentDiagnostics {
    fn new(
        input: Entity<InputState>,
        buffer: Entity<Buffer>,
        root: Option<PathBuf>,
        buffer_was_dirty: bool,
    ) -> Self {
        Self {
            input,
            buffer,
            root,
            request_id: 0,
            last_epoch: 0,
            buffer_was_dirty,
        }
    }

    fn handle_buffer_change(&mut self, cx: &mut Context<Self>) {
        let is_dirty = self.buffer.read(cx).is_dirty();
        if is_dirty {
            self.buffer_was_dirty = true;
            return;
        }

        if self.buffer_was_dirty {
            self.buffer_was_dirty = false;
            self.did_save(cx);
        }
    }

    fn did_save(&mut self, cx: &mut Context<Self>) {
        let content = self.input.read(cx).value().to_string();
        let Some(request) = self.save_request_for_content(content, cx) else {
            return;
        };
        let previous_epoch = self.last_epoch;
        let entity = cx.entity().clone();

        cx.spawn(async move |_, cx| {
            let _ = cx
                .background_executor()
                .spawn(async move { lsp::did_save(request) })
                .await;
            let _ = cx.update(|cx| {
                Self::request_after(
                    &entity,
                    cx,
                    Duration::ZERO,
                    Some(previous_epoch),
                    DIAGNOSTIC_FOLLOW_UP_REQUESTS,
                    false,
                );
            });
        })
        .detach();
    }

    fn request(diagnostics: &Entity<Self>, cx: &mut App) {
        Self::request_after(
            diagnostics,
            cx,
            DIAGNOSTIC_REQUEST_DEBOUNCE,
            None,
            DIAGNOSTIC_FOLLOW_UP_REQUESTS,
            true,
        );
    }

    fn request_after(
        diagnostics: &Entity<Self>,
        cx: &mut App,
        delay: Duration,
        previous_epoch: Option<u64>,
        follow_up_requests: usize,
        clear_stale: bool,
    ) {
        let pending = diagnostics.update(cx, |diagnostics, cx| {
            diagnostics.request_id += 1;
            let request_id = diagnostics.request_id;
            let content = diagnostics.input.read(cx).value().to_string();
            if clear_stale {
                diagnostics.clear_input(cx);
            }
            let request = diagnostics.request_for_content(content.clone(), previous_epoch, cx)?;

            Some((request_id, content, request))
        });

        let Some((request_id, content, request)) = pending else {
            return;
        };

        let diagnostics = diagnostics.clone();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(delay).await;

            let should_request = diagnostics.update(cx, |diagnostics, cx| {
                diagnostics.request_id == request_id
                    && diagnostics.input.read(cx).value().as_ref() == content.as_str()
            });
            if !should_request {
                return;
            }

            let response = cx
                .background_executor()
                .spawn(async move { lsp::diagnostics(request) })
                .await;

            let response_epoch = diagnostics.update(cx, |diagnostics, cx| {
                if diagnostics.request_id != request_id {
                    return None;
                }
                if diagnostics.input.read(cx).value().as_ref() != content.as_str() {
                    return None;
                }

                match response {
                    Ok(response) => {
                        let epoch = response.epoch;
                        if response.fresh {
                            diagnostics.apply(content.as_str(), response.diagnostics, epoch, cx);
                        } else if previous_epoch.is_none() {
                            diagnostics.clear_input(cx);
                        }
                        Some(epoch)
                    }
                    Err(_) => {
                        if previous_epoch.is_none() {
                            diagnostics.clear_input(cx);
                        }
                        None
                    }
                }
            });

            let next_previous_epoch = response_epoch.or(previous_epoch);
            if follow_up_requests > 0 && next_previous_epoch.is_some() {
                let diagnostics = diagnostics.clone();
                let _ = cx.update(|cx| {
                    Self::request_after(
                        &diagnostics,
                        cx,
                        DIAGNOSTIC_FOLLOW_UP_DELAY,
                        next_previous_epoch,
                        follow_up_requests - 1,
                        false,
                    );
                });
            }
        })
        .detach();
    }

    fn request_for_content(
        &mut self,
        content: String,
        previous_epoch: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Option<lsp::DiagnosticsRequest> {
        let buffer = self.buffer.read(cx);
        let Some(language) = buffer.language() else {
            self.clear(cx);
            return None;
        };
        let language_id = language.as_str().to_string();
        if !lsp::has_installed_server(&language_id) {
            self.clear(cx);
            return None;
        }

        let path = buffer.path().to_path_buf();
        let Some(root) = self
            .root
            .clone()
            .or_else(|| path.parent().map(Path::to_path_buf))
        else {
            self.clear(cx);
            return None;
        };

        Some(lsp::DiagnosticsRequest {
            root,
            path,
            language_id,
            content,
            previous_epoch,
        })
    }

    fn save_request_for_content(
        &mut self,
        content: String,
        cx: &mut Context<Self>,
    ) -> Option<lsp::SaveRequest> {
        let buffer = self.buffer.read(cx);
        let Some(language) = buffer.language() else {
            return None;
        };
        let language_id = language.as_str().to_string();
        if !lsp::has_installed_server(&language_id) {
            return None;
        }

        let path = buffer.path().to_path_buf();
        let Some(root) = self
            .root
            .clone()
            .or_else(|| path.parent().map(Path::to_path_buf))
        else {
            return None;
        };

        Some(lsp::SaveRequest {
            root,
            path,
            language_id,
            content,
        })
    }

    fn apply(
        &mut self,
        content: &str,
        diagnostics: Vec<LspDiagnostic>,
        epoch: u64,
        cx: &mut Context<Self>,
    ) {
        self.last_epoch = epoch;
        let diagnostics = component_diagnostics_from_lsp(diagnostics, content);
        self.input.update(cx, |input, cx| {
            if let Some(set) = input.diagnostics_mut() {
                set.clear();
                set.extend(diagnostics);
            }
            cx.notify();
        });
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.request_id += 1;
        self.clear_input(cx);
    }

    fn clear_input(&self, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            if let Some(set) = input.diagnostics_mut() {
                if !set.is_empty() {
                    set.clear();
                    cx.notify();
                }
            }
        });
    }
}

impl ComponentColorPicker {
    fn new(
        input: Entity<InputState>,
        buffer: Entity<Buffer>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            input,
            buffer,
            active: None,
            active_hue: None,
            active_saturation: None,
            last_scroll_offset: None,
            picker_anchor: None,
            overlay_bounds: None,
            spectrum_bounds: None,
            hue_bounds: None,
            alpha_bounds: None,
        }
    }

    fn open_color(&mut self, color: ComponentCssColor, cx: &mut Context<Self>) {
        self.last_scroll_offset = Some(self.input.read(cx).scroll_offset());
        self.picker_anchor = self.picker_anchor_for_range(&color.range, cx);
        let hsva = component_color_to_hsva(color.color);
        self.active_hue = Some(hsva.h);
        self.active_saturation = Some(hsva.s);
        self.active = Some(color);
        cx.notify();
    }

    fn picker_anchor_for_range(
        &self,
        range: &Range<usize>,
        cx: &mut Context<Self>,
    ) -> Option<(Pixels, Pixels)> {
        let input = self.input.read(cx);
        let bounds = input.range_to_bounds(range)?;
        let origin = self.overlay_bounds?.origin;
        Some((
            (bounds.origin.x - origin.x).max(px(0.0)),
            (bounds.origin.y - origin.y + bounds.size.height + px(2.0)).max(px(0.0)),
        ))
    }

    fn handle_input_update(&mut self, cx: &mut Context<Self>) {
        let scroll_offset = self.input.read(cx).scroll_offset();
        let scrolled = self
            .last_scroll_offset
            .is_some_and(|last_scroll_offset| last_scroll_offset != scroll_offset);
        self.last_scroll_offset = Some(scroll_offset);

        if scrolled {
            self.close(cx);
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.active.is_some() {
            self.active = None;
            self.active_hue = None;
            self.active_saturation = None;
            self.picker_anchor = None;
            cx.notify();
        }
    }

    fn apply_from_position(
        &mut self,
        kind: ComponentColorDragKind,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active.as_ref() else {
            return;
        };
        let mut color = component_color_to_hsva(active.color);
        if let Some(hue) = self.active_hue {
            color.h = hue;
        }
        if let Some(saturation) = self.active_saturation {
            color.s = saturation;
        }

        match kind {
            ComponentColorDragKind::Spectrum => {
                let Some(bounds) = self.spectrum_bounds else {
                    return;
                };
                color.s = component_color_unit((position.x - bounds.left()) / bounds.size.width);
                color.v =
                    component_color_unit(1.0 - (position.y - bounds.top()) / bounds.size.height);
                self.active_saturation = Some(color.s);
            }
            ComponentColorDragKind::Hue => {
                let Some(bounds) = self.hue_bounds else {
                    return;
                };
                color.h = component_color_unit((position.x - bounds.left()) / bounds.size.width);
                self.active_hue = Some(color.h);
            }
            ComponentColorDragKind::Alpha => {
                let Some(bounds) = self.alpha_bounds else {
                    return;
                };
                color.a = component_color_unit((position.x - bounds.left()) / bounds.size.width);
            }
        }

        self.active_hue = Some(color.h);
        self.active_saturation = Some(color.s);
        self.apply_color(component_color_from_hsva(color), window, cx);
    }

    fn apply_color(&mut self, color: Hsla, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active.clone() else {
            return;
        };
        let content = self.input.read(cx).value().to_string();
        let Some(current) = component_css_color_for_range(&content, &active.range) else {
            return;
        };

        let new_text = component_css_format_color(&current.format, color);
        let mut new_content = content.clone();
        new_content.replace_range(current.range.clone(), &new_text);
        let new_rope = Rope::from(new_content.as_str());
        let document_colors = component_document_colors(&new_rope, &new_content);
        let utf16_range = component_range_to_utf16(&content, &current.range);
        self.input.update(cx, |input, cx| {
            EntityInputHandler::replace_text_in_range(
                input,
                Some(utf16_range),
                &new_text,
                window,
                cx,
            );
            input.lsp.set_document_colors(document_colors, cx);
        });

        self.active = Some(ComponentCssColor {
            range: current.range.start..current.range.start + new_text.len(),
            color,
            format: current.format,
        });
        cx.notify();
    }

    fn render_swatches(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !component_color_language(self.buffer.read(cx).language()) {
            return Vec::new();
        }

        let input = self.input.read(cx);
        let content = input.value().to_string();
        let Some(overlay_bounds) = self.overlay_bounds else {
            return Vec::new();
        };
        let origin = overlay_bounds.origin;

        component_css_colors(&content)
            .into_iter()
            .filter_map(|color| {
                let bounds = input.range_to_bounds(&color.range)?;
                let left = (bounds.origin.x - origin.x - COLOR_SWATCH_SIZE - COLOR_SWATCH_GAP)
                    .max(px(0.0));
                let top = (bounds.origin.y - origin.y
                    + (bounds.size.height - COLOR_SWATCH_SIZE).half())
                .max(px(0.0));

                Some(
                    div()
                        .id(("color-swatch", color.range.start))
                        .absolute()
                        .left(left)
                        .top(top)
                        .size(COLOR_SWATCH_SIZE)
                        .rounded(px(1.0))
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |picker, _, _, cx| {
                                picker.open_color(color.clone(), cx);
                                cx.stop_propagation();
                            }),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_spectrum(
        &self,
        color: ComponentColorHsva,
        entity_id: EntityId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hue = component_color_from_hsva(ComponentColorHsva {
            s: 1.0,
            v: 1.0,
            a: 1.0,
            ..color
        });
        let white = gpui::hsla(0.0, 0.0, 1.0, 1.0);
        let transparent_white = gpui::hsla(0.0, 0.0, 1.0, 0.0);
        let black = gpui::hsla(0.0, 0.0, 0.0, 1.0);
        let transparent_black = gpui::hsla(0.0, 0.0, 0.0, 0.0);
        let control = ComponentColorDrag {
            entity_id,
            kind: ComponentColorDragKind::Spectrum,
        };

        div()
            .id("color-spectrum")
            .relative()
            .w_full()
            .h(COLOR_PICKER_SPECTRUM_HEIGHT)
            .rounded(px(5.0))
            .bg(hue)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(5.0))
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(white, 0.0),
                        linear_color_stop(transparent_white, 1.0),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(5.0))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(transparent_black, 0.0),
                        linear_color_stop(black, 1.0),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(color.s))
                    .top(relative(1.0 - color.v))
                    .ml(-COLOR_PICKER_THUMB_SIZE.half())
                    .mt(-COLOR_PICKER_THUMB_SIZE.half())
                    .size(COLOR_PICKER_THUMB_SIZE)
                    .rounded(px(999.0))
                    .border_2()
                    .border_color(white)
                    .shadow_md(),
            )
            .on_prepaint({
                let entity = cx.entity().clone();
                move |bounds, _, cx| {
                    entity.update(cx, |picker, _| picker.spectrum_bounds = Some(bounds));
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|picker, event: &MouseDownEvent, window, cx| {
                    picker.apply_from_position(
                        ComponentColorDragKind::Spectrum,
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_drag(control, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(cx.listener(
                move |picker, event: &DragMoveEvent<ComponentColorDrag>, window, cx| {
                    let drag = event.drag(cx);
                    if drag.entity_id != entity_id || drag.kind != ComponentColorDragKind::Spectrum
                    {
                        return;
                    }
                    picker.apply_from_position(
                        ComponentColorDragKind::Spectrum,
                        event.event.position,
                        window,
                        cx,
                    );
                },
            ))
            .into_any_element()
    }

    fn render_hue_slider(
        &self,
        color: ComponentColorHsva,
        entity_id: EntityId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = ComponentColorDrag {
            entity_id,
            kind: ComponentColorDragKind::Hue,
        };
        div()
            .id("color-hue-slider")
            .relative()
            .w_full()
            .h(COLOR_PICKER_SLIDER_HEIGHT)
            .rounded(px(999.0))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .overflow_hidden()
                    .rounded(px(999.0))
                    .flex()
                    .flex_row()
                    .children((0..48).map(|ix| {
                        div()
                            .flex_1()
                            .h_full()
                            .bg(gpui::hsla(ix as f32 / 47.0, 1.0, 0.5, 1.0))
                    })),
            )
            .child(component_color_slider_thumb(color.h))
            .on_prepaint({
                let entity = cx.entity().clone();
                move |bounds, _, cx| {
                    entity.update(cx, |picker, _| picker.hue_bounds = Some(bounds));
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|picker, event: &MouseDownEvent, window, cx| {
                    picker.apply_from_position(
                        ComponentColorDragKind::Hue,
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_drag(control, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(cx.listener(
                move |picker, event: &DragMoveEvent<ComponentColorDrag>, window, cx| {
                    let drag = event.drag(cx);
                    if drag.entity_id != entity_id || drag.kind != ComponentColorDragKind::Hue {
                        return;
                    }
                    picker.apply_from_position(
                        ComponentColorDragKind::Hue,
                        event.event.position,
                        window,
                        cx,
                    );
                },
            ))
            .into_any_element()
    }

    fn render_alpha_slider(
        &self,
        color: ComponentColorHsva,
        entity_id: EntityId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let start = component_color_from_hsva(ComponentColorHsva { a: 0.0, ..color });
        let end = component_color_from_hsva(ComponentColorHsva { a: 1.0, ..color });
        let control = ComponentColorDrag {
            entity_id,
            kind: ComponentColorDragKind::Alpha,
        };

        div()
            .id("color-alpha-slider")
            .relative()
            .w_full()
            .h(COLOR_PICKER_SLIDER_HEIGHT)
            .rounded(px(999.0))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .overflow_hidden()
                    .rounded(px(999.0))
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(start, 0.0),
                        linear_color_stop(end, 1.0),
                    )),
            )
            .child(component_color_slider_thumb(color.a))
            .on_prepaint({
                let entity = cx.entity().clone();
                move |bounds, _, cx| {
                    entity.update(cx, |picker, _| picker.alpha_bounds = Some(bounds));
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|picker, event: &MouseDownEvent, window, cx| {
                    picker.apply_from_position(
                        ComponentColorDragKind::Alpha,
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_drag(control, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(cx.listener(
                move |picker, event: &DragMoveEvent<ComponentColorDrag>, window, cx| {
                    let drag = event.drag(cx);
                    if drag.entity_id != entity_id || drag.kind != ComponentColorDragKind::Alpha {
                        return;
                    }
                    picker.apply_from_position(
                        ComponentColorDragKind::Alpha,
                        event.event.position,
                        window,
                        cx,
                    );
                },
            ))
            .into_any_element()
    }
}

impl Render for ComponentColorPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let swatches = self.render_swatches(cx);
        if !component_color_language(self.buffer.read(cx).language()) {
            self.active = None;
            self.active_hue = None;
            self.active_saturation = None;
            return component_color_overlay(swatches, None, cx);
        }

        let Some(active) = self.active.clone() else {
            return component_color_overlay(swatches, None, cx);
        };

        let position = {
            let content = self.input.read(cx).value().to_string();
            if component_css_color_for_range(&content, &active.range).is_none() {
                None
            } else {
                self.picker_anchor
                    .or_else(|| self.picker_anchor_for_range(&active.range, cx))
            }
        };

        let Some((left, top)) = position else {
            self.active = None;
            self.active_hue = None;
            self.active_saturation = None;
            self.picker_anchor = None;
            return component_color_overlay(swatches, None, cx);
        };
        self.picker_anchor = Some((left, top));
        let mut color = component_color_to_hsva(active.color);
        if let Some(hue) = self.active_hue {
            color.h = hue;
        }
        if let Some(saturation) = self.active_saturation {
            color.s = saturation;
        }
        let entity_id = cx.entity_id();
        let theme = *cx.theme();

        let picker = div()
            .id("component-color-picker")
            .absolute()
            .left(left)
            .top(top)
            .w(COLOR_PICKER_WIDTH)
            .flex()
            .flex_col()
            .gap(px(10.0))
            .p(px(10.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.bg_surface)
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|picker, _, _, cx| {
                picker.close(cx);
            }))
            .child(self.render_spectrum(color, entity_id, cx))
            .child(self.render_hue_slider(color, entity_id, cx))
            .child(self.render_alpha_slider(color, entity_id, cx));

        component_color_overlay(swatches, Some(picker.into_any_element()), cx)
    }
}

fn component_color_overlay(
    swatches: Vec<AnyElement>,
    picker: Option<AnyElement>,
    cx: &mut Context<ComponentColorPicker>,
) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .on_prepaint({
            let entity = cx.entity().clone();
            move |bounds, _, cx| {
                entity.update(cx, |picker, cx| {
                    if picker.overlay_bounds.is_none_or(|previous| {
                        previous.origin != bounds.origin || previous.size != bounds.size
                    }) {
                        picker.overlay_bounds = Some(bounds);
                        cx.notify();
                    }
                });
            }
        })
        .children(swatches)
        .when_some(picker, |this, picker| this.child(picker))
        .into_any_element()
}

fn component_color_slider_thumb(value: f32) -> AnyElement {
    div()
        .absolute()
        .left(relative(value.clamp(0.0, 1.0)))
        .top(px(-3.0))
        .ml(-COLOR_PICKER_SLIDER_THUMB_WIDTH.half())
        .w(COLOR_PICKER_SLIDER_THUMB_WIDTH)
        .h(COLOR_PICKER_SLIDER_THUMB_HEIGHT)
        .rounded(px(999.0))
        .border_2()
        .border_color(gpui::hsla(0.0, 0.0, 1.0, 1.0))
        .shadow_md()
        .into_any_element()
}

fn component_color_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn component_color_to_hsva(color: Hsla) -> ComponentColorHsva {
    let rgba = color.to_rgb();
    let max = rgba.r.max(rgba.g).max(rgba.b);
    let min = rgba.r.min(rgba.g).min(rgba.b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        color.h
    } else if max == rgba.r {
        ((rgba.g - rgba.b) / delta).rem_euclid(6.0) / 6.0
    } else if max == rgba.g {
        ((rgba.b - rgba.r) / delta + 2.0) / 6.0
    } else {
        ((rgba.r - rgba.g) / delta + 4.0) / 6.0
    };

    ComponentColorHsva {
        h: component_color_unit(hue),
        s: if max == 0.0 { 0.0 } else { delta / max },
        v: max,
        a: color.a,
    }
}

fn component_color_from_hsva(color: ComponentColorHsva) -> Hsla {
    let h = color.h.rem_euclid(1.0) * 6.0;
    let c = color.v * color.s;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let m = color.v - c;
    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    gpui::Rgba {
        r: (r + m).clamp(0.0, 1.0),
        g: (g + m).clamp(0.0, 1.0),
        b: (b + m).clamp(0.0, 1.0),
        a: component_color_unit(color.a),
    }
    .into()
}

impl Render for ComponentCompletionMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open() || !self.input.focus_handle(cx).is_focused(window) {
            return div().into_any_element();
        }

        let input = self.input.read(cx);
        let content = input.value().to_string();
        let cursor = input.cursor();
        if cursor != self.replace_range.end {
            self.clear_without_notify();
            return div().into_any_element();
        }

        let raw_query = completion_raw_query_for_offset(&content, cursor);
        let query = completion_filter_query(&raw_query);
        let visual_query = if query.is_empty() { &raw_query } else { &query };
        let anchor = cursor.saturating_sub(visual_query.len());
        let Some(bounds) = input.range_to_bounds(&(anchor..anchor)) else {
            return div().into_any_element();
        };
        let origin = input
            .range_to_bounds(&(0..0))
            .map(|bounds| bounds.origin)
            .unwrap_or(bounds.origin);
        let theme = *cx.theme();
        let left = (bounds.origin.x - origin.x
            + completion_query_visual_width(visual_query, window))
        .max(px(0.0));
        let line_height = input.line_height().unwrap_or(bounds.size.height);
        let top = completion_line_for_offset(&content, anchor) as f32 * line_height
            + input.scroll_offset().y
            + line_height
            + px(4.0);
        let desired_width = self
            .width
            .unwrap_or_else(|| completion_menu_desired_width(&self.items));
        let width = completion_menu_width(desired_width, window, left);

        let first_visible = self.first_visible;
        let last_visible = self.last_visible();

        div()
            .absolute()
            .left(left)
            .top(top)
            .w(width)
            .max_h(px(280.0))
            .overflow_hidden()
            .occlude()
            .shadow_lg()
            .rounded(rems(0.25))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.bg_surface)
            .p(rems(0.25))
            .text_xs()
            .font_family(FONT_FAMILY)
            .on_scroll_wheel(cx.listener(|menu, event: &gpui::ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(16.0)).y;
                let lines = if delta < Pixels::ZERO {
                    1
                } else if delta > Pixels::ZERO {
                    -1
                } else {
                    0
                };
                if menu.scroll(lines, cx) {
                    cx.stop_propagation();
                }
            }))
            .children(
                self.items[first_visible..last_visible]
                    .iter()
                    .enumerate()
                    .map(|(visible_index, item)| {
                        let index = first_visible + visible_index;
                        let selected = index == self.selected;
                        div()
                            .id(index)
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(rems(0.75))
                            .px(rems(0.4))
                            .py(rems(0.2))
                            .rounded(rems(0.2))
                            .text_color(theme.text)
                            .when(selected, |row| row.bg(theme.bg_selected))
                            .hover(|row| row.bg(theme.bg_hover))
                            .child(div().flex_none().child(item.label.clone()))
                            .when_some(item.detail.clone(), |row, detail| {
                                row.child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_color(theme.text_subtle)
                                        .italic()
                                        .child(detail),
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |menu, _, window, cx| {
                                    if menu.accept_index(index, window, cx) {
                                        cx.stop_propagation();
                                    }
                                }),
                            )
                    }),
            )
            .into_any_element()
    }
}

impl ComponentCompletionItem {
    fn new(item: CompletionItem) -> Self {
        let detail = completion_compact_detail(&item, completion_full_detail(&item).as_deref());
        Self {
            label: item.label.clone(),
            item,
            detail,
        }
    }
}

fn component_document_colors(text: &Rope, content: &str) -> Vec<(lsp_types::Range, Hsla)> {
    component_css_colors(content)
        .into_iter()
        .map(|color| {
            (
                lsp_types::Range::new(
                    text.offset_to_position(color.range.start),
                    text.offset_to_position(color.range.end),
                ),
                color.color,
            )
        })
        .collect()
}

fn component_document_color_information(text: &Rope, content: &str) -> Vec<ColorInformation> {
    component_document_colors(text, content)
        .into_iter()
        .map(|(range, color)| {
            let rgba = color.to_rgb();
            ColorInformation {
                range,
                color: Color {
                    red: rgba.r,
                    green: rgba.g,
                    blue: rgba.b,
                    alpha: rgba.a,
                },
            }
        })
        .collect()
}

impl DocumentColorProvider for ComponentColorProvider {
    fn document_colors(
        &self,
        text: &Rope,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<ColorInformation>>> {
        if !component_color_language(self.buffer.read(cx).language()) {
            return Task::ready(Ok(Vec::new()));
        }

        let content = text.to_string();
        Task::ready(Ok(component_document_color_information(text, &content)))
    }
}

impl HoverProvider for ComponentLspProvider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Option<lsp_types::Hover>>> {
        let content = text.to_string();
        let position = text.offset_to_position(offset);
        let request = {
            let buffer = self.buffer.read(cx);
            let Some(language) = buffer.language() else {
                return Task::ready(Ok(None));
            };
            let language_id = language.as_str().to_string();
            if !lsp::has_installed_server(&language_id) {
                return Task::ready(Ok(None));
            }
            let path = buffer.path().to_path_buf();
            let Some(root) = self
                .root
                .clone()
                .or_else(|| path.parent().map(Path::to_path_buf))
            else {
                return Task::ready(Ok(None));
            };

            lsp::HoverRequest {
                root,
                path,
                language_id,
                content,
                position: lsp::Position {
                    line: position.line,
                    character: position.character,
                },
            }
        };

        cx.background_executor().spawn(async move {
            let hover = lsp::hover(request).map_err(anyhow::Error::from)?;
            Ok(hover.map(|hover| lsp_types::Hover {
                contents: lsp_types::HoverContents::Scalar(lsp_types::MarkedString::String(
                    hover.contents,
                )),
                range: None,
            }))
        })
    }
}

fn component_editor_language(language: &str) -> &str {
    match language {
        "shellscript" => "bash",
        "typescriptreact" => "tsx",
        // gpui-component does not currently ship a separate JSX language.
        "javascriptreact" => "javascript",
        other => other,
    }
}

fn component_diagnostics_from_lsp(
    diagnostics: Vec<LspDiagnostic>,
    content: &str,
) -> Vec<ComponentDiagnostic> {
    let lines = content.split('\n').collect::<Vec<_>>();
    diagnostics
        .into_iter()
        .map(|diagnostic| component_diagnostic_from_lsp(diagnostic, &lines))
        .collect()
}

fn component_diagnostic_from_lsp(diagnostic: LspDiagnostic, lines: &[&str]) -> ComponentDiagnostic {
    let range = component_position_from_lsp(diagnostic.range.start, lines)
        ..component_position_from_lsp(diagnostic.range.end, lines);
    let mut diagnostic = ComponentDiagnostic::from(diagnostic);
    diagnostic.range = range;
    diagnostic
}

fn component_position_from_lsp(position: LspPosition, lines: &[&str]) -> LspPosition {
    let line = lines
        .get(position.line as usize)
        .copied()
        .unwrap_or_default();
    let byte_index = byte_index_for_utf16_column(line, position.character as usize);
    let character = line[..byte_index].chars().count() as u32;
    LspPosition::new(position.line, character)
}

fn byte_index_for_utf16_column(line: &str, utf16_column: usize) -> usize {
    let mut current_column = 0;
    for (byte_index, ch) in line.char_indices() {
        let next_column = current_column + ch.len_utf16();
        if next_column > utf16_column {
            return byte_index;
        }
        if next_column == utf16_column {
            return byte_index + ch.len_utf8();
        }
        current_column = next_column;
    }
    line.len()
}

fn empty_completion_response() -> CompletionResponse {
    CompletionResponse::Array(Vec::new())
}

fn completion_should_request(query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }
    if query.chars().last().is_some_and(completion_query_char) {
        return true;
    }

    query.ends_with('.') || query.ends_with("::") || query.ends_with("->")
}

fn completion_raw_query_for_offset(content: &str, offset: usize) -> String {
    let offset = clamp_to_char_boundary(content, offset);
    let line_start = content[..offset].rfind('\n').map_or(0, |index| index + 1);
    let prefix = &content[line_start..offset];
    let start = prefix
        .char_indices()
        .rev()
        .find_map(|(index, ch)| completion_query_separator(ch).then_some(index + ch.len_utf8()))
        .unwrap_or(0);
    prefix[start..].to_string()
}

fn completion_query_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';')
}

fn enhance_completion_response(response: &mut CompletionResponse) {
    match response {
        CompletionResponse::Array(items) => enhance_completion_items(items),
        CompletionResponse::List(list) => enhance_completion_items(&mut list.items),
    }
}

fn enhance_completion_items(items: &mut [CompletionItem]) {
    for item in items {
        enhance_completion_item(item);
    }
}

fn enhance_completion_item(item: &mut CompletionItem) {
    let full_detail = completion_full_detail(item);
    if item.documentation.is_none()
        && completion_has_extra_detail(item)
        && let Some(detail) = full_detail.clone()
    {
        item.documentation = Some(Documentation::String(detail));
    }
}

fn completion_has_extra_detail(item: &CompletionItem) -> bool {
    item.detail
        .as_deref()
        .is_some_and(|detail| !detail.trim().is_empty())
        || item.label_details.as_ref().is_some_and(|details| {
            details
                .detail
                .as_deref()
                .is_some_and(|detail| !detail.trim().is_empty())
                || details
                    .description
                    .as_deref()
                    .is_some_and(|description| !description.trim().is_empty())
        })
}

fn completion_compact_detail(item: &CompletionItem, full_detail: Option<&str>) -> Option<String> {
    if let Some(detail) = item.detail.as_deref()
        && detail.chars().count() <= MAX_COMPLETION_DETAIL_CHARS
    {
        return Some(detail.to_string());
    }

    let kind = item.kind.and_then(completion_kind_label);
    let label_detail = item
        .label_details
        .as_ref()
        .and_then(|details| details.detail.as_deref())
        .map(str::trim)
        .filter(|detail| !detail.is_empty());

    match (kind, label_detail, full_detail) {
        (Some(kind), Some(label_detail), _) => Some(format!("({kind}) {label_detail}")),
        (Some(kind), None, _) => Some(format!("({kind})")),
        (None, Some(label_detail), _) => Some(label_detail.to_string()),
        (None, None, Some(detail)) => Some(truncate_completion_detail(detail)),
        (None, None, None) => None,
    }
}

fn completion_full_detail(item: &CompletionItem) -> Option<String> {
    let mut parts = Vec::new();
    let detail = item
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if !detail.is_some_and(|detail| detail.starts_with('('))
        && let Some(kind) = item.kind.and_then(completion_kind_label)
    {
        parts.push(format!("({kind})"));
    }
    if let Some(detail) = detail {
        parts.push(detail.to_string());
    }
    if let Some(label_details) = item.label_details.as_ref() {
        if let Some(detail) = label_details
            .detail
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(detail.to_string());
        }
        if let Some(description) = label_details
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(description.to_string());
        }
    }

    (!parts.is_empty()).then(|| parts.join(" "))
}

fn truncate_completion_detail(detail: &str) -> String {
    let mut chars = detail.chars();
    let truncated = chars
        .by_ref()
        .take(MAX_COMPLETION_DETAIL_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn completion_kind_label(kind: CompletionItemKind) -> Option<&'static str> {
    if kind == CompletionItemKind::METHOD {
        Some("method")
    } else if kind == CompletionItemKind::FUNCTION {
        Some("function")
    } else if kind == CompletionItemKind::CONSTRUCTOR {
        Some("constructor")
    } else if kind == CompletionItemKind::FIELD {
        Some("field")
    } else if kind == CompletionItemKind::VARIABLE {
        Some("variable")
    } else if kind == CompletionItemKind::CLASS {
        Some("class")
    } else if kind == CompletionItemKind::INTERFACE {
        Some("interface")
    } else if kind == CompletionItemKind::MODULE {
        Some("module")
    } else if kind == CompletionItemKind::PROPERTY {
        Some("property")
    } else if kind == CompletionItemKind::ENUM {
        Some("enum")
    } else if kind == CompletionItemKind::KEYWORD {
        Some("keyword")
    } else if kind == CompletionItemKind::SNIPPET {
        Some("snippet")
    } else if kind == CompletionItemKind::FILE {
        Some("file")
    } else if kind == CompletionItemKind::FOLDER {
        Some("folder")
    } else if kind == CompletionItemKind::ENUM_MEMBER {
        Some("enum member")
    } else if kind == CompletionItemKind::CONSTANT {
        Some("constant")
    } else if kind == CompletionItemKind::STRUCT {
        Some("struct")
    } else if kind == CompletionItemKind::EVENT {
        Some("event")
    } else if kind == CompletionItemKind::OPERATOR {
        Some("operator")
    } else if kind == CompletionItemKind::TYPE_PARAMETER {
        Some("type parameter")
    } else {
        None
    }
}

fn rank_completion_response(mut response: CompletionResponse, query: &str) -> CompletionResponse {
    if query.is_empty() {
        return response;
    }

    match &mut response {
        CompletionResponse::Array(items) => rank_completion_items(items, query),
        CompletionResponse::List(list) => rank_completion_items(&mut list.items, query),
    }
    response
}

fn rank_completion_items(items: &mut [CompletionItem], query: &str) {
    items.sort_by(|a, b| completion_match_score(a, query).cmp(&completion_match_score(b, query)));
}

fn completion_match_score(item: &CompletionItem, query: &str) -> (u8, usize, usize) {
    let text = item
        .filter_text
        .as_deref()
        .unwrap_or(item.label.as_str())
        .to_lowercase();

    if text == query {
        return (0, 0, text.len());
    }
    if text.starts_with(query) {
        return (1, 0, text.len());
    }
    if let Some(index) = completion_word_boundary_match(&text, query) {
        return (2, index, text.len());
    }
    if let Some(index) = text.find(query) {
        return (3, index, text.len());
    }
    if let Some((index, span)) = completion_fuzzy_match(&text, query) {
        return (4, index, span);
    }

    (5, usize::MAX, text.len())
}

fn completion_filter_query(query: &str) -> String {
    let query = query.trim().to_lowercase();
    let start = query
        .char_indices()
        .rev()
        .find_map(|(index, ch)| (!completion_query_char(ch)).then_some(index + ch.len_utf8()))
        .unwrap_or(0);
    query[start..].to_string()
}

fn completion_word_boundary_match(text: &str, query: &str) -> Option<usize> {
    text.match_indices(query)
        .find(|(index, _)| {
            *index == 0
                || text[..*index]
                    .chars()
                    .last()
                    .is_some_and(|ch| !completion_query_char(ch))
        })
        .map(|(index, _)| index)
}

fn completion_fuzzy_match(text: &str, query: &str) -> Option<(usize, usize)> {
    let mut query_chars = query.chars();
    let mut next = query_chars.next()?;
    let mut first = None;

    for (index, ch) in text.char_indices() {
        if ch != next {
            continue;
        }

        let first_index = *first.get_or_insert(index);
        match query_chars.next() {
            Some(ch) => next = ch,
            None => return Some((first_index, index + ch.len_utf8() - first_index)),
        }
    }

    None
}

fn completion_query_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '$')
}

fn completion_response_items(response: CompletionResponse) -> Vec<CompletionItem> {
    match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    }
}

fn completion_edit_for_item(
    item: &CompletionItem,
    content: &str,
    fallback_range: Range<usize>,
) -> (Range<usize>, String) {
    if let Some(text_edit) = item.text_edit.as_ref() {
        return match text_edit {
            CompletionTextEdit::Edit(edit) => (
                component_lsp_range_to_offset(content, edit.range),
                edit.new_text.clone(),
            ),
            CompletionTextEdit::InsertAndReplace(edit) => (
                component_lsp_range_to_offset(content, edit.replace),
                edit.new_text.clone(),
            ),
        };
    }

    let new_text = item
        .insert_text
        .clone()
        .unwrap_or_else(|| item.label.clone());
    (fallback_range, new_text)
}

fn component_lsp_range_to_offset(content: &str, range: lsp_types::Range) -> Range<usize> {
    component_lsp_position_to_offset(content, range.start)
        ..component_lsp_position_to_offset(content, range.end)
}

fn component_lsp_position_to_offset(content: &str, position: lsp_types::Position) -> usize {
    let mut line = 0u32;
    let mut character = 0u32;

    for (offset, ch) in content.char_indices() {
        if line == position.line && character >= position.character {
            return offset;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    content.len()
}

fn completion_menu_desired_width(items: &[ComponentCompletionItem]) -> Pixels {
    let max_chars = items
        .iter()
        .map(|item| {
            item.label.chars().count()
                + item
                    .detail
                    .as_deref()
                    .map(|detail| detail.chars().count() + 2)
                    .unwrap_or(0)
        })
        .max()
        .unwrap_or(0) as f32;
    (px(max_chars * COMPLETION_MENU_CHAR_WIDTH) + COMPLETION_MENU_HORIZONTAL_PADDING)
        .max(COMPLETION_MENU_MIN_WIDTH)
        .min(COMPLETION_MENU_MAX_WIDTH)
}

fn completion_menu_width(desired: Pixels, window: &Window, left: Pixels) -> Pixels {
    let available = (window.bounds().size.width - left - px(8.0)).max(px(80.0));
    desired.min(available)
}

fn completion_query_visual_width(query: &str, window: &Window) -> Pixels {
    px(query.chars().count() as f32
        * f32::from(window.rem_size())
        * FONT_SIZE_REM
        * COMPLETION_CURSOR_CHAR_WIDTH_FACTOR)
}

fn completion_line_for_offset(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
}

fn clamp_to_char_boundary(content: &str, mut offset: usize) -> usize {
    offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn copy_current_component_line<T: 'static>(input: &Entity<InputState>, cx: &mut Context<T>) {
    let Some((content, range)) = current_component_line_range(input, cx) else {
        return;
    };
    if range.is_empty() {
        return;
    }

    cx.write_to_clipboard(ClipboardItem::new_string(content[range].to_string()));
    cx.stop_propagation();
}

fn cut_current_component_line<T: 'static>(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> bool {
    let Some((content, range)) = current_component_line_range(input, cx) else {
        return false;
    };
    if range.is_empty() {
        return false;
    }

    cx.write_to_clipboard(ClipboardItem::new_string(
        content[range.clone()].to_string(),
    ));
    let utf16_range = component_range_to_utf16(&content, &range);
    input.update(cx, |input, cx| {
        EntityInputHandler::replace_text_in_range(input, Some(utf16_range), "", window, cx);
    });
    true
}

fn insert_component_auto_pair(
    input: &Entity<InputState>,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if event.keystroke.modifiers.control
        || event.keystroke.modifiers.alt
        || event.keystroke.modifiers.platform
    {
        return false;
    }

    let Some((open, close)) = component_auto_pair(event.keystroke.key.as_str()) else {
        return false;
    };

    input.update(cx, |input, cx| {
        let content = input.value().to_string();
        let selected_range = input.selected_range();
        if !selected_range.is_empty() {
            return false;
        }

        let utf16_range = component_range_to_utf16(&content, &selected_range);
        let replacement = format!("{open}{close}");
        EntityInputHandler::replace_text_in_range(
            input,
            Some(utf16_range),
            &replacement,
            window,
            cx,
        );

        let position = input
            .text()
            .offset_to_position(selected_range.start + open.len_utf8());
        input.set_cursor_position(position, window, cx);
        true
    })
}

fn component_auto_pair(key: &str) -> Option<(char, char)> {
    match key {
        "(" => Some(('(', ')')),
        "[" => Some(('[', ']')),
        "{" => Some(('{', '}')),
        "\"" => Some(('\"', '\"')),
        "'" => Some(('\'', '\'')),
        "`" => Some(('`', '`')),
        _ => None,
    }
}

fn current_component_line_range<T: 'static>(
    input: &Entity<InputState>,
    cx: &Context<T>,
) -> Option<(String, Range<usize>)> {
    let input = input.read(cx);
    let selection = input.selected_range();
    if !selection.is_empty() {
        return None;
    }

    let content = input.value().to_string();
    let range = component_line_range_for_offset(&content, selection.start);
    Some((content, range))
}

fn component_line_range_for_offset(content: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(content.len());
    let start = content[..offset].rfind('\n').map_or(0, |index| index + 1);
    let end = content[offset..]
        .find('\n')
        .map_or(content.len(), |index| offset + index + 1);
    start..end
}

fn component_range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    component_offset_to_utf16(content, range.start)..component_offset_to_utf16(content, range.end)
}

fn component_offset_to_utf16(content: &str, offset: usize) -> usize {
    let mut utf16_offset = 0usize;
    let mut utf8_offset = 0usize;
    for ch in content.chars() {
        if utf8_offset >= offset {
            break;
        }
        utf8_offset += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

pub fn install_default_keybindings(cx: &mut App) {
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new(
            "alt-left",
            gpui_component::input::MoveToPreviousWord,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "alt-right",
            gpui_component::input::MoveToNextWord,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "alt-shift-left",
            gpui_component::input::SelectToPreviousWordStart,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "alt-shift-right",
            gpui_component::input::SelectToNextWordEnd,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
    ]);
}

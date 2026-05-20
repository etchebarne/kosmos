use std::cell::Cell as StdCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use file_tree::ActiveFileTree;
use gpui::{
    AnyElement, App, Bounds, Context, DragMoveEvent, Element, ElementId, Global, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, Pixels, Point, Render,
    ScrollHandle, ScrollWheelEvent, SharedString, Window, anchored, deferred, div, prelude::*, px,
    rems,
};
use icons::{Icon, IconName};
use infinity::{
    CanvasPanel, CanvasPoint, InfinityCanvasKey, InfinityCanvasStore, Viewport,
    virtual_panel_tab_id,
};
use tabs::{Tab, TabKind, registry};
use theme::ActiveTheme;

use crate::components::tooltip::with_tooltip_namespace;
use crate::delegate::{PaneDelegate, SettingsDelegate};

const PANEL_HEADER_HEIGHT_REM: f32 = 2.25;
const PANEL_RESIZE_HANDLE_REM: f32 = 1.0;
const MENU_MIN_WIDTH_REM: f32 = 12.0;
const SCROLL_ZOOM_RATE: f32 = 0.12;

type CanvasBounds = Rc<StdCell<Option<Bounds<Pixels>>>>;

#[derive(Default)]
pub struct InfinityUi {
    store: InfinityCanvasStore,
    context_menu: Option<InfinityContextMenu>,
    file_tree_scrolls: HashMap<(InfinityCanvasKey, usize), ScrollHandle>,
}

impl InfinityUi {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    pub fn drop_canvas(workspace_id: usize, tab_id: usize, cx: &mut App) {
        let key = InfinityCanvasKey::new(workspace_id, tab_id);
        if cx.try_global::<Self>().is_some() {
            cx.update_global::<Self, _>(|ui, _| {
                ui.store.remove_canvas(key);
            });
        }
    }

    fn snapshot(&self, key: InfinityCanvasKey) -> infinity::InfinityCanvas {
        self.store.snapshot(key)
    }

    fn file_tree_scroll(&mut self, key: InfinityCanvasKey, panel_id: usize) -> ScrollHandle {
        self.file_tree_scrolls
            .entry((key, panel_id))
            .or_default()
            .clone()
    }
}

impl Global for InfinityUi {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct InfinityContextMenu {
    key: InfinityCanvasKey,
    position: Point<Pixels>,
    canvas_position: CanvasPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InfinityDragKind {
    Pan,
    MovePanel(usize),
    ResizePanel(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InfinityDrag {
    key: InfinityCanvasKey,
    kind: InfinityDragKind,
}

impl InfinityDrag {
    const fn pan(key: InfinityCanvasKey) -> Self {
        Self {
            key,
            kind: InfinityDragKind::Pan,
        }
    }

    const fn move_panel(key: InfinityCanvasKey, panel_id: usize) -> Self {
        Self {
            key,
            kind: InfinityDragKind::MovePanel(panel_id),
        }
    }

    const fn resize_panel(key: InfinityCanvasKey, panel_id: usize) -> Self {
        Self {
            key,
            kind: InfinityDragKind::ResizePanel(panel_id),
        }
    }
}

impl Render for InfinityDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub fn render<T: PaneDelegate + SettingsDelegate>(
    workspace_id: usize,
    workspace_path: &Path,
    tab_id: usize,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    ensure_state(cx);

    let key = InfinityCanvasKey::new(workspace_id, tab_id);
    let snapshot = cx.global::<InfinityUi>().snapshot(key);
    let viewport = snapshot.viewport();
    let panels = snapshot.ordered_panels();
    let is_empty = panels.is_empty();
    let context_menu = cx
        .global::<InfinityUi>()
        .context_menu
        .filter(|menu| menu.key == key);
    let canvas_bounds: CanvasBounds = Rc::new(StdCell::new(None));

    let mut panel_elements = Vec::with_capacity(panels.len());
    for panel in panels {
        panel_elements.push(render_panel(
            key,
            tab_id,
            workspace_id,
            workspace_path,
            viewport,
            panel,
            canvas_bounds.clone(),
            window,
            cx,
        ));
    }

    let theme = *cx.theme();
    let bounds_for_left_pan = canvas_bounds.clone();
    let bounds_for_middle_pan = canvas_bounds.clone();
    let bounds_for_mouse_move = canvas_bounds.clone();
    let bounds_for_context_menu = canvas_bounds.clone();
    let bounds_for_scroll = canvas_bounds.clone();
    let bounds_for_prepaint = canvas_bounds.clone();
    let dismiss_layer = context_menu.map(|_| render_context_menu_dismiss_layer(cx));
    let menu_overlay = context_menu.map(|menu| render_context_menu(menu, cx));

    div()
        .on_children_prepainted(move |bounds, _, _| {
            if let Some(bounds) = bounds.first().copied() {
                bounds_for_prepaint.set(Some(bounds));
            }
        })
        .id(("infinity-canvas", tab_id))
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .overflow_hidden()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                let Some(pointer) =
                    pointer_from_stored_bounds(&bounds_for_left_pan, event.position, window)
                else {
                    return;
                };
                close_all_context_menus(cx);
                mutate_canvas(key, cx, |canvas| {
                    canvas.begin_pan(pointer);
                    true
                });
            }),
        )
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                let Some(pointer) =
                    pointer_from_stored_bounds(&bounds_for_middle_pan, event.position, window)
                else {
                    return;
                };
                close_all_context_menus(cx);
                mutate_canvas(key, cx, |canvas| {
                    canvas.begin_pan(pointer);
                    true
                });
                cx.stop_propagation();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                let Some(pointer) =
                    pointer_from_stored_bounds(&bounds_for_context_menu, event.position, window)
                else {
                    return;
                };
                close_embedded_context_menus(cx);
                open_context_menu(key, event.position, pointer, cx);
                cx.stop_propagation();
            }),
        )
        .on_drag(InfinityDrag::pan(key), |drag, _, _, cx| cx.new(|_| *drag))
        .on_mouse_move(move |event, window, cx| {
            let Some(pointer) =
                pointer_from_stored_bounds(&bounds_for_mouse_move, event.position, window)
            else {
                return;
            };
            if update_canvas_drag(key, pointer, cx) {
                close_all_context_menus_app(cx);
                window.refresh();
            }
        })
        .on_drag_move(
            cx.listener(move |_, event: &DragMoveEvent<InfinityDrag>, window, cx| {
                let drag = *event.drag(cx);
                if drag.key != key {
                    return;
                }
                let pointer = pointer_from_bounds(event.event.position, event.bounds, window);
                if mutate_canvas(key, cx, |canvas| canvas.drag_to(pointer)) {
                    close_all_context_menus(cx);
                    cx.notify();
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                if mutate_canvas(key, cx, |canvas| canvas.finish_interaction()) {
                    cx.notify();
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(move |_, _, _, cx| {
                if mutate_canvas(key, cx, |canvas| canvas.finish_interaction()) {
                    cx.notify();
                }
            }),
        )
        .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            if finish_canvas_interaction(key, cx) {
                window.refresh();
            }
        })
        .on_mouse_up_out(MouseButton::Middle, move |_, window, cx| {
            if finish_canvas_interaction(key, cx) {
                window.refresh();
            }
        })
        .on_scroll_wheel(move |event, window, cx| {
            handle_scroll(key, &bounds_for_scroll, event, window, cx);
        })
        .child(render_canvas_plane(tab_id, cx))
        .when(is_empty, |this| this.child(render_empty_hint(cx)))
        .children(panel_elements)
        .child(render_zoom_badge(viewport, cx))
        .when_some(dismiss_layer, |this, layer| this.child(layer))
        .when_some(menu_overlay, |this, menu| this.child(menu))
        .into_any_element()
}

fn render_canvas_plane<T: PaneDelegate + SettingsDelegate>(
    tab_id: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(("infinity-plane", tab_id))
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .bg(theme.bg_surface)
        .into_any_element()
}

fn render_empty_hint<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.text_muted)
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .rounded(rems(0.75))
                .border_1()
                .border_color(theme.border_subtle)
                .bg(gpui::Hsla::from(theme.bg_elevated).opacity(0.72))
                .p_4()
                .child(
                    div()
                        .text_lg()
                        .text_color(theme.text)
                        .child("Infinity canvas"),
                )
                .child(
                    div()
                        .text_sm()
                        .child("Right-click empty space to add a panel."),
                ),
        )
        .into_any_element()
}

fn render_context_menu<T: PaneDelegate + SettingsDelegate>(
    menu: InfinityContextMenu,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let items = registry::ALL
        .iter()
        .copied()
        .filter(|kind| !kind.is_hidden && kind.id != registry::INFINITY.id)
        .map(|kind| render_context_menu_item(menu.key, menu.canvas_position, kind, cx))
        .collect::<Vec<_>>();

    deferred(
        anchored().position(menu.position).snap_to_window().child(
            div()
                .id("infinity-context-menu")
                .min_w(rems(MENU_MIN_WIDTH_REM))
                .p_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .rounded(rems(0.375))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.bg_elevated)
                .shadow_lg()
                .text_sm()
                .text_color(theme.text)
                .block_mouse_except_scroll()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .children(items),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

fn render_context_menu_item<T: PaneDelegate + SettingsDelegate>(
    key: InfinityCanvasKey,
    canvas_position: CanvasPoint,
    kind: &'static TabKind,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let kind_id = kind.id;
    div()
        .id(SharedString::from(format!("infinity-menu-{kind_id}")))
        .flex()
        .items_center()
        .gap_2()
        .h(rems(1.75))
        .px_2()
        .rounded(rems(0.25))
        .text_color(theme.text)
        .hover(move |this| this.bg(theme.bg_selected).text_color(theme.text_emphasis))
        .on_click(cx.listener(move |_, _, _, cx| {
            add_panel_at(key, kind_id, canvas_position, cx);
        }))
        .child(
            div()
                .w(rems(1.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(super::icon_for_kind(kind.id))
                        .size(14.0)
                        .color(theme.text_muted),
                ),
        )
        .child(kind.name)
        .into_any_element()
}

fn render_context_menu_dismiss_layer<T: PaneDelegate + SettingsDelegate>(
    cx: &mut Context<T>,
) -> AnyElement {
    div()
        .id("infinity-context-menu-dismiss")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _, _, cx| {
                close_context_menu(cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|_, _, _, cx| {
                close_context_menu(cx);
            }),
        )
        .into_any_element()
}

fn render_zoom_badge<T: PaneDelegate + SettingsDelegate>(
    viewport: Viewport,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .absolute()
        .right(rems(1.0))
        .bottom(rems(1.0))
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(gpui::Hsla::from(theme.bg_elevated).opacity(0.9))
        .px_2()
        .py_1()
        .text_xs()
        .text_color(theme.text_muted)
        .child(format!("{}%", (viewport.zoom * 100.0).round() as i32))
        .into_any_element()
}

fn render_panel<T: PaneDelegate + SettingsDelegate>(
    key: InfinityCanvasKey,
    owner_tab_id: usize,
    workspace_id: usize,
    workspace_path: &Path,
    viewport: Viewport,
    panel: CanvasPanel,
    canvas_bounds: CanvasBounds,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let panel_id = panel.id;
    let screen_position = viewport.canvas_to_screen(panel.position);
    let panel_width = panel.size.width * viewport.zoom;
    let panel_height = panel.size.height * viewport.zoom;
    let zoom = viewport.zoom;
    let header_height = PANEL_HEADER_HEIGHT_REM * zoom;
    let title = panel_title(&panel);
    let icon_name = super::icon_for_kind(&panel.kind);
    let bounds_for_move = canvas_bounds.clone();
    let bounds_for_resize = canvas_bounds.clone();
    let tab = embedded_tab(owner_tab_id, workspace_path, &panel);
    let file_tree_scroll = (panel.kind == registry::FILE_TREE.id)
        .then(|| cx.update_global::<InfinityUi, _>(|ui, _| ui.file_tree_scroll(key, panel_id)));
    let panel_body = render_scaled_panel_body(
        workspace_id,
        workspace_path,
        panel_id,
        &tab,
        file_tree_scroll,
        viewport.zoom,
        window,
        cx,
    );

    div()
        .id(("infinity-panel", panel_id))
        .absolute()
        .left(rems(screen_position.x))
        .top(rems(screen_position.y))
        .w(rems(panel_width))
        .h(rems(panel_height))
        .min_w(rems(infinity::MIN_PANEL_WIDTH_REM * viewport.zoom))
        .min_h(rems(infinity::MIN_PANEL_HEIGHT_REM * viewport.zoom))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(rems((0.625 * viewport.zoom).max(0.25)))
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.bg_surface)
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .id(("infinity-panel-header", panel_id))
                .h(rems(header_height))
                .min_h(rems(0.5))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .gap(rems(0.5 * zoom))
                .bg(theme.bg_elevated)
                .border_b_1()
                .border_color(theme.border_subtle)
                .px(rems(0.5 * zoom))
                .text_color(theme.text)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let Some(pointer) =
                            pointer_from_stored_bounds(&bounds_for_move, event.position, window)
                        else {
                            return;
                        };
                        if mutate_canvas(key, cx, |canvas| {
                            canvas.begin_move_panel(panel_id, pointer)
                        }) {
                            cx.notify();
                        }
                    }),
                )
                .on_drag(InfinityDrag::move_panel(key, panel_id), |drag, _, _, cx| {
                    cx.new(|_| *drag)
                })
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(rems(0.5 * zoom))
                        .child(
                            Icon::new(icon_name)
                                .size(14.0 * zoom)
                                .color(theme.text_muted),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(rems(0.875 * zoom))
                                .child(title.clone()),
                        ),
                )
                .child(
                    div()
                        .id(("infinity-panel-close", panel_id))
                        .size(rems(1.5 * zoom))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(rems(0.375 * zoom))
                        .hover(move |this| this.bg(theme.bg_close_hover))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if mutate_canvas(key, cx, |canvas| canvas.remove_panel(panel_id)) {
                                cx.update_global::<InfinityUi, _>(|ui, _| {
                                    ui.file_tree_scrolls.remove(&(key, panel_id));
                                });
                                cx.notify();
                            }
                        }))
                        .child(
                            Icon::new(IconName::Close)
                                .size(13.0 * zoom)
                                .color(theme.text_muted),
                        ),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(panel_body),
        )
        .child(
            div()
                .id(("infinity-panel-resize", panel_id))
                .absolute()
                .right_0()
                .bottom_0()
                .size(rems((PANEL_RESIZE_HANDLE_REM * viewport.zoom).max(0.75)))
                .cursor_nwse_resize()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let Some(pointer) =
                            pointer_from_stored_bounds(&bounds_for_resize, event.position, window)
                        else {
                            return;
                        };
                        if mutate_canvas(key, cx, |canvas| {
                            canvas.begin_resize_panel(panel_id, pointer)
                        }) {
                            cx.notify();
                        }
                    }),
                )
                .on_drag(
                    InfinityDrag::resize_panel(key, panel_id),
                    |drag, _, _, cx| cx.new(|_| *drag),
                )
                .child(
                    div()
                        .absolute()
                        .right(rems(0.25 * zoom))
                        .bottom(rems(0.25 * zoom))
                        .size(rems(0.375 * zoom))
                        .rounded_full()
                        .bg(theme.text_muted),
                ),
        )
        .into_any_element()
}

fn render_scaled_panel_body<T: PaneDelegate + SettingsDelegate>(
    workspace_id: usize,
    workspace_path: &Path,
    panel_id: usize,
    tab: &Tab,
    file_tree_scroll: Option<ScrollHandle>,
    scale: f32,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let base_rem = window.rem_size();
    window.set_rem_size(scaled_rem(base_rem, scale));
    let namespace = format!("infinity:{workspace_id}:{panel_id}:{}", tab.id);
    let child = with_tooltip_namespace(namespace, || {
        if tab.kind == registry::FILE_TREE.id
            && let Some(scroll_handle) = file_tree_scroll
        {
            super::file_tree::render_with_scroll(scroll_handle, cx)
        } else {
            super::render(workspace_id, workspace_path, panel_id, tab, window, cx)
        }
    });
    window.set_rem_size(base_rem);
    ScaledRemElement { child, scale }.into_any_element()
}

struct ScaledRemElement {
    child: AnyElement,
    scale: f32,
}

impl IntoElement for ScaledRemElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ScaledRemElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let base_rem = window.rem_size();
        window.set_rem_size(scaled_rem(base_rem, self.scale));
        let layout_id = self.child.request_layout(window, cx);
        window.set_rem_size(base_rem);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let base_rem = window.rem_size();
        window.set_rem_size(scaled_rem(base_rem, self.scale));
        self.child.prepaint(window, cx);
        window.set_rem_size(base_rem);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let base_rem = window.rem_size();
        window.set_rem_size(scaled_rem(base_rem, self.scale));
        self.child.paint(window, cx);
        window.set_rem_size(base_rem);
    }
}

fn scaled_rem(base_rem: Pixels, scale: f32) -> Pixels {
    px(f32::from(base_rem) * scale)
}

fn embedded_tab(owner_tab_id: usize, workspace_path: &Path, panel: &CanvasPanel) -> Tab {
    let mut tab = Tab {
        id: virtual_panel_tab_id(owner_tab_id, panel.id),
        kind: panel.kind.clone(),
        title: None,
        path: None,
    };
    if tab.kind == registry::TERMINAL.id {
        tab.path = Some(workspace_path.to_path_buf());
    }
    tab
}

fn panel_title(panel: &CanvasPanel) -> SharedString {
    if let Some(kind) = registry::get(&panel.kind) {
        SharedString::new_static(kind.name)
    } else {
        SharedString::from(panel.kind.clone())
    }
}

fn ensure_state<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) {
    if cx.try_global::<InfinityUi>().is_none() {
        cx.set_global(InfinityUi::default());
    }
}

fn mutate_canvas<T: PaneDelegate + SettingsDelegate>(
    key: InfinityCanvasKey,
    cx: &mut Context<T>,
    mutate: impl FnOnce(&mut infinity::InfinityCanvas) -> bool,
) -> bool {
    cx.update_global::<InfinityUi, _>(|ui, _| mutate(ui.store.canvas(key)))
}

fn close_all_context_menus<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> bool {
    let closed_infinity_menu = close_context_menu(cx);
    let closed_embedded_menu = close_embedded_context_menus(cx);
    if closed_embedded_menu {
        cx.notify();
    }
    closed_infinity_menu || closed_embedded_menu
}

fn close_all_context_menus_app(cx: &mut App) -> bool {
    close_context_menu_app(cx) || close_embedded_context_menus(cx)
}

fn close_embedded_context_menus(cx: &mut App) -> bool {
    let closed_git_menu = super::git::close_menu(cx);
    let closed_file_tree_menu = cx.file_tree().cloned().is_some_and(|file_tree| {
        file_tree.update(cx, |tree, cx| {
            let had_context_menu = tree.context_menu().is_some();
            tree.close_context_menu(cx);
            had_context_menu
        })
    });
    closed_git_menu || closed_file_tree_menu
}

fn update_canvas_drag(key: InfinityCanvasKey, pointer: CanvasPoint, cx: &mut App) -> bool {
    if cx.try_global::<InfinityUi>().is_none() {
        return false;
    }
    cx.update_global::<InfinityUi, _>(|ui, _| ui.store.canvas(key).drag_to(pointer))
}

fn open_context_menu<T: PaneDelegate + SettingsDelegate>(
    key: InfinityCanvasKey,
    position: Point<Pixels>,
    screen_position: CanvasPoint,
    cx: &mut Context<T>,
) {
    cx.update_global::<InfinityUi, _>(|ui, _| {
        let canvas_position = ui
            .store
            .canvas(key)
            .viewport()
            .screen_to_canvas(screen_position);
        ui.context_menu = Some(InfinityContextMenu {
            key,
            position,
            canvas_position,
        });
    });
    cx.notify();
}

fn close_context_menu<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> bool {
    let closed = cx.update_global::<InfinityUi, _>(|ui, _| ui.context_menu.take().is_some());
    if closed {
        cx.notify();
    }
    closed
}

fn close_context_menu_app(cx: &mut App) -> bool {
    if cx.try_global::<InfinityUi>().is_none() {
        return false;
    }
    cx.update_global::<InfinityUi, _>(|ui, _| ui.context_menu.take().is_some())
}

fn add_panel_at<T: PaneDelegate + SettingsDelegate>(
    key: InfinityCanvasKey,
    kind_id: &'static str,
    canvas_position: CanvasPoint,
    cx: &mut Context<T>,
) {
    cx.update_global::<InfinityUi, _>(|ui, _| {
        let canvas = ui.store.canvas(key);
        canvas.add_panel(kind_id, canvas_position);
        ui.context_menu = None;
    });
    cx.notify();
}

fn finish_canvas_interaction(key: InfinityCanvasKey, cx: &mut App) -> bool {
    if cx.try_global::<InfinityUi>().is_none() {
        return false;
    }
    cx.update_global::<InfinityUi, _>(|ui, _| ui.store.canvas(key).finish_interaction())
}

fn handle_scroll(
    key: InfinityCanvasKey,
    canvas_bounds: &CanvasBounds,
    event: &ScrollWheelEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(pointer) = pointer_from_stored_bounds(canvas_bounds, event.position, window) else {
        return;
    };
    if cx.try_global::<InfinityUi>().is_none() {
        cx.set_global(InfinityUi::default());
    }
    let Some(canvas_snapshot) = cx
        .try_global::<InfinityUi>()
        .map(|ui| ui.store.snapshot(key))
    else {
        return;
    };
    if let Some((panel_id, panel_kind)) = panel_body_at(&canvas_snapshot, pointer) {
        if panel_kind == registry::FILE_TREE.id {
            let delta_y = event
                .delta
                .pixel_delta(window.line_height() * canvas_snapshot.viewport().zoom)
                .y;
            if scroll_file_tree(key, panel_id, delta_y, cx) {
                cx.stop_propagation();
                window.refresh();
            }
        }
        return;
    }

    let rem_size = window.rem_size();
    let delta = event.delta.pixel_delta(rem_size);
    let delta_x_rem = delta.x / rem_size;
    let delta_y_rem = delta.y / rem_size;
    let changed = cx.update_global::<InfinityUi, _>(|ui, _| {
        let canvas = ui.store.canvas(key);
        if event.modifiers.control {
            let factor = (delta_y_rem * SCROLL_ZOOM_RATE).exp().clamp(0.75, 1.35);
            canvas.zoom_at(pointer, factor)
        } else {
            canvas.pan_by(CanvasPoint::new(delta_x_rem, delta_y_rem))
        }
    });
    if changed {
        close_all_context_menus_app(cx);
        cx.stop_propagation();
        window.refresh();
    }
}

fn pointer_from_stored_bounds(
    canvas_bounds: &CanvasBounds,
    position: Point<Pixels>,
    window: &mut Window,
) -> Option<CanvasPoint> {
    canvas_bounds
        .get()
        .map(|bounds| pointer_from_bounds(position, bounds, window))
}

fn panel_body_at(canvas: &infinity::InfinityCanvas, pointer: CanvasPoint) -> Option<(usize, &str)> {
    let viewport = canvas.viewport();
    canvas.panels().iter().find_map(|panel| {
        let screen_position = viewport.canvas_to_screen(panel.position);
        let panel_width = panel.size.width * viewport.zoom;
        let panel_height = panel.size.height * viewport.zoom;
        let header_height = PANEL_HEADER_HEIGHT_REM * viewport.zoom;
        let is_over_body = pointer.x >= screen_position.x
            && pointer.x <= screen_position.x + panel_width
            && pointer.y >= screen_position.y + header_height
            && pointer.y <= screen_position.y + panel_height;
        is_over_body.then_some((panel.id, panel.kind.as_str()))
    })
}

fn scroll_file_tree(
    key: InfinityCanvasKey,
    panel_id: usize,
    delta_y: Pixels,
    cx: &mut App,
) -> bool {
    if delta_y == Pixels::ZERO {
        return false;
    }
    let scroll_handle =
        cx.update_global::<InfinityUi, _>(|ui, _| ui.file_tree_scroll(key, panel_id));
    let offset = scroll_handle.offset();
    let max_offset = scroll_handle.max_offset();
    let next_y = (offset.y + delta_y)
        .max(-max_offset.height)
        .min(Pixels::ZERO);
    if next_y == offset.y {
        return false;
    }
    scroll_handle.set_offset(Point::new(offset.x, next_y));
    true
}

fn pointer_from_bounds(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) -> CanvasPoint {
    let rem_size = window.rem_size();
    CanvasPoint::new(
        (position.x - bounds.left()) / rem_size,
        (position.y - bounds.top()) / rem_size,
    )
}

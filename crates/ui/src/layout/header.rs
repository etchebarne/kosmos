use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Context, IntoElement, MouseButton, MouseDownEvent,
    SharedString, Window, WindowControlArea, anchored, deferred, div, ease_in_out, prelude::*,
    rems, svg,
};

use icons::{Icon, IconName};
use theme::{ActiveTheme, Theme};
use workspace::{Workspace, WorkspaceManager};

use crate::delegate::{HeaderDelegate, HeaderMenu, WorkspaceDelegate, WorkspaceMenuState};
use crate::drag::WorkspaceDrag;

pub fn render<T: HeaderDelegate>(
    active_menu: Option<HeaderMenu>,
    workspace_manager: &WorkspaceManager,
    workspace_menu: Option<WorkspaceMenuState>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("app-header")
        .h(rems(2.5))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .bg(theme.bg_surface)
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text_header)
        .on_mouse_down(MouseButton::Left, |event, window, cx| {
            cx.stop_propagation();
            if event.click_count >= 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(rems(1.75))
                        .px(rems(0.5))
                        .child(
                            svg()
                                .path("brand/kosmos-icon.svg")
                                .h(rems(1.125))
                                .w(rems(1.35))
                                .flex_none()
                                .text_color(theme.text_emphasis),
                        ),
                )
                .child(render_menu_button::<T>(
                    active_menu,
                    HeaderMenu::File,
                    "File",
                    &["New File", "Open...", "Save", "Save As..."],
                    cx,
                ))
                .child(render_menu_button::<T>(
                    active_menu,
                    HeaderMenu::Edit,
                    "Edit",
                    &["Undo", "Redo", "Cut", "Copy", "Paste"],
                    cx,
                ))
                .child(render_menu_button::<T>(
                    active_menu,
                    HeaderMenu::Selection,
                    "Selection",
                    &["Select All", "Expand Selection", "Shrink Selection"],
                    cx,
                )),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(render_workspace_bar(
            workspace_manager,
            workspace_menu,
            window,
            cx,
        ))
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .child(render_window_button(
                    "window-minimize",
                    IconName::ChromeMinimize,
                    theme.bg_hover_strong,
                    WindowControlArea::Min,
                    false,
                    |window| window.minimize_window(),
                    &theme,
                ))
                .child(render_window_button(
                    "window-maximize",
                    IconName::ChromeMaximize,
                    theme.bg_hover_strong,
                    WindowControlArea::Max,
                    false,
                    |window| window.zoom_window(),
                    &theme,
                ))
                .child(render_window_button(
                    "window-close",
                    IconName::ChromeClose,
                    theme.danger,
                    WindowControlArea::Close,
                    true,
                    |window| window.remove_window(),
                    &theme,
                )),
        )
        .into_any_element()
}

fn render_workspace_bar<T: WorkspaceDelegate>(
    manager: &WorkspaceManager,
    workspace_menu: Option<WorkspaceMenuState>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let active = manager.active_id();
    let previous_active = manager.previous_active_id();
    let active_changed = active != previous_active;
    let mut elements: Vec<AnyElement> = Vec::new();
    for workspace in manager.workspaces() {
        let is_active = active == Some(workspace.id);
        let should_animate = active_changed
            && (Some(workspace.id) == active || Some(workspace.id) == previous_active);
        elements.push(render_workspace_button(
            workspace,
            is_active,
            should_animate,
            window,
            cx,
        ));
    }
    elements.push(render_add_button(cx));
    if let Some(state) = workspace_menu {
        elements.push(render_workspace_menu(state, cx));
        elements.push(render_workspace_menu_dismiss(cx));
    }

    div()
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .children(elements)
        .into_any_element()
}

fn render_add_button<T: WorkspaceDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let hover_group = SharedString::from("workspace-add");
    let accent = theme.accent;
    div()
        .id("workspace-add")
        .group(hover_group.clone())
        .relative()
        .size(rems(1.75))
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.3125))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .can_drop(|drag, _, _| drag.downcast_ref::<WorkspaceDrag>().is_some())
        .on_drop(cx.listener(|this, drag: &WorkspaceDrag, _, cx| {
            cx.stop_propagation();
            this.move_workspace_to_end(drag.id, cx);
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.open_workspace_picker(cx);
        }))
        .child(
            div()
                .absolute()
                .left(rems(-0.1875))
                .top(rems(0.25))
                .bottom(rems(0.25))
                .w(rems(0.125))
                .rounded_full()
                .hover(|s| s)
                .group_drag_over::<WorkspaceDrag>(hover_group.clone(), move |s| s.bg(accent)),
        )
        .child(Icon::new(IconName::Add).size(16.0).color(theme.text_muted))
        .into_any_element()
}

fn render_workspace_button<T: WorkspaceDelegate>(
    workspace: &Workspace,
    is_active: bool,
    should_animate: bool,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = workspace.id;
    let initial = SharedString::from(workspace.initial());
    let name = SharedString::from(workspace.name.clone());
    let hover_group = SharedString::from(format!("workspace-{id}"));
    let accent = theme.accent;
    let drag_initial = initial.clone();

    let inactive_w = 1.75_f32;
    let active_w = measure_text_rems(window, name.as_ref()) + 1.25;
    let anim_id = SharedString::from(format!("ws-anim-{id}-{}", is_active as u8));

    let content = div().relative().h_full().overflow_hidden();
    let content = if should_animate {
        content
            .with_animation(
                anim_id,
                Animation::new(Duration::from_millis(180)).with_easing(ease_in_out),
                move |el, delta| {
                    let p = if is_active { delta } else { 1.0 - delta };
                    let width_rem = inactive_w + (active_w - inactive_w) * p;
                    el.w(rems(width_rem))
                        .child(
                            div()
                                .absolute()
                                .top(rems(0.))
                                .left(rems(0.))
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(1.0 - p)
                                .child(initial.clone()),
                        )
                        .child(
                            div()
                                .absolute()
                                .top(rems(0.))
                                .left(rems(0.))
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(p)
                                .child(name.clone()),
                        )
                },
            )
            .into_any_element()
    } else {
        let width_rem = if is_active { active_w } else { inactive_w };
        content
            .w(rems(width_rem))
            .flex()
            .items_center()
            .justify_center()
            .child(if is_active { name } else { initial })
            .into_any_element()
    };

    div()
        .id(("workspace", id))
        .group(hover_group.clone())
        .relative()
        .h(rems(1.75))
        .flex()
        .items_center()
        .rounded(rems(0.3125))
        .text_sm()
        .bg(if is_active {
            theme.bg_selected
        } else {
            theme.bg_surface
        })
        .text_color(if is_active {
            theme.text_emphasis
        } else {
            theme.text_muted
        })
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .can_drop(move |drag, _, _| {
            drag.downcast_ref::<WorkspaceDrag>()
                .is_some_and(|drag| drag.id != id)
        })
        .on_drop(cx.listener(move |this, drag: &WorkspaceDrag, _, cx| {
            cx.stop_propagation();
            this.move_workspace_before(drag.id, id, cx);
        }))
        .on_drag(
            WorkspaceDrag::new(id, drag_initial),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.open_workspace_menu(id, event.position, cx);
            }),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.select_workspace(id, cx);
        }))
        .child(
            div()
                .absolute()
                .left(rems(-0.1875))
                .top(rems(0.25))
                .bottom(rems(0.25))
                .w(rems(0.125))
                .rounded_full()
                .hover(|s| s)
                .group_drag_over::<WorkspaceDrag>(hover_group.clone(), move |s| s.bg(accent)),
        )
        .child(content)
        .into_any_element()
}

fn measure_text_rems(window: &mut Window, text: &str) -> f32 {
    let style = window.text_style();
    let run = style.to_run(text.len());
    let rem_size = window.rem_size();
    let font_size = rem_size * 0.875_f32;
    let layout = window
        .text_system()
        .layout_line(text, font_size, &[run], None);
    f32::from(layout.width) / f32::from(rem_size)
}

fn render_workspace_menu<T: WorkspaceDelegate>(
    state: WorkspaceMenuState,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = state.id;
    let hover_bg = theme.bg_selected;
    let hover_text = theme.text_emphasis;

    let item = div()
        .id("workspace-menu-close")
        .flex()
        .items_center()
        .gap_2()
        .h(rems(1.625))
        .px_2()
        .rounded(rems(0.25))
        .text_color(theme.text)
        .hover(move |this| this.bg(hover_bg).text_color(hover_text))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.close_workspace(id, cx);
            this.close_workspace_menu(cx);
        }))
        .child(
            div()
                .w(rems(1.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::Close)
                        .size(14.0)
                        .color(theme.text_muted),
                ),
        )
        .child("Close");

    deferred(
        anchored().position(state.position).snap_to_window().child(
            div()
                .id("workspace-context-menu")
                .min_w(rems(10.0))
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
                .child(item),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

fn render_workspace_menu_dismiss<T: WorkspaceDelegate>(cx: &mut Context<T>) -> AnyElement {
    deferred(
        div()
            .id("workspace-menu-dismiss")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_workspace_menu(cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_workspace_menu(cx);
                }),
            ),
    )
    .with_priority(1)
    .into_any_element()
}

fn render_menu_button<T: HeaderDelegate>(
    active_menu: Option<HeaderMenu>,
    menu: HeaderMenu,
    label: &'static str,
    items: &'static [&'static str],
    cx: &mut Context<T>,
) -> impl IntoElement + 'static {
    let theme = *cx.theme();
    let is_active = active_menu == Some(menu);
    let dropdown = is_active.then(|| render_menu_dropdown(menu, items, &theme));

    div()
        .id(("menu-button", menu.id()))
        .relative()
        .h(rems(1.75))
        .px_3()
        .flex()
        .items_center()
        .rounded(rems(0.3125))
        .text_sm()
        .bg(if is_active {
            theme.bg_selected
        } else {
            theme.bg_surface
        })
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_header_menu(menu, cx);
        }))
        .child(label)
        .children(dropdown)
}

fn render_menu_dropdown(menu: HeaderMenu, items: &[&'static str], theme: &Theme) -> AnyElement {
    let item_text = theme.text_header;
    let item_hover_bg = theme.bg_selected;
    let item_hover_text = theme.text_emphasis;

    let mut item_elements = Vec::new();
    for (index, item) in items.iter().enumerate() {
        item_elements.push(
            div()
                .id(("menu-item", menu.id() * 100 + index))
                .h(rems(1.75))
                .px_3()
                .flex()
                .items_center()
                .rounded(rems(0.25))
                .text_sm()
                .text_color(item_text)
                .hover(move |this| this.bg(item_hover_bg).text_color(item_hover_text))
                .child(*item),
        );
    }

    deferred(
        div()
            .id(("menu-dropdown", menu.id()))
            .absolute()
            .top(rems(2.0))
            .left(rems(0.0))
            .w(rems(11.5))
            .p_1()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(rems(0.375))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.bg_elevated)
            .shadow_lg()
            .block_mouse_except_scroll()
            .children(item_elements),
    )
    .into_any_element()
}

fn render_window_button(
    id: &'static str,
    icon: IconName,
    hover_background: gpui::Rgba,
    control_area: WindowControlArea,
    round_right: bool,
    action: impl Fn(&mut Window) + 'static,
    theme: &Theme,
) -> impl IntoElement + 'static {
    let text_color = theme.text_muted;
    let hover_text = theme.text_emphasis;
    div()
        .id(id)
        .h_full()
        .w(rems(2.875))
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(text_color)
        .when(round_right, |this| this.rounded_r(rems(0.4375)))
        .window_control_area(control_area)
        .hover(move |this| this.bg(hover_background).text_color(hover_text))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window);
        })
        .child(Icon::new(icon).size(16.0).color(text_color))
}

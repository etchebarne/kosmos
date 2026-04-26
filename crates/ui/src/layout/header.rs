use gpui::{
    AnyElement, Context, IntoElement, MouseButton, Window, WindowControlArea, deferred, div,
    prelude::*, rems,
};

use icons::{Icon, IconName};
use theme::{ActiveTheme, Theme};
use workspace::{Workspace, WorkspaceManager};

use crate::delegate::{HeaderDelegate, HeaderMenu, WorkspaceDelegate};

pub fn render<T: HeaderDelegate>(
    active_menu: Option<HeaderMenu>,
    workspace_manager: &WorkspaceManager,
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
        .child(render_workspace_bar(workspace_manager, cx))
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
    cx: &mut Context<T>,
) -> AnyElement {
    let mut elements: Vec<AnyElement> = Vec::new();
    for workspace in manager.workspaces() {
        let is_active = manager.active_id() == Some(workspace.id);
        elements.push(render_workspace_button(workspace, is_active, cx));
    }
    elements.push(render_add_button(cx));

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
    div()
        .id("workspace-add")
        .size(rems(1.75))
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.3125))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.open_workspace_picker(cx);
        }))
        .child(Icon::new(IconName::Add).size(16.0).color(theme.text_muted))
        .into_any_element()
}

fn render_workspace_button<T: WorkspaceDelegate>(
    workspace: &Workspace,
    is_active: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = workspace.id;
    div()
        .id(("workspace", id))
        .size(rems(1.75))
        .flex()
        .items_center()
        .justify_center()
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
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.select_workspace(id, cx);
        }))
        .child(workspace.initial())
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

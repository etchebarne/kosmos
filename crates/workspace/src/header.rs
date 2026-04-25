use gpui::{
    AnyElement, Context, IntoElement, MouseButton, Window, WindowControlArea, deferred, div,
    prelude::*, px,
};

use icons::{Icon, IconName};
use theme::{ActiveTheme, Theme};

use crate::{WorkspaceDelegate, WorkspaceManager, render_workspace_bar};

pub trait HeaderDelegate: WorkspaceDelegate {
    fn toggle_header_menu(&mut self, menu: HeaderMenu, cx: &mut Context<Self>);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderMenu {
    File,
    Edit,
    Selection,
}

impl HeaderMenu {
    fn id(self) -> usize {
        match self {
            Self::File => 0,
            Self::Edit => 1,
            Self::Selection => 2,
        }
    }
}

pub fn render_header<T: HeaderDelegate>(
    active_menu: Option<HeaderMenu>,
    workspace_manager: &WorkspaceManager,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("app-header")
        .h(px(40.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .bg(theme.bg_surface)
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text_header)
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            cx.stop_propagation();
            window.start_window_move();
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
        .h(px(28.0))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(5.0))
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

fn render_menu_dropdown(
    menu: HeaderMenu,
    items: &[&'static str],
    theme: &Theme,
) -> AnyElement {
    let item_text = theme.text_header;
    let item_hover_bg = theme.bg_selected;
    let item_hover_text = theme.text_emphasis;

    let mut item_elements = Vec::new();
    for (index, item) in items.iter().enumerate() {
        item_elements.push(
            div()
                .id(("menu-item", menu.id() * 100 + index))
                .h(px(28.0))
                .px_3()
                .flex()
                .items_center()
                .rounded(px(4.0))
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
            .top(px(32.0))
            .left(px(0.0))
            .w(px(184.0))
            .p_1()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(px(6.0))
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
        .w(px(46.0))
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(text_color)
        .when(round_right, |this| this.rounded_r(px(7.0)))
        .window_control_area(control_area)
        .hover(move |this| this.bg(hover_background).text_color(hover_text))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window);
        })
        .child(Icon::new(icon).size(16.0).color(text_color))
}

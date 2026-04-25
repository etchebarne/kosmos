use gpui::{
    AnyElement, Context, IntoElement, MouseButton, Window, WindowControlArea, deferred, div,
    prelude::*, px, rgb,
};

use crate::icon::{Icon, IconName};
use crate::workspace::{WorkspaceDelegate, WorkspaceManager, render_workspace_bar};

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
    div()
        .id("app-header")
        .h(px(36.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .bg(rgb(0x0f172a))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgb(0x263244))
        .text_color(rgb(0xdbe4ef))
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
                    rgb(0x334155),
                    WindowControlArea::Min,
                    false,
                    |window| window.minimize_window(),
                ))
                .child(render_window_button(
                    "window-maximize",
                    IconName::ChromeMaximize,
                    rgb(0x334155),
                    WindowControlArea::Max,
                    false,
                    |window| window.zoom_window(),
                ))
                .child(render_window_button(
                    "window-close",
                    IconName::ChromeClose,
                    rgb(0xdc2626),
                    WindowControlArea::Close,
                    true,
                    |window| window.remove_window(),
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
    let is_active = active_menu == Some(menu);
    let dropdown = is_active.then(|| render_menu_dropdown(menu, items));

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
            rgb(0x263244)
        } else {
            rgb(0x0f172a)
        })
        .hover(|this| this.bg(rgb(0x1f2937)).text_color(rgb(0xffffff)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_header_menu(menu, cx);
        }))
        .child(label)
        .children(dropdown)
}

fn render_menu_dropdown(menu: HeaderMenu, items: &[&'static str]) -> AnyElement {
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
                .text_color(rgb(0xdbe4ef))
                .hover(|this| this.bg(rgb(0x263244)).text_color(rgb(0xffffff)))
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
            .border_color(rgb(0x334155))
            .bg(rgb(0x111827))
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
) -> impl IntoElement + 'static {
    div()
        .id(id)
        .h_full()
        .w(px(46.0))
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgb(0xcbd5e1))
        .when(round_right, |this| this.rounded_r(px(7.0)))
        .window_control_area(control_area)
        .hover(move |this| this.bg(hover_background).text_color(rgb(0xffffff)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window);
        })
        .child(Icon::new(icon).size(16.0).color(rgb(0xcbd5e1)))
}

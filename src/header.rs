use gpui::{
    AnyElement, Context, IntoElement, MouseButton, Window, WindowControlArea, div, prelude::*, px,
    rgb,
};

pub trait HeaderDelegate: Sized + 'static {
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
        .border_b_1()
        .border_color(rgb(0x253044))
        .text_color(rgb(0xdbe4ef))
        .cursor_move()
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            cx.stop_propagation();
            window.start_window_move();
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .child(render_menu_button::<T>(
                    active_menu,
                    HeaderMenu::File,
                    "File",
                    cx,
                ))
                .child(render_menu_button::<T>(
                    active_menu,
                    HeaderMenu::Edit,
                    "Edit",
                    cx,
                ))
                .child(render_menu_button::<T>(
                    active_menu,
                    HeaderMenu::Selection,
                    "Selection",
                    cx,
                )),
        )
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
                    "-",
                    rgb(0x1f2937),
                    rgb(0x334155),
                    WindowControlArea::Min,
                    |window| window.minimize_window(),
                ))
                .child(render_window_button(
                    "window-maximize",
                    "[]",
                    rgb(0x1f2937),
                    rgb(0x334155),
                    WindowControlArea::Max,
                    |window| window.zoom_window(),
                ))
                .child(render_window_button(
                    "window-close",
                    "x",
                    rgb(0x1f2937),
                    rgb(0xdc2626),
                    WindowControlArea::Close,
                    |window| window.remove_window(),
                )),
        )
        .into_any_element()
}

pub fn render_active_menu(active_menu: Option<HeaderMenu>) -> Option<AnyElement> {
    match active_menu? {
        HeaderMenu::File => Some(render_menu_dropdown(
            HeaderMenu::File,
            &["New File", "Open...", "Save", "Save As..."],
        )),
        HeaderMenu::Edit => Some(render_menu_dropdown(
            HeaderMenu::Edit,
            &["Undo", "Redo", "Cut", "Copy", "Paste"],
        )),
        HeaderMenu::Selection => Some(render_menu_dropdown(
            HeaderMenu::Selection,
            &["Select All", "Expand Selection", "Shrink Selection"],
        )),
    }
}

fn render_menu_button<T: HeaderDelegate>(
    active_menu: Option<HeaderMenu>,
    menu: HeaderMenu,
    label: &'static str,
    cx: &mut Context<T>,
) -> impl IntoElement + 'static {
    let is_active = active_menu == Some(menu);

    div()
        .id(("menu-button", menu.id()))
        .h(px(28.0))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(5.0))
        .text_sm()
        .cursor_pointer()
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
}

fn render_menu_dropdown(menu: HeaderMenu, items: &[&'static str]) -> AnyElement {
    let left = match menu {
        HeaderMenu::File => px(8.0),
        HeaderMenu::Edit => px(56.0),
        HeaderMenu::Selection => px(104.0),
    };

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
                .cursor_pointer()
                .hover(|this| this.bg(rgb(0x263244)).text_color(rgb(0xffffff)))
                .child(*item),
        );
    }

    div()
        .id(("menu-dropdown", menu.id()))
        .absolute()
        .top(px(40.0))
        .left(left)
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
        .children(item_elements)
        .into_any_element()
}

fn render_window_button(
    id: &'static str,
    label: &'static str,
    background: gpui::Rgba,
    hover_background: gpui::Rgba,
    control_area: WindowControlArea,
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
        .bg(background)
        .cursor_pointer()
        .window_control_area(control_area)
        .hover(move |this| this.bg(hover_background).text_color(rgb(0xffffff)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window);
        })
        .child(label)
}

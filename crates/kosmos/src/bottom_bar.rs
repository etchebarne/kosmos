use gpui::{AnyElement, IntoElement, div, prelude::*, px, rgb};

pub fn render_bottom_bar() -> AnyElement {
    div()
        .id("app-bottom-bar")
        .h(px(28.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .overflow_hidden()
        .bg(rgb(0x0f172a))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgb(0x263244))
        .text_color(rgb(0xdbe4ef))
        .text_sm()
        .child(div().child("Ready"))
        .child(div().child("UTF-8"))
        .into_any_element()
}

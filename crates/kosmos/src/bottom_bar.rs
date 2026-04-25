use gpui::{AnyElement, IntoElement, div, prelude::*, px};
use theme::Theme;

pub fn render_bottom_bar(theme: &Theme) -> AnyElement {
    div()
        .id("app-bottom-bar")
        .h(px(28.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .overflow_hidden()
        .bg(theme.bg_surface)
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text_header)
        .text_sm()
        .child(div().child("Ready"))
        .child(div().child("UTF-8"))
        .into_any_element()
}

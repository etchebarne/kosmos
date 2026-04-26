use gpui::{AnyElement, IntoElement, div, prelude::*, rems};
use theme::Theme;

pub fn render(theme: &Theme) -> AnyElement {
    div()
        .id("app-bottom-bar")
        .h(rems(1.75))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .overflow_hidden()
        .bg(theme.bg_surface)
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text_header)
        .text_sm()
        .child(div().child("Ready"))
        .child(div().child("UTF-8"))
        .into_any_element()
}

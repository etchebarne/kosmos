use gpui::{AnyElement, App, IntoElement, div, prelude::*, px};

use icons::{Icon, IconName};
use theme::ActiveTheme;

pub fn render(icon: IconName, name: &'static str, cx: &mut App) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(theme.text_subtle)
        .child(Icon::new(icon).size(32.0).color(theme.text_muted))
        .child(div().text_xl().child(name))
        .child(
            div()
                .text_sm()
                .text_color(theme.text_muted)
                .child("Coming soon"),
        )
        .child(div().h(px(4.0)))
        .into_any_element()
}

use gpui::{Div, div, prelude::*, rems};

use icons::{Icon, IconName};
use theme::Theme;

pub(crate) fn render(icon_name: IconName, label: &'static str, theme: Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .h(rems(2.25))
        .px_3()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .text_sm()
        .hover(move |this| {
            this.bg(theme.bg_hover)
                .border_color(theme.border_strong)
                .text_color(theme.text_emphasis)
        })
        .child(Icon::new(icon_name).size(16.0).color(theme.text_muted))
        .child(div().child(label))
}

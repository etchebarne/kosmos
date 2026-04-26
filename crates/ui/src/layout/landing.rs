use gpui::{AnyElement, Context, IntoElement, div, prelude::*, rems};

use icons::{Icon, IconName};
use theme::ActiveTheme;

use crate::delegate::WorkspaceDelegate;

pub fn render<T: WorkspaceDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(theme.bg_surface)
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text)
        .child(div().text_2xl().child("Welcome to Kosmos!"))
        .child(
            div()
                .text_color(theme.text_subtle)
                .child("Open your first workspace to get started"),
        )
        .child(
            div()
                .id("landing-open-workspace")
                .mt_2()
                .flex()
                .items_center()
                .gap_2()
                .px(rems(1.0))
                .py(rems(0.5))
                .rounded(rems(0.375))
                .bg(theme.bg_selected)
                .text_color(theme.text)
                .text_sm()
                .hover(move |this| {
                    this.bg(theme.bg_hover_strong)
                        .text_color(theme.text_emphasis)
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_workspace_picker(cx);
                }))
                .child(Icon::new(IconName::Add).size(16.0).color(theme.text))
                .child("Open Workspace"),
        )
        .into_any_element()
}

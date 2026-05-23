use gpui::{AnyElement, Context, IntoElement, div, prelude::*, rems};
use gpui_component::{Icon as ComponentIcon, button::Button};

use icons::IconName;
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
            Button::new("landing-open-workspace")
                .outline()
                .icon(ComponentIcon::empty().path(IconName::Add.path()))
                .label("Open Workspace")
                .mt_2()
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_workspace_picker(cx);
                })),
        )
        .into_any_element()
}

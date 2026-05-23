use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, rems};
use gpui_component::{Icon as ComponentIcon, button::Button};

use tabs::{TabKind, registry};
use theme::ActiveTheme;

use crate::delegate::PaneDelegate;

pub fn render<T: PaneDelegate>(pane_id: usize, tab_id: usize, cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let buttons: Vec<AnyElement> = registry::ALL
        .iter()
        .copied()
        .filter(|kind| !kind.is_hidden)
        .map(|kind| render_button(pane_id, tab_id, kind, cx))
        .collect();

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .text_color(theme.text)
        .child(
            div()
                .text_xl()
                .text_color(theme.text_emphasis)
                .child("Open a tab"),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_center()
                .gap_2()
                .max_w(rems(40.0))
                .children(buttons),
        )
        .into_any_element()
}

fn render_button<T: PaneDelegate>(
    pane_id: usize,
    tab_id: usize,
    kind: &'static TabKind,
    cx: &mut Context<T>,
) -> AnyElement {
    let kind_id = kind.id;
    Button::new(SharedString::new_static(kind_id))
        .outline()
        .icon(ComponentIcon::empty().path(super::icon_for_kind(kind.id).path()))
        .label(kind.name)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.replace_tab_kind(pane_id, tab_id, kind_id, cx);
        }))
        .into_any_element()
}

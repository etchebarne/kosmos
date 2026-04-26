use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};

use icons::{Icon, IconName};
use panes::Pane;
use tabs::Tab;
use theme::ActiveTheme;

use crate::delegate::PaneDelegate;
use crate::drag::TabDrag;

pub fn render<T: PaneDelegate>(
    pane: &Pane,
    tab: &Tab,
    can_close: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let pane_id = pane.id;
    let id = tab.id;
    let is_active = pane.active_tab == id;
    let hover_group = SharedString::from(format!("tab-{pane_id}-{id}"));

    div()
        .id(("tab", id))
        .group(hover_group.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(px(32.0))
        .w(px(154.0))
        .px_2()
        .rounded(px(6.0))
        .when(is_active, |this| this.bg(gpui::white().opacity(0.08)))
        .text_color(if is_active {
            theme.text_emphasis
        } else {
            theme.text_muted
        })
        .text_sm()
        .hover(move |this| this.bg(theme.bg_hover))
        .drag_over::<TabDrag>({
            let drag_over_bg = theme.bg_drag_over;
            move |this, drag, _, _| {
                if drag.id == id {
                    this
                } else {
                    this.bg(drag_over_bg)
                }
            }
        })
        .can_drop(move |drag, _, _| {
            drag.downcast_ref::<TabDrag>()
                .is_some_and(|drag| drag.id != id)
        })
        .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
            cx.stop_propagation();
            this.move_tab_before(drag.clone(), pane_id, id, cx);
        }))
        .on_drag(
            TabDrag::new(id, pane_id, tab.title.clone()),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_tab(pane_id, id, cx);
        }))
        .child(
            Icon::new(IconName::File)
                .size(16.0)
                .color(if is_active {
                    theme.text
                } else {
                    theme.text_subtle
                })
                .into_any_element(),
        )
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(tab.title.clone()),
        )
        .child(
            div()
                .id(("close-tab", id))
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .text_color(theme.text)
                .invisible()
                .when(can_close, |this| {
                    let close_hover_bg = theme.bg_close_hover;
                    this.group_hover(hover_group, |this| this.visible())
                        .hover(move |this| this.bg(close_hover_bg))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_tab(pane_id, id, cx);
                        }))
                })
                .child(Icon::new(IconName::Close).size(14.0).color(theme.text)),
        )
        .into_any_element()
}

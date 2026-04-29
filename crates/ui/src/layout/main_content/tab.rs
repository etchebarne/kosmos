use file_tree::NodeKind;
use gpui::{AnyElement, Context, IntoElement, MouseButton, SharedString, div, prelude::*, rems};

use icons::{Icon, IconName};
use panes::Pane;
use tabs::Tab;
use theme::ActiveTheme;

use crate::delegate::PaneDelegate;
use crate::drag::TabDrag;
use crate::metrics::{TAB_HEIGHT, TAB_RADIUS};
use crate::tabs::file_tree::drag::FileNodeDrag;

pub fn render<T: PaneDelegate>(
    pane: &Pane,
    tab: &Tab,
    can_close: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let pane_id = pane.id();
    let id = tab.id;
    let is_active = pane.active_tab() == id;
    let hover_group = SharedString::from(format!("tab-{pane_id}-{id}"));
    let title = tab.title();
    let icon_name = tab.icon();
    let accent = theme.accent;

    div()
        .id(("tab", id))
        .group(hover_group.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(TAB_HEIGHT)
        .px_2()
        .rounded(TAB_RADIUS)
        .when(is_active, |this| this.bg(theme.bg_selected))
        .text_color(if is_active {
            theme.text_emphasis
        } else {
            theme.text_muted
        })
        .text_sm()
        .hover(move |this| this.bg(theme.bg_hover))
        .can_drop(move |drag, _, _| {
            if drag
                .downcast_ref::<TabDrag>()
                .is_some_and(|drag| drag.id != id)
            {
                return true;
            }
            drag.downcast_ref::<FileNodeDrag>()
                .is_some_and(|d| d.kind == NodeKind::File)
        })
        .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
            cx.stop_propagation();
            this.move_tab_before(drag.clone(), pane_id, id, cx);
        }))
        .on_drop(cx.listener(move |this, drag: &FileNodeDrag, _, cx| {
            if drag.kind != NodeKind::File {
                return;
            }
            let Some(path) = drag.paths.first().cloned() else {
                return;
            };
            cx.stop_propagation();
            this.open_file_before(path, pane_id, id, cx);
        }))
        .on_drag(
            TabDrag::new(id, pane_id, title.clone(), icon_name),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        )
        .when(can_close, |this| {
            this.on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_tab(pane_id, id, cx);
                }),
            )
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_tab(pane_id, id, cx);
        }))
        .child(
            div()
                .absolute()
                .left(rems(0.0))
                .top(rems(0.25))
                .bottom(rems(0.25))
                .w(rems(0.125))
                .rounded_full()
                // No-op hover forces GPUI to insert a hitbox; without it,
                // group_drag_over styles are skipped and the line never paints.
                .hover(|s| s)
                .group_drag_over::<TabDrag>(hover_group.clone(), move |s| s.bg(accent))
                .group_drag_over::<FileNodeDrag>(hover_group.clone(), move |s| s.bg(accent)),
        )
        .child(
            Icon::new(icon_name)
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
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(title),
        )
        .child(
            div()
                .id(("close-tab", id))
                .size(rems(1.25))
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.25))
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

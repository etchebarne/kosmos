use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use icons::{Icon, IconName};
use pane_tree::{DropZone, PaneTree};
use panes::Pane;
use theme::ActiveTheme;

use crate::delegate::PaneDelegate;
use crate::drag::TabDrag;
use crate::metrics::PANE_HEADER_HEIGHT;
use crate::tabs as tab_views;

use super::tab;

pub fn render<T: PaneDelegate>(tree: &PaneTree, pane: &Pane, cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let active_tab_id = pane.active_tab();
    let tabs = pane.tabs();
    let active_tab = tabs.iter().find(|t| t.id == active_tab_id).cloned();
    let can_close = tree.total_tabs() > 1;
    let mut tab_elements: Vec<AnyElement> = Vec::new();

    for (i, t) in tabs.iter().enumerate() {
        if i > 0 {
            let prev_tab = &tabs[i - 1];
            let show_divider = prev_tab.id != active_tab_id && t.id != active_tab_id;
            tab_elements.push(
                div()
                    .w(px(1.0))
                    .h(px(16.0))
                    .flex_none()
                    .when(show_divider, |this| this.bg(theme.border_strong))
                    .into_any_element(),
            );
        }
        tab_elements.push(tab::render(pane, t, can_close, cx));
    }

    let pane_id = pane.id();
    let body = match active_tab {
        Some(tab) => tab_views::render(pane_id, &tab, cx),
        None => div().flex_1().min_h_0().into_any_element(),
    };
    div()
        .id(("pane", pane_id))
        .relative()
        .size_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .child(
            div()
                .h(PANE_HEADER_HEIGHT)
                .w_full()
                .flex()
                .items_center()
                .gap_1()
                .p(px(6.0))
                .bg(theme.bg_elevated)
                .border_b_1()
                .border_color(theme.border_subtle)
                .overflow_hidden()
                .child(
                    div()
                        .id(("tab-scroll", pane_id))
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .overflow_x_scroll()
                        .children(tab_elements)
                        .child(render_add_tab_button(pane_id, cx)),
                ),
        )
        .child(body)
        .child(
            div()
                .absolute()
                .top(PANE_HEADER_HEIGHT)
                .bottom_0()
                .left_0()
                .right_0()
                .child(render_drop_zone(pane_id, DropZone::Center, cx))
                .child(render_drop_zone(pane_id, DropZone::Left, cx))
                .child(render_drop_zone(pane_id, DropZone::Right, cx))
                .child(render_drop_zone(pane_id, DropZone::Top, cx))
                .child(render_drop_zone(pane_id, DropZone::Bottom, cx)),
        )
        .into_any_element()
}

fn render_add_tab_button<T: PaneDelegate>(pane_id: usize, cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id(("add-tab", pane_id))
        .size(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.add_tab(pane_id, tabs::registry::BLANK.id, cx);
        }))
        .child(Icon::new(IconName::Add).color(theme.text_muted))
        .into_any_element()
}

fn drop_zone_group_name(pane_id: usize, drop_zone: DropZone) -> String {
    let suffix = match drop_zone {
        DropZone::Center => "center",
        DropZone::Left => "left",
        DropZone::Right => "right",
        DropZone::Top => "top",
        DropZone::Bottom => "bottom",
    };
    format!("drop-zone-{pane_id}-{suffix}")
}

fn render_drop_zone<T: PaneDelegate>(
    pane_id: usize,
    drop_zone: DropZone,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = match drop_zone {
        DropZone::Center => 0,
        DropZone::Left => 1,
        DropZone::Right => 2,
        DropZone::Top => 3,
        DropZone::Bottom => 4,
    };

    div()
        .id(("drop-zone", pane_id * 10 + id))
        .group(drop_zone_group_name(pane_id, drop_zone))
        .absolute()
        .when(drop_zone == DropZone::Center, |this| {
            this.top(gpui::relative(0.25))
                .bottom(gpui::relative(0.25))
                .left(gpui::relative(0.25))
                .right(gpui::relative(0.25))
        })
        .when(drop_zone == DropZone::Left, |this| {
            this.top(gpui::relative(0.25))
                .bottom(gpui::relative(0.25))
                .left_0()
                .w(gpui::relative(0.25))
        })
        .when(drop_zone == DropZone::Right, |this| {
            this.top(gpui::relative(0.25))
                .bottom(gpui::relative(0.25))
                .right_0()
                .w(gpui::relative(0.25))
        })
        .when(drop_zone == DropZone::Top, |this| {
            this.top_0().left_0().right_0().h(gpui::relative(0.25))
        })
        .when(drop_zone == DropZone::Bottom, |this| {
            this.bottom_0().left_0().right_0().h(gpui::relative(0.25))
        })
        .when(
            !matches!(drop_zone, DropZone::Left | DropZone::Right),
            |this| {
                this.drag_over::<TabDrag>(move |s, _, _, _| {
                    let alpha = if drop_zone == DropZone::Center {
                        0.08
                    } else {
                        0.18
                    };
                    s.bg(gpui::Hsla::from(theme.accent).opacity(alpha))
                })
            },
        )
        .when(
            matches!(drop_zone, DropZone::Left | DropZone::Right),
            |this| {
                let group_name = drop_zone_group_name(pane_id, drop_zone);
                let highlight_bg = gpui::Hsla::from(theme.accent).opacity(0.18);
                this.child(
                    div()
                        .absolute()
                        .top(gpui::relative(-0.5))
                        .bottom(gpui::relative(-0.5))
                        .left_0()
                        .right_0()
                        // No-op hover forces GPUI to insert a hitbox; without it,
                        // group_drag_over styles are skipped and the highlight never paints.
                        .hover(|s| s)
                        .group_drag_over::<TabDrag>(group_name, move |s| s.bg(highlight_bg)),
                )
            },
        )
        .can_drop(|drag, _, _| drag.downcast_ref::<TabDrag>().is_some())
        .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
            cx.stop_propagation();
            match drop_zone {
                DropZone::Center => this.move_tab_to_pane(drag.clone(), pane_id, cx),
                DropZone::Left | DropZone::Right | DropZone::Top | DropZone::Bottom => {
                    this.split_pane(drag.clone(), pane_id, drop_zone, cx)
                }
            }
        }))
        .into_any_element()
}

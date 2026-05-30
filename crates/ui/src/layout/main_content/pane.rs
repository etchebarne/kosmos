use std::path::Path;

use file_tree::NodeKind;
use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, rems};
use gpui_component::{
    Icon as ComponentIcon, Sizable,
    button::{Button, ButtonVariants},
    separator::Separator,
};

use icons::IconName;
use pane_tree::{DropZone, PaneTree};
use panes::Pane;
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, SettingsDelegate, TabScrollHandles};
use crate::drag::TabDrag;
use crate::metrics::PANE_HEADER_HEIGHT;
use crate::tabs as tab_views;
use crate::tabs::file_tree::drag::FileNodeDrag;

use super::tab;

pub fn render<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    tree: &PaneTree,
    pane: &Pane,
    workspace_id: usize,
    workspace_path: &Path,
    tab_scrolls: &TabScrollHandles,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
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
            let divider = if show_divider {
                Separator::vertical()
                    .h(rems(1.0))
                    .color(gpui::Hsla::from(theme.border_strong))
                    .into_any_element()
            } else {
                div()
                    .w(rems(0.0625))
                    .h(rems(1.0))
                    .flex_none()
                    .into_any_element()
            };
            tab_elements.push(divider);
        }
        tab_elements.push(tab::render(workspace_id, pane, t, can_close, window, cx));
    }

    let pane_id = pane.id();
    let scroll_handle = tab_scrolls.handle(pane_id);
    if tab_scrolls.is_end_anchored(pane_id) && !tabs.is_empty() {
        tab_scrolls.scroll_to_index(pane_id, last_tab_scroll_index(tabs.len()));
    }
    let accept_file_drops = active_tab
        .as_ref()
        .map(|t| t.kind.as_str() != tabs::registry::FILE_TREE.id)
        .unwrap_or(true);
    let body = match active_tab {
        Some(tab) => tab_views::render(workspace_id, workspace_path, pane_id, &tab, window, cx),
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
        .capture_any_mouse_down(cx.listener(move |this, _, _, cx| {
            this.focus_pane(pane_id, cx);
        }))
        .child(
            div()
                .h(PANE_HEADER_HEIGHT)
                .flex_none()
                .w_full()
                .flex()
                .items_center()
                .gap_1()
                .p(rems(0.375))
                .bg(theme.bg_elevated)
                .border_b_1()
                .border_color(theme.border_subtle)
                .overflow_hidden()
                .child(
                    div()
                        .id(("tab-scroll", pane_id))
                        .h_full()
                        .flex()
                        .flex_initial()
                        .min_w_0()
                        .items_center()
                        .gap(rems(0.125))
                        .overflow_x_scroll()
                        .track_scroll(&scroll_handle)
                        .children(tab_elements),
                )
                .child(render_add_tab_button(pane_id, cx)),
        )
        .child(body)
        .child(
            div()
                .absolute()
                .top(PANE_HEADER_HEIGHT)
                .bottom_0()
                .left_0()
                .right_0()
                .child(render_drop_zone(
                    pane_id,
                    DropZone::Center,
                    accept_file_drops,
                    cx,
                ))
                .child(render_drop_zone(
                    pane_id,
                    DropZone::Left,
                    accept_file_drops,
                    cx,
                ))
                .child(render_drop_zone(
                    pane_id,
                    DropZone::Right,
                    accept_file_drops,
                    cx,
                ))
                .child(render_drop_zone(
                    pane_id,
                    DropZone::Top,
                    accept_file_drops,
                    cx,
                ))
                .child(render_drop_zone(
                    pane_id,
                    DropZone::Bottom,
                    accept_file_drops,
                    cx,
                )),
        )
        .into_any_element()
}

fn render_add_tab_button<T: PaneDelegate>(pane_id: usize, cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let group_name = format!("add-tab-drop-{pane_id}");
    let accent = theme.accent;

    div()
        .id(("add-tab-drop", pane_id))
        .group(group_name.clone())
        .relative()
        .flex_none()
        .size(rems(2.0))
        .can_drop(|drag, _, _| {
            if drag.downcast_ref::<TabDrag>().is_some() {
                return true;
            }
            drag.downcast_ref::<FileNodeDrag>()
                .is_some_and(|d| d.kind == NodeKind::File)
        })
        .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
            cx.stop_propagation();
            this.move_tab_to_end(drag.clone(), pane_id, cx);
        }))
        .on_drop(cx.listener(move |this, drag: &FileNodeDrag, _, cx| {
            if drag.kind != NodeKind::File {
                return;
            }
            let Some(path) = drag.paths.first().cloned() else {
                return;
            };
            cx.stop_propagation();
            this.open_file_in_pane(path, pane_id, cx);
        }))
        .child(
            div()
                .absolute()
                .left(rems(-0.125))
                .top(rems(0.25))
                .bottom(rems(0.25))
                .w(rems(0.125))
                .rounded_full()
                .hover(|s| s)
                .group_drag_over::<TabDrag>(group_name.clone(), move |s| s.bg(accent))
                .group_drag_over::<FileNodeDrag>(group_name, move |s| s.bg(accent)),
        )
        .child(
            Button::new(("add-tab", pane_id))
                .ghost()
                .tab_stop(false)
                .size(rems(2.0))
                .child(ComponentIcon::empty().path(IconName::Add.path()).small())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.add_tab(pane_id, tabs::registry::BLANK.id, cx);
                })),
        )
        .into_any_element()
}

fn last_tab_scroll_index(tab_count: usize) -> usize {
    // The scrollable strip is: tab, divider, tab, ..., tab.
    // `n` tabs + `n - 1` dividers = `2 * n - 1` children.
    2 * tab_count - 2
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
    accept_file_drops: bool,
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
                let alpha = if drop_zone == DropZone::Center {
                    0.08
                } else {
                    0.18
                };
                let bg = gpui::Hsla::from(theme.accent).opacity(alpha);
                let this = this.drag_over::<TabDrag>(move |s, _: &TabDrag, _, _| s.bg(bg));
                if accept_file_drops {
                    this.drag_over::<FileNodeDrag>(move |s, _: &FileNodeDrag, _, _| s.bg(bg))
                } else {
                    this
                }
            },
        )
        .when(
            matches!(drop_zone, DropZone::Left | DropZone::Right),
            |this| {
                let group_name = drop_zone_group_name(pane_id, drop_zone);
                let highlight_bg = gpui::Hsla::from(theme.accent).opacity(0.18);
                let highlight = div()
                    .absolute()
                    .top(gpui::relative(-0.5))
                    .bottom(gpui::relative(-0.5))
                    .left_0()
                    .right_0()
                    // No-op hover forces GPUI to insert a hitbox; without it,
                    // group_drag_over styles are skipped and the highlight never paints.
                    .hover(|s| s)
                    .group_drag_over::<TabDrag>(group_name.clone(), move |s| s.bg(highlight_bg));
                let highlight = if accept_file_drops {
                    highlight
                        .group_drag_over::<FileNodeDrag>(group_name, move |s| s.bg(highlight_bg))
                } else {
                    highlight
                };
                this.child(highlight)
            },
        )
        .can_drop(move |drag, _, _| {
            if drag.downcast_ref::<TabDrag>().is_some() {
                return true;
            }
            accept_file_drops
                && drag
                    .downcast_ref::<FileNodeDrag>()
                    .is_some_and(|d| d.kind == NodeKind::File)
        })
        .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
            cx.stop_propagation();
            match drop_zone {
                DropZone::Center => this.move_tab_to_pane(drag.clone(), pane_id, cx),
                DropZone::Left | DropZone::Right | DropZone::Top | DropZone::Bottom => {
                    this.split_pane(drag.clone(), pane_id, drop_zone, cx)
                }
            }
        }))
        .on_drop(cx.listener(move |this, drag: &FileNodeDrag, _, cx| {
            if drag.kind != NodeKind::File {
                return;
            }
            let Some(path) = drag.paths.first().cloned() else {
                return;
            };
            cx.stop_propagation();
            match drop_zone {
                DropZone::Center => this.open_file_in_pane(path, pane_id, cx),
                DropZone::Left | DropZone::Right | DropZone::Top | DropZone::Bottom => {
                    this.split_pane_with_file(path, pane_id, drop_zone, cx)
                }
            }
        }))
        .into_any_element()
}

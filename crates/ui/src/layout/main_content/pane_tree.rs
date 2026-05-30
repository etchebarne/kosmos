use gpui::{
    AnyElement, Context, DragMoveEvent, IntoElement, MouseButton, SharedString, Window, div,
    prelude::*, relative, rems,
};
use std::path::Path;

use pane_tree::{PaneNode, PaneTree, SplitAxis};
use theme::{ActiveTheme, Theme};

use crate::delegate::{PaneDelegate, SettingsDelegate, TabScrollHandles};
use crate::drag::SplitResize;

use super::pane;

pub fn render<T: PaneDelegate + SettingsDelegate + gpui::Render>(
    tree: &PaneTree,
    node: &PaneNode,
    workspace_id: usize,
    workspace_path: &Path,
    tab_scrolls: &TabScrollHandles,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    match node {
        PaneNode::Leaf(p) => pane::render(
            tree,
            p,
            workspace_id,
            workspace_path,
            tab_scrolls,
            window,
            cx,
        ),
        PaneNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let split_id = *id;
            let axis = *axis;
            let ratio = *ratio;
            div()
                .id(("split", split_id))
                .size_full()
                .min_w_0()
                .min_h_0()
                .flex()
                .when(axis == SplitAxis::Row, |this| this.flex_row())
                .when(axis == SplitAxis::Column, |this| this.flex_col())
                .bg(theme.bg_root)
                .on_drag_move(cx.listener(
                    move |this, event: &DragMoveEvent<SplitResize>, _, cx| {
                        let drag = *event.drag(cx);
                        if drag.split_id != split_id {
                            return;
                        }
                        let ratio = match drag.axis {
                            SplitAxis::Row => {
                                (event.event.position.x - event.bounds.left())
                                    / event.bounds.size.width
                            }
                            SplitAxis::Column => {
                                (event.event.position.y - event.bounds.top())
                                    / event.bounds.size.height
                            }
                        };
                        this.resize_split(drag.split_id, ratio, cx);
                    },
                ))
                .child(
                    div()
                        .flex_none()
                        .min_w_0()
                        .min_h_0()
                        .when(axis == SplitAxis::Row, |this| {
                            this.w(relative(ratio)).h_full()
                        })
                        .when(axis == SplitAxis::Column, |this| {
                            this.h(relative(ratio)).w_full()
                        })
                        .child(render(
                            tree,
                            first,
                            workspace_id,
                            workspace_path,
                            tab_scrolls,
                            window,
                            cx,
                        )),
                )
                .child(render_resize_handle(split_id, axis, &theme, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .when(axis == SplitAxis::Row, |this| this.h_full())
                        .when(axis == SplitAxis::Column, |this| this.w_full())
                        .child(render(
                            tree,
                            second,
                            workspace_id,
                            workspace_path,
                            tab_scrolls,
                            window,
                            cx,
                        )),
                )
                .into_any_element()
        }
    }
}

fn render_resize_handle<T: PaneDelegate + SettingsDelegate>(
    split_id: usize,
    axis: SplitAxis,
    theme: &Theme,
    cx: &mut Context<T>,
) -> AnyElement {
    let hover_bg = theme.accent;
    let group_name = SharedString::from(format!("resize-{split_id}"));
    div()
        .id(("resize", split_id))
        .group(group_name.clone())
        .relative()
        .flex_none()
        .bg(theme.bg_hover)
        .group_hover(group_name, move |this| this.bg(hover_bg))
        .when(axis == SplitAxis::Row, |this| this.w(rems(0.1875)).h_full())
        .when(axis == SplitAxis::Column, |this| {
            this.h(rems(0.1875)).w_full()
        })
        .child(
            div()
                .id(("resize-hit", split_id))
                .absolute()
                .when(axis == SplitAxis::Row, |this| {
                    this.top_0()
                        .bottom_0()
                        .left(rems(-0.1875))
                        .right(rems(-0.1875))
                        .cursor_col_resize()
                })
                .when(axis == SplitAxis::Column, |this| {
                    this.left_0()
                        .right_0()
                        .top(rems(-0.1875))
                        .bottom(rems(-0.1875))
                        .cursor_row_resize()
                })
                .on_drag(SplitResize::new(split_id, axis), |resize, _, _, cx| {
                    cx.new(|_| *resize)
                })
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.finish_resize_split(cx)),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.finish_resize_split(cx)),
                ),
        )
        .into_any_element()
}

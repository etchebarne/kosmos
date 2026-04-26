use gpui::{
    AnyElement, Context, DragMoveEvent, IntoElement, SharedString, div, prelude::*, px, relative,
};

use pane_tree::{PaneNode, PaneTree, SplitAxis};
use theme::{ActiveTheme, Theme};

use crate::delegate::{PaneDelegate, TabScrollHandles};
use crate::drag::SplitResize;

use super::pane;

pub fn render<T: PaneDelegate>(
    tree: &PaneTree,
    node: &PaneNode,
    tab_scrolls: &TabScrollHandles,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    match node {
        PaneNode::Leaf(p) => pane::render(tree, p, tab_scrolls, cx),
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
                        .child(render(tree, first, tab_scrolls, cx)),
                )
                .child(render_resize_handle(split_id, axis, &theme))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(render(tree, second, tab_scrolls, cx)),
                )
                .into_any_element()
        }
    }
}

fn render_resize_handle(split_id: usize, axis: SplitAxis, theme: &Theme) -> AnyElement {
    let hover_bg = theme.accent;
    let group_name = SharedString::from(format!("resize-{split_id}"));
    div()
        .id(("resize", split_id))
        .group(group_name.clone())
        .relative()
        .flex_none()
        .bg(theme.bg_hover)
        .group_hover(group_name, move |this| this.bg(hover_bg))
        .when(axis == SplitAxis::Row, |this| this.w(px(3.0)).h_full())
        .when(axis == SplitAxis::Column, |this| this.h(px(3.0)).w_full())
        .child(
            div()
                .id(("resize-hit", split_id))
                .absolute()
                .when(axis == SplitAxis::Row, |this| {
                    this.top_0()
                        .bottom_0()
                        .left(px(-3.0))
                        .right(px(-3.0))
                        .cursor_col_resize()
                })
                .when(axis == SplitAxis::Column, |this| {
                    this.left_0()
                        .right_0()
                        .top(px(-3.0))
                        .bottom(px(-3.0))
                        .cursor_row_resize()
                })
                .on_drag(SplitResize::new(split_id, axis), |resize, _, _, cx| {
                    cx.new(|_| *resize)
                }),
        )
        .into_any_element()
}

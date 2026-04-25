use gpui::{
    AnyElement, Context, DragMoveEvent, IntoElement, PathPromptOptions, Render, SharedString,
    Window, div, prelude::*, px, relative, rgb,
};

use icons::{Icon, IconName};
use tabs::{SplitResize, TabDrag};
use workspace::{
    DropZone, HeaderDelegate, HeaderMenu, Pane, PaneNode, SplitAxis, Tab, WorkspaceDelegate,
    WorkspaceManager, render_header, render_landing,
};

use crate::bottom_bar::render_bottom_bar;

pub struct IdeApp {
    active_menu: Option<HeaderMenu>,
    workspaces: WorkspaceManager,
}

impl IdeApp {
    pub fn new() -> Self {
        Self {
            active_menu: None,
            workspaces: persistence::load(),
        }
    }

    pub fn start_observing_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.observe_window_bounds(window, |_, window, _| {
            persistence::save_window_bounds(window.window_bounds());
        })
        .detach();
    }

    fn persist_active_workspace(&self) {
        if let Some(workspace) = self.workspaces.active_workspace() {
            persistence::save_workspace(workspace);
        }
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.active_menu.take().is_some() {
            cx.notify();
        }
    }

    fn mutate_active_tree(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut workspace::PaneTree) -> bool,
    ) {
        let Some(tree) = self.workspaces.active_pane_tree_mut() else {
            return;
        };
        if !f(tree) {
            return;
        }
        cx.notify();
        self.persist_active_workspace();
    }

    fn add_tab(&mut self, pane_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| tree.add_tab(pane_id));
    }

    fn select_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| tree.select_tab(pane_id, tab_id));
    }

    fn close_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| tree.close_tab(pane_id, tab_id));
    }

    fn move_tab_before(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        target_tab_id: usize,
        cx: &mut Context<Self>,
    ) {
        self.mutate_active_tree(cx, |tree| {
            tree.move_tab_before(drag.source_pane_id, drag.id, target_pane_id, target_tab_id)
        });
    }

    fn move_tab_to_pane(&mut self, drag: TabDrag, target_pane_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| {
            tree.move_tab_to_pane(drag.source_pane_id, drag.id, target_pane_id)
        });
    }

    fn split_pane(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    ) {
        self.mutate_active_tree(cx, |tree| {
            tree.split_pane(drag.source_pane_id, drag.id, target_pane_id, drop_zone)
        });
    }

    fn resize_split(&mut self, split_id: usize, ratio: f32, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| tree.resize_split(split_id, ratio));
    }

    fn render_node(&self, node: &PaneNode, cx: &mut Context<Self>) -> AnyElement {
        match node {
            PaneNode::Leaf(pane) => self.render_pane(pane, cx),
            PaneNode::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => div()
                .id(("split", *id))
                .size_full()
                .min_w_0()
                .min_h_0()
                .flex()
                .when(*axis == SplitAxis::Row, |this| this.flex_row())
                .when(*axis == SplitAxis::Column, |this| this.flex_col())
                .bg(rgb(0x0b1120))
                .on_drag_move(
                    cx.listener({
                        let split_id = *id;
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
                        }
                    }),
                )
                .child(
                    div()
                        .flex_none()
                        .min_w_0()
                        .min_h_0()
                        .when(*axis == SplitAxis::Row, |this| {
                            this.w(relative(*ratio)).h_full()
                        })
                        .when(*axis == SplitAxis::Column, |this| {
                            this.h(relative(*ratio)).w_full()
                        })
                        .child(self.render_node(first, cx)),
                )
                .child(self.render_resize_handle(*id, *axis))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(self.render_node(second, cx)),
                )
                .into_any_element(),
        }
    }

    fn render_resize_handle(&self, split_id: usize, axis: SplitAxis) -> AnyElement {
        div()
            .id(("resize", split_id))
            .flex_none()
            .bg(rgb(0x1f2937))
            .hover(|this| this.bg(rgb(0x3b82f6)))
            .when(axis == SplitAxis::Row, |this| {
                this.w(px(6.0)).h_full().cursor_col_resize()
            })
            .when(axis == SplitAxis::Column, |this| {
                this.h(px(6.0)).w_full().cursor_row_resize()
            })
            .on_drag(
                SplitResize::new(split_id, axis),
                |resize, position, _, cx| cx.new(|_| resize.position(position)),
            )
            .into_any_element()
    }

    fn render_pane(&self, pane: &Pane, cx: &mut Context<Self>) -> AnyElement {
        let active_title = pane
            .tabs
            .iter()
            .find(|tab| tab.id == pane.active_tab)
            .map(|tab| tab.title.clone())
            .unwrap_or_else(|| "Blank".into());
        let mut tab_elements = Vec::new();

        for tab in &pane.tabs {
            tab_elements.push(self.render_tab(pane, tab, cx));
        }

        div()
            .id(("pane", pane.id))
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .rounded(px(8.0))
            .bg(rgb(0x0f172a))
            .border_1()
            .border_color(rgb(0x263244))
            .text_color(rgb(0xe5e7eb))
            .child(
                div()
                    .h(px(44.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .p(px(6.0))
                    .bg(rgb(0x111827))
                    .rounded_t(px(7.0))
                    .border_b_1()
                    .border_color(rgb(0x2d3748))
                    .overflow_hidden()
                    .child(
                        div()
                            .id(("tab-scroll", pane.id))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .overflow_x_scroll()
                            .children(tab_elements)
                            .child(
                                div()
                                    .id(("add-tab", pane.id))
                                    .size(px(32.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(6.0))
                                    .text_color(rgb(0xcbd5e1))
                                    .hover(|this| {
                                        this.bg(rgb(0x1f2937)).text_color(rgb(0xffffff))
                                    })
                                    .on_click(cx.listener({
                                        let pane_id = pane.id;
                                        move |this, _, _, cx| {
                                            this.add_tab(pane_id, cx);
                                        }
                                    }))
                                    .child(Icon::new(IconName::Add).color(rgb(0xcbd5e1))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xl()
                    .text_color(rgb(0x94a3b8))
                    .child(active_title),
            )
            .child(self.render_drop_zone(pane.id, DropZone::Center, cx))
            .child(self.render_drop_zone(pane.id, DropZone::Left, cx))
            .child(self.render_drop_zone(pane.id, DropZone::Right, cx))
            .child(self.render_drop_zone(pane.id, DropZone::Top, cx))
            .child(self.render_drop_zone(pane.id, DropZone::Bottom, cx))
            .into_any_element()
    }

    fn render_drop_zone(
        &self,
        pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match drop_zone {
            DropZone::Center => 0,
            DropZone::Left => 1,
            DropZone::Right => 2,
            DropZone::Top => 3,
            DropZone::Bottom => 4,
        };

        div()
            .id(("drop-zone", pane_id * 10 + id))
            .absolute()
            .when(drop_zone == DropZone::Center, |this| {
                this.top(px(108.0))
                    .bottom(px(64.0))
                    .left(px(64.0))
                    .right(px(64.0))
            })
            .when(drop_zone == DropZone::Left, |this| {
                this.top(px(44.0)).bottom_0().left_0().w(px(64.0))
            })
            .when(drop_zone == DropZone::Right, |this| {
                this.top(px(44.0)).bottom_0().right_0().w(px(64.0))
            })
            .when(drop_zone == DropZone::Top, |this| {
                this.top(px(44.0)).left_0().right_0().h(px(64.0))
            })
            .when(drop_zone == DropZone::Bottom, |this| {
                this.bottom_0().left_0().right_0().h(px(64.0))
            })
            .drag_over::<TabDrag>(move |this, _, _, _| {
                this.bg(gpui::blue().opacity(if drop_zone == DropZone::Center {
                    0.08
                } else {
                    0.18
                }))
            })
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

    fn render_tab(
        &self,
        pane: &Pane,
        tab: &Tab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        let pane_id = pane.id;
        let id = tab.id;
        let is_active = pane.active_tab == id;
        let can_close = self
            .workspaces
            .active_pane_tree()
            .map(|tree| tree.total_tabs() > 1)
            .unwrap_or(false);
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
            .px_3()
            .rounded(px(6.0))
            .when(is_active, |this| this.bg(gpui::white().opacity(0.08)))
            .text_color(if is_active {
                rgb(0xffffff)
            } else {
                rgb(0xcbd5e1)
            })
            .text_sm()
            .hover(|this| this.bg(rgb(0x1f2937)))
            .drag_over::<TabDrag>(move |this, drag, _, _| {
                if drag.id == id {
                    this
                } else {
                    this.bg(rgb(0x1e3a5f))
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
            .on_drag(TabDrag::new(id, pane_id, tab.title.clone()), |drag, position, _, cx| {
                cx.new(|_| drag.clone().position(position))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_tab(pane_id, id, cx);
            }))
            .child(
                Icon::new(IconName::File)
                    .size(16.0)
                    .color(if is_active {
                        rgb(0xe5e7eb)
                    } else {
                        rgb(0x94a3b8)
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
                    .text_color(rgb(0xe5e7eb))
                    .invisible()
                    .when(can_close, |this| {
                        this.group_hover(hover_group, |this| this.visible())
                            .hover(|this| this.bg(rgb(0x374151)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_tab(pane_id, id, cx);
                            }))
                    })
                    .child(Icon::new(IconName::Close).size(14.0).color(rgb(0xe5e7eb))),
            )
    }
}

impl HeaderDelegate for IdeApp {
    fn toggle_header_menu(&mut self, menu: HeaderMenu, cx: &mut Context<Self>) {
        self.active_menu = if self.active_menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }
}

impl WorkspaceDelegate for IdeApp {
    fn open_workspace_picker(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Workspace".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.workspaces.add(path);
                cx.notify();
                this.persist_active_workspace();
                persistence::save_session(&this.workspaces);
            });
        })
        .detach();
    }

    fn select_workspace(&mut self, id: usize, cx: &mut Context<Self>) {
        if self.workspaces.select(id) {
            cx.notify();
            persistence::save_session(&self.workspaces);
        }
    }
}

impl Render for IdeApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main_content = match self.workspaces.active_pane_tree() {
            Some(tree) => self.render_node(tree.root(), cx),
            None => render_landing(cx),
        };

        div()
            .id("app-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .bg(rgb(0x0b1120))
            .on_click(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .child(render_header(self.active_menu, &self.workspaces, cx))
            .child(div().flex_1().min_h_0().child(main_content))
            .child(render_bottom_bar())
    }
}

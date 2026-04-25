use gpui::{
    AnyElement, Context, DragMoveEvent, IntoElement, PathPromptOptions, Render, SharedString,
    Window, div, prelude::*, px, relative,
};

use icons::{Icon, IconName};
use tabs::{SplitResize, TabDrag};
use theme::{ActiveTheme, Theme};
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
        let theme = *cx.theme();
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
                .bg(theme.bg_root)
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
                .child(self.render_resize_handle(*id, *axis, &theme))
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

    fn render_resize_handle(&self, split_id: usize, axis: SplitAxis, theme: &Theme) -> AnyElement {
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
                    .on_drag(
                        SplitResize::new(split_id, axis),
                        |resize, _, _, cx| cx.new(|_| *resize),
                    ),
            )
            .into_any_element()
    }

    fn render_pane(&self, pane: &Pane, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let active_title = pane
            .tabs
            .iter()
            .find(|tab| tab.id == pane.active_tab)
            .map(|tab| tab.title.clone())
            .unwrap_or_else(|| "Blank".into());
        let mut tab_elements: Vec<AnyElement> = Vec::new();

        for (i, tab) in pane.tabs.iter().enumerate() {
            if i > 0 {
                let prev_tab = &pane.tabs[i - 1];
                let show_divider =
                    prev_tab.id != pane.active_tab && tab.id != pane.active_tab;
                tab_elements.push(
                    div()
                        .w(px(1.0))
                        .h(px(16.0))
                        .flex_none()
                        .when(show_divider, |this| this.bg(theme.border_strong))
                        .into_any_element(),
                );
            }
            tab_elements.push(self.render_tab(pane, tab, cx).into_any_element());
        }

        div()
            .id(("pane", pane.id))
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
                    .h(px(44.0))
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
                            .id(("tab-scroll", pane.id))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
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
                                    .text_color(theme.text_muted)
                                    .hover(move |this| {
                                        this.bg(theme.bg_hover).text_color(theme.text_emphasis)
                                    })
                                    .on_click(cx.listener({
                                        let pane_id = pane.id;
                                        move |this, _, _, cx| {
                                            this.add_tab(pane_id, cx);
                                        }
                                    }))
                                    .child(Icon::new(IconName::Add).color(theme.text_muted)),
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
                    .text_color(theme.text_subtle)
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
        let theme = *cx.theme();
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
        let theme = *cx.theme();
        let main_content = match self.workspaces.active_pane_tree() {
            Some(tree) => div()
                .size_full()
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .child(self.render_node(tree.root(), cx))
                .into_any_element(),
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
            .bg(theme.bg_root)
            .on_click(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .child(render_header(self.active_menu, &self.workspaces, cx))
            .child(div().flex_1().min_h_0().child(main_content))
            .child(render_bottom_bar(&theme))
    }
}

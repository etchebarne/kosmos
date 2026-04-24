use gpui::{
    AnyElement, App, Application, Bounds, Context, DragMoveEvent, IntoElement, Pixels, Point,
    SharedString, Window, WindowBounds, WindowOptions, div, prelude::*, px, relative, rgb, size,
};

#[derive(Clone)]
struct Tab {
    id: usize,
    title: SharedString,
}

#[derive(Clone)]
struct Pane {
    id: usize,
    tabs: Vec<Tab>,
    active_tab: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitAxis {
    Row,
    Column,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DropZone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone)]
enum PaneNode {
    Leaf(Pane),
    Split {
        id: usize,
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

#[derive(Clone, Copy)]
struct TabDrag {
    id: usize,
    source_pane_id: usize,
    position: Point<Pixels>,
}

impl TabDrag {
    fn new(id: usize, source_pane_id: usize) -> Self {
        Self {
            id,
            source_pane_id,
            position: Point::default(),
        }
    }

    fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for TabDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.position.x - px(70.0))
            .pt(self.position.y - px(18.0))
            .child(
                div()
                    .h(px(36.0))
                    .w(px(154.0))
                    .flex()
                    .items_center()
                    .px_3()
                    .rounded_t(px(7.0))
                    .border_1()
                    .border_color(rgb(0x60a5fa))
                    .bg(rgb(0x111827))
                    .text_sm()
                    .text_color(rgb(0xffffff))
                    .shadow_lg()
                    .child("Blank"),
            )
    }
}

#[derive(Clone, Copy)]
struct SplitResize {
    split_id: usize,
    axis: SplitAxis,
    position: Point<Pixels>,
}

impl SplitResize {
    fn new(split_id: usize, axis: SplitAxis) -> Self {
        Self {
            split_id,
            axis,
            position: Point::default(),
        }
    }

    fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for SplitResize {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.position.x - px(16.0))
            .pt(self.position.y - px(16.0))
            .child(
                div()
                    .size(px(32.0))
                    .rounded(px(6.0))
                    .bg(gpui::blue().opacity(0.18)),
            )
    }
}

struct TabApp {
    root: PaneNode,
    next_tab_id: usize,
    next_pane_id: usize,
    next_split_id: usize,
}

impl TabApp {
    fn new() -> Self {
        Self {
            root: PaneNode::Leaf(Pane {
                id: 0,
                tabs: vec![Tab {
                    id: 0,
                    title: "Blank".into(),
                }],
                active_tab: 0,
            }),
            next_tab_id: 1,
            next_pane_id: 1,
            next_split_id: 1,
        }
    }

    fn add_tab(&mut self, pane_id: usize, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;

        if let Some(pane) = Self::find_pane_mut(&mut self.root, pane_id) {
            pane.tabs.push(Tab {
                id,
                title: "Blank".into(),
            });
            pane.active_tab = id;
            cx.notify();
        }
    }

    fn select_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        if let Some(pane) = Self::find_pane_mut(&mut self.root, pane_id) {
            pane.active_tab = tab_id;
            cx.notify();
        }
    }

    fn close_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        if Self::total_tabs_in(&self.root) == 1 {
            return;
        }

        if Self::take_tab_from_pane(&mut self.root, pane_id, tab_id).is_some() {
            Self::collapse_empty_panes(&mut self.root);
            cx.notify();
        }
    }

    fn move_tab_before(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        target_tab_id: usize,
        cx: &mut Context<Self>,
    ) {
        if drag.source_pane_id == target_pane_id && drag.id == target_tab_id {
            return;
        }

        let Some(tab) = Self::take_tab_from_pane(&mut self.root, drag.source_pane_id, drag.id)
        else {
            return;
        };

        let Some(target_pane) = Self::find_pane_mut(&mut self.root, target_pane_id) else {
            Self::insert_tab_at_end(&mut self.root, drag.source_pane_id, tab);
            return;
        };

        let insertion_index = target_pane
            .tabs
            .iter()
            .position(|tab| tab.id == target_tab_id)
            .unwrap_or(target_pane.tabs.len());

        target_pane.active_tab = tab.id;
        target_pane.tabs.insert(insertion_index, tab);
        Self::collapse_empty_panes(&mut self.root);
        cx.notify();
    }

    fn move_tab_to_pane(&mut self, drag: TabDrag, target_pane_id: usize, cx: &mut Context<Self>) {
        if drag.source_pane_id == target_pane_id {
            return;
        }

        let Some(tab) = Self::take_tab_from_pane(&mut self.root, drag.source_pane_id, drag.id)
        else {
            return;
        };

        if !Self::insert_tab_at_end(&mut self.root, target_pane_id, tab.clone()) {
            Self::insert_tab_at_end(&mut self.root, drag.source_pane_id, tab);
            return;
        }

        Self::collapse_empty_panes(&mut self.root);
        cx.notify();
    }

    fn split_pane(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    ) {
        if drop_zone == DropZone::Center || Self::total_tabs_in(&self.root) == 1 {
            return;
        }

        if drag.source_pane_id == target_pane_id {
            let Some(source_pane) = Self::find_pane(&self.root, drag.source_pane_id) else {
                return;
            };

            if source_pane.tabs.len() == 1 {
                return;
            }
        }

        let Some(tab) = Self::take_tab_from_pane(&mut self.root, drag.source_pane_id, drag.id)
        else {
            return;
        };

        let new_pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let new_split_id = self.next_split_id;
        self.next_split_id += 1;

        if !Self::split_leaf_with_tab(
            &mut self.root,
            target_pane_id,
            tab.clone(),
            new_pane_id,
            new_split_id,
            drop_zone,
        ) {
            Self::insert_tab_at_end(&mut self.root, drag.source_pane_id, tab);
            return;
        }

        Self::collapse_empty_panes(&mut self.root);
        cx.notify();
    }

    fn resize_split(&mut self, split_id: usize, ratio: f32, cx: &mut Context<Self>) {
        if let Some(split_ratio) = Self::find_split_ratio_mut(&mut self.root, split_id) {
            *split_ratio = ratio.clamp(0.15, 0.85);
            cx.notify();
        }
    }

    fn find_pane(node: &PaneNode, pane_id: usize) -> Option<&Pane> {
        match node {
            PaneNode::Leaf(pane) if pane.id == pane_id => Some(pane),
            PaneNode::Leaf(_) => None,
            PaneNode::Split { first, second, .. } => {
                Self::find_pane(first, pane_id).or_else(|| Self::find_pane(second, pane_id))
            }
        }
    }

    fn find_pane_mut(node: &mut PaneNode, pane_id: usize) -> Option<&mut Pane> {
        match node {
            PaneNode::Leaf(pane) if pane.id == pane_id => Some(pane),
            PaneNode::Leaf(_) => None,
            PaneNode::Split { first, second, .. } => {
                Self::find_pane_mut(first, pane_id).or_else(|| Self::find_pane_mut(second, pane_id))
            }
        }
    }

    fn total_tabs_in(node: &PaneNode) -> usize {
        match node {
            PaneNode::Leaf(pane) => pane.tabs.len(),
            PaneNode::Split { first, second, .. } => {
                Self::total_tabs_in(first) + Self::total_tabs_in(second)
            }
        }
    }

    fn find_split_ratio_mut(node: &mut PaneNode, split_id: usize) -> Option<&mut f32> {
        match node {
            PaneNode::Leaf(_) => None,
            PaneNode::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == split_id {
                    Some(ratio)
                } else {
                    Self::find_split_ratio_mut(first, split_id)
                        .or_else(|| Self::find_split_ratio_mut(second, split_id))
                }
            }
        }
    }

    fn take_tab_from_pane(node: &mut PaneNode, pane_id: usize, tab_id: usize) -> Option<Tab> {
        let pane = Self::find_pane_mut(node, pane_id)?;
        let tab_index = pane.tabs.iter().position(|tab| tab.id == tab_id)?;
        let tab = pane.tabs.remove(tab_index);

        if pane.active_tab == tab_id && !pane.tabs.is_empty() {
            let next_active_index = tab_index.saturating_sub(1).min(pane.tabs.len() - 1);
            pane.active_tab = pane.tabs[next_active_index].id;
        }

        Some(tab)
    }

    fn insert_tab_at_end(node: &mut PaneNode, pane_id: usize, tab: Tab) -> bool {
        let Some(pane) = Self::find_pane_mut(node, pane_id) else {
            return false;
        };

        pane.active_tab = tab.id;
        pane.tabs.push(tab);
        true
    }

    fn split_leaf_with_tab(
        node: &mut PaneNode,
        pane_id: usize,
        tab: Tab,
        new_pane_id: usize,
        new_split_id: usize,
        drop_zone: DropZone,
    ) -> bool {
        match node {
            PaneNode::Leaf(pane) if pane.id == pane_id => {
                let axis = match drop_zone {
                    DropZone::Left | DropZone::Right => SplitAxis::Row,
                    DropZone::Top | DropZone::Bottom => SplitAxis::Column,
                    DropZone::Center => return false,
                };
                let new_pane = PaneNode::Leaf(Pane {
                    id: new_pane_id,
                    active_tab: tab.id,
                    tabs: vec![tab],
                });
                let existing_pane = PaneNode::Leaf(pane.clone());

                let (first, second) = match drop_zone {
                    DropZone::Left | DropZone::Top => (new_pane, existing_pane),
                    DropZone::Right | DropZone::Bottom => (existing_pane, new_pane),
                    DropZone::Center => return false,
                };

                *node = PaneNode::Split {
                    id: new_split_id,
                    axis,
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            PaneNode::Leaf(_) => false,
            PaneNode::Split { first, second, .. } => {
                Self::split_leaf_with_tab(
                    first,
                    pane_id,
                    tab.clone(),
                    new_pane_id,
                    new_split_id,
                    drop_zone,
                ) || Self::split_leaf_with_tab(
                    second,
                    pane_id,
                    tab,
                    new_pane_id,
                    new_split_id,
                    drop_zone,
                )
            }
        }
    }

    fn collapse_empty_panes(node: &mut PaneNode) -> bool {
        let replacement = match node {
            PaneNode::Leaf(pane) => return pane.tabs.is_empty(),
            PaneNode::Split { first, second, .. } => {
                let first_empty = Self::collapse_empty_panes(first);
                let second_empty = Self::collapse_empty_panes(second);

                match (first_empty, second_empty) {
                    (true, true) => return true,
                    (true, false) => Some((**second).clone()),
                    (false, true) => Some((**first).clone()),
                    (false, false) => None,
                }
            }
        };

        if let Some(replacement) = replacement {
            *node = replacement;
        }

        false
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
                    cx.listener(|this, event: &DragMoveEvent<SplitResize>, _, cx| {
                        let drag = *event.drag(cx);
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
            .bg(rgb(0x0f172a))
            .border_1()
            .border_color(rgb(0x263244))
            .text_color(rgb(0xe5e7eb))
            .child(
                div()
                    .h(px(44.0))
                    .w_full()
                    .flex()
                    .items_end()
                    .gap_1()
                    .px_3()
                    .pt_2()
                    .bg(rgb(0x111827))
                    .border_b_1()
                    .border_color(rgb(0x2d3748))
                    .overflow_hidden()
                    .children(tab_elements)
                    .child(
                        div()
                            .id(("add-tab", pane.id))
                            .size(px(32.0))
                            .mb_1()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.0))
                            .text_lg()
                            .text_color(rgb(0xcbd5e1))
                            .cursor_pointer()
                            .hover(|this| this.bg(rgb(0x1f2937)).text_color(rgb(0xffffff)))
                            .on_click(cx.listener({
                                let pane_id = pane.id;
                                move |this, _, _, cx| {
                                    this.add_tab(pane_id, cx);
                                }
                            }))
                            .child("+"),
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
                    DropZone::Center => this.move_tab_to_pane(*drag, pane_id, cx),
                    DropZone::Left | DropZone::Right | DropZone::Top | DropZone::Bottom => {
                        this.split_pane(*drag, pane_id, drop_zone, cx)
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
        let can_close = Self::total_tabs_in(&self.root) > 1;
        let hover_group = SharedString::from(format!("tab-{pane_id}-{id}"));

        div()
            .id(("tab", id))
            .group(hover_group.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(36.0))
            .w(px(154.0))
            .px_3()
            .rounded_t(px(7.0))
            .border_1()
            .border_color(if is_active {
                rgb(0x3b82f6)
            } else {
                rgb(0x2d3748)
            })
            .bg(if is_active {
                rgb(0x111827)
            } else {
                rgb(0x1f2937)
            })
            .text_color(if is_active {
                rgb(0xffffff)
            } else {
                rgb(0xcbd5e1)
            })
            .text_sm()
            .cursor_move()
            .hover(|this| this.bg(rgb(0x273449)))
            .drag_over::<TabDrag>(move |this, drag, _, _| {
                if drag.id == id {
                    this
                } else {
                    this.border_color(rgb(0x60a5fa)).bg(rgb(0x1e3a5f))
                }
            })
            .can_drop(move |drag, _, _| {
                drag.downcast_ref::<TabDrag>()
                    .is_some_and(|drag| drag.id != id)
            })
            .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
                cx.stop_propagation();
                this.move_tab_before(*drag, pane_id, id, cx);
            }))
            .on_drag(TabDrag::new(id, pane_id), |drag, position, _, cx| {
                cx.new(|_| drag.position(position))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_tab(pane_id, id, cx);
            }))
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
                    .child("x"),
            )
    }
}

impl Render for TabApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_1()
            .bg(rgb(0x0b1120))
            .child(self.render_node(&self.root, cx))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| TabApp::new()),
        )
        .unwrap();

        cx.activate(true);
    });
}

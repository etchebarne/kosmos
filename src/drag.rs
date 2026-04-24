use gpui::{Context, IntoElement, Pixels, Point, Render, Window, div, prelude::*, px, rgb};

use crate::pane_tree::SplitAxis;

#[derive(Clone, Copy)]
pub struct TabDrag {
    pub id: usize,
    pub source_pane_id: usize,
    position: Point<Pixels>,
}

impl TabDrag {
    pub fn new(id: usize, source_pane_id: usize) -> Self {
        Self {
            id,
            source_pane_id,
            position: Point::default(),
        }
    }

    pub fn position(mut self, position: Point<Pixels>) -> Self {
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
pub struct SplitResize {
    pub split_id: usize,
    pub axis: SplitAxis,
    position: Point<Pixels>,
}

impl SplitResize {
    pub fn new(split_id: usize, axis: SplitAxis) -> Self {
        Self {
            split_id,
            axis,
            position: Point::default(),
        }
    }

    pub fn position(mut self, position: Point<Pixels>) -> Self {
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

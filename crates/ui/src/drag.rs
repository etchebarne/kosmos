use gpui::{
    Context, IntoElement, Pixels, Point, Render, SharedString, Window, div, prelude::*, rems,
};

use icons::{Icon, IconName};
use pane_tree::SplitAxis;
use theme::ActiveTheme;

use crate::metrics::{TAB_HEIGHT, TAB_RADIUS, TAB_WIDTH};

#[derive(Clone)]
pub struct TabDrag {
    pub id: usize,
    pub source_pane_id: usize,
    pub title: SharedString,
    pub icon: IconName,
    position: Point<Pixels>,
}

impl TabDrag {
    pub fn new(id: usize, source_pane_id: usize, title: SharedString, icon: IconName) -> Self {
        Self {
            id,
            source_pane_id,
            title,
            icon,
            position: Point::default(),
        }
    }

    pub fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for TabDrag {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let rem = window.rem_size();
        div()
            .pl(self.position.x - rems(4.375).to_pixels(rem))
            .pt(self.position.y - rems(1.125).to_pixels(rem))
            .child(
                div()
                    .h(TAB_HEIGHT)
                    .w(TAB_WIDTH)
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .rounded(TAB_RADIUS)
                    .bg(gpui::white().opacity(0.08))
                    .text_sm()
                    .text_color(theme.text_emphasis)
                    .shadow_lg()
                    .child(
                        Icon::new(self.icon)
                            .size(16.0)
                            .color(theme.text)
                            .into_any_element(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(self.title.clone()),
                    ),
            )
    }
}

#[derive(Clone, Copy)]
pub struct SplitResize {
    pub split_id: usize,
    pub axis: SplitAxis,
}

impl SplitResize {
    pub fn new(split_id: usize, axis: SplitAxis) -> Self {
        Self { split_id, axis }
    }
}

impl Render for SplitResize {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

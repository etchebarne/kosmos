use gpui::{
    Context, IntoElement, Pixels, Point, Render, SharedString, Window, div, prelude::*, px,
};

use icons::{Icon, IconName};
use theme::ActiveTheme;
use workspace::SplitAxis;

#[derive(Clone)]
pub struct TabDrag {
    pub id: usize,
    pub source_pane_id: usize,
    pub title: SharedString,
    position: Point<Pixels>,
}

impl TabDrag {
    pub fn new(id: usize, source_pane_id: usize, title: SharedString) -> Self {
        Self {
            id,
            source_pane_id,
            title,
            position: Point::default(),
        }
    }

    pub fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for TabDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        div()
            .pl(self.position.x - px(70.0))
            .pt(self.position.y - px(18.0))
            .child(
                div()
                    .h(px(32.0))
                    .w(px(154.0))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .rounded(px(6.0))
                    .bg(gpui::white().opacity(0.08))
                    .text_sm()
                    .text_color(theme.text_emphasis)
                    .shadow_lg()
                    .child(
                        Icon::new(IconName::File)
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

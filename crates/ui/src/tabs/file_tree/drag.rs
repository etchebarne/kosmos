use std::path::PathBuf;

use gpui::{
    Context, IntoElement, Pixels, Point, Render, SharedString, Window, div, prelude::*, rems,
};

use icons::{Icon, IconName};
use theme::ActiveTheme;

#[derive(Clone)]
pub struct FileNodeDrag {
    pub path: PathBuf,
    pub name: SharedString,
    pub icon: IconName,
    position: Point<Pixels>,
}

impl FileNodeDrag {
    pub fn new(path: PathBuf, name: SharedString, icon: IconName) -> Self {
        Self {
            path,
            name,
            icon,
            position: Point::default(),
        }
    }

    pub fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for FileNodeDrag {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let rem = window.rem_size();
        div()
            .pl(self.position.x - rems(0.5).to_pixels(rem))
            .pt(self.position.y - rems(0.75).to_pixels(rem))
            .child(
                div()
                    .h(rems(1.625))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(rems(0.3125))
                    .bg(theme.bg_selected)
                    .text_sm()
                    .text_color(theme.text_emphasis)
                    .shadow_lg()
                    .child(
                        Icon::new(self.icon)
                            .size(14.0)
                            .color(theme.text)
                            .into_any_element(),
                    )
                    .child(self.name.clone()),
            )
    }
}

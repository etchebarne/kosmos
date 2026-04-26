use std::rc::Rc;

use gpui::{
    App, ElementId, IntoElement, MouseButton, RenderOnce, Window, div, prelude::*, px,
};

use theme::ActiveTheme;

type ChangeHandler = Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    value: bool,
    on_change: Option<ChangeHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, value: bool) -> Self {
        Self {
            id: id.into(),
            value,
            on_change: None,
        }
    }

    pub fn on_change<F>(mut self, f: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let track_bg = if self.value {
            theme.accent
        } else {
            theme.bg_hover
        };
        let knob_offset = if self.value { 18.0 } else { 2.0 };
        let new_value = !self.value;
        let on_change = self.on_change;

        div()
            .id(self.id)
            .relative()
            .w(px(36.0))
            .h(px(20.0))
            .rounded(px(10.0))
            .bg(track_bg)
            .border_1()
            .border_color(theme.border)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                if let Some(handler) = &on_change {
                    handler(&new_value, window, cx);
                }
            })
            .child(
                div()
                    .absolute()
                    .top(px(2.0))
                    .left(px(knob_offset))
                    .size(px(14.0))
                    .rounded_full()
                    .bg(theme.text_emphasis),
            )
    }
}

use std::rc::Rc;

use gpui::{App, ElementId, IntoElement, MouseButton, RenderOnce, Window, div, prelude::*, rems};

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
            theme.bg_selected
        };
        let knob_bg = if theme.is_dark {
            theme.text_emphasis
        } else {
            gpui::rgb(0xffffff)
        };
        let knob_offset = if self.value { 1.1875 } else { 0.1875 };
        let new_value = !self.value;
        let on_change = self.on_change;

        div()
            .id(self.id)
            .relative()
            .w(rems(2.25))
            .h(rems(1.25))
            .rounded(rems(0.625))
            .bg(track_bg)
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
                    .top(rems(0.1875))
                    .left(rems(knob_offset))
                    .size(rems(0.875))
                    .rounded_full()
                    .bg(knob_bg),
            )
    }
}

use std::rc::Rc;

use gpui::{
    App, ElementId, IntoElement, MouseButton, RenderOnce, SharedString, Window, div, prelude::*,
    rems,
};

use theme::ActiveTheme;

type ChangeHandler = Rc<dyn Fn(&i64, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct NumericInput {
    id: SharedString,
    value: i64,
    min: Option<i64>,
    max: Option<i64>,
    step: i64,
    on_change: Option<ChangeHandler>,
}

impl NumericInput {
    pub fn new(id: impl Into<SharedString>, value: i64) -> Self {
        Self {
            id: id.into(),
            value,
            min: None,
            max: None,
            step: 1,
            on_change: None,
        }
    }

    pub fn min(mut self, min: i64) -> Self {
        self.min = Some(min);
        self
    }

    pub fn max(mut self, max: i64) -> Self {
        self.max = Some(max);
        self
    }

    pub fn step(mut self, step: i64) -> Self {
        self.step = step.max(1);
        self
    }

    pub fn on_change<F>(mut self, f: F) -> Self
    where
        F: Fn(&i64, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for NumericInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let value = self.value;
        let step = self.step;
        let can_decrement = self.min.is_none_or(|m| value > m);
        let can_increment = self.max.is_none_or(|m| value < m);
        let dec_value = self
            .min
            .map(|m| (value - step).max(m))
            .unwrap_or(value - step);
        let inc_value = self
            .max
            .map(|m| (value + step).min(m))
            .unwrap_or(value + step);

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(stepper(
                ElementId::Name(format!("{}-dec", self.id).into()),
                "-",
                can_decrement,
                dec_value,
                self.on_change.clone(),
            ))
            .child(
                div()
                    .h(rems(1.75))
                    .min_w(rems(3.5))
                    .px_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(rems(0.3125))
                    .bg(theme.bg_elevated)
                    .border_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.text)
                    .child(value.to_string()),
            )
            .child(stepper(
                ElementId::Name(format!("{}-inc", self.id).into()),
                "+",
                can_increment,
                inc_value,
                self.on_change,
            ))
    }
}

fn stepper(
    id: ElementId,
    label: &'static str,
    enabled: bool,
    new_value: i64,
    on_change: Option<ChangeHandler>,
) -> impl IntoElement {
    StepperButton {
        id,
        label,
        enabled,
        new_value,
        on_change,
    }
}

#[derive(IntoElement)]
struct StepperButton {
    id: ElementId,
    label: &'static str,
    enabled: bool,
    new_value: i64,
    on_change: Option<ChangeHandler>,
}

impl RenderOnce for StepperButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let text_color = if self.enabled {
            theme.text
        } else {
            theme.text_subtle
        };
        let new_value = self.new_value;
        let enabled = self.enabled;
        let on_change = self.on_change;

        div()
            .id(self.id)
            .size(rems(1.75))
            .flex()
            .items_center()
            .justify_center()
            .rounded(rems(0.3125))
            .bg(theme.bg_elevated)
            .border_1()
            .border_color(theme.border)
            .text_sm()
            .text_color(text_color)
            .when(enabled, |this| {
                this.hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_emphasis))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                if !enabled {
                    return;
                }
                if let Some(handler) = &on_change {
                    handler(&new_value, window, cx);
                }
            })
            .child(self.label)
    }
}

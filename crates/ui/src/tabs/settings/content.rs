use gpui::{AnyElement, Context, IntoElement, div, prelude::*, rems};

use settings::{ActiveSettings, Category, Setting};
use theme::ActiveTheme;

use crate::delegate::SettingsDelegate;
use crate::tabs::settings::controls;

pub fn render<T: SettingsDelegate>(
    category: &'static Category,
    open_dropdown: Option<&'static str>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let mut rows: Vec<AnyElement> = Vec::new();
    for setting in category.settings {
        rows.push(render_row(setting, open_dropdown, cx));
    }

    div()
        .id("settings-content")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(rems(1.5))
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_xl()
                .text_color(theme.text_emphasis)
                .child(category.name),
        )
        .children(rows)
        .into_any_element()
}

fn render_row<T: SettingsDelegate>(
    setting: &'static Setting,
    open_dropdown: Option<&'static str>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let value = cx.settings().value(setting);
    let description = setting.description.map(|desc| {
        div()
            .text_sm()
            .text_color(theme.text_subtle)
            .child(desc)
            .into_any_element()
    });

    div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap_4()
        .py_2()
        .border_b_1()
        .border_color(theme.border_subtle)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.text)
                        .child(setting.name),
                )
                .children(description),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .child(controls::render(setting, &value, open_dropdown, cx)),
        )
        .into_any_element()
}

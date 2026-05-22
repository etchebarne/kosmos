use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, rems};
use gpui_component::scroll::ScrollableElement;

use registry::ToolKind;
use settings::{ActiveSettings, Category, Setting};
use theme::ActiveTheme;

use crate::delegate::SettingsDelegate;
use crate::tabs::settings::{cards, controls};

pub fn render<T: SettingsDelegate>(
    category: &'static Category,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let body: AnyElement = match category.id {
        "language_servers" => cards::render_marketplace(ToolKind::Lsp, cx),
        "formatters" => cards::render_marketplace(ToolKind::Formatter, cx),
        "linters" => cards::render_marketplace(ToolKind::Linter, cx),
        _ => render_rows(category, open_dropdown, window, cx),
    };

    div()
        .id("settings-content")
        .flex_1()
        .min_w_0()
        .h_full()
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
        .child(body)
        .overflow_y_scrollbar()
        .into_any_element()
}

fn render_rows<T: SettingsDelegate>(
    category: &'static Category,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut current_group: Option<&'static str> = None;
    for setting in category.settings {
        if setting.group != current_group {
            current_group = setting.group;
            if let Some(group) = setting.group {
                rows.push(render_group_header(group, theme));
            }
        }
        rows.push(render_row(setting, open_dropdown, window, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap_4()
        .children(rows)
        .into_any_element()
}

fn render_group_header(name: &'static str, theme: theme::Theme) -> AnyElement {
    div()
        .pt_4()
        .pb_1()
        .text_lg()
        .text_color(theme.text_emphasis)
        .child(name)
        .into_any_element()
}

fn render_row<T: SettingsDelegate>(
    setting: &'static Setting,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
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
        .items_center()
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
                .child(div().text_sm().text_color(theme.text).child(setting.name))
                .children(description),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .child(controls::render(setting, &value, open_dropdown, window, cx)),
        )
        .into_any_element()
}

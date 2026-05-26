use gpui::{AnyElement, Context, Entity, IntoElement, SharedString, Window, div, prelude::*, rems};
use gpui_component::{
    Icon as ComponentIcon, IconName as ComponentIconName, Sizable, Size,
    alert::Alert,
    group_box::{GroupBox, GroupBoxVariants},
    input::{Input, InputState},
    label::Label,
    scroll::ScrollableElement,
};

use registry::ToolKind;
use settings::{ActiveSettings, Category, Setting, Settings};
use theme::ActiveTheme;

use crate::delegate::SettingsDelegate;
use crate::tabs::settings::{cards, controls};

pub fn render<T: SettingsDelegate>(
    search: Option<Entity<InputState>>,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let query = search
        .as_ref()
        .map(|input| input.read(cx).value().trim().to_lowercase())
        .unwrap_or_default();
    let sections: Vec<AnyElement> = Settings::categories()
        .iter()
        .filter_map(|category| render_category(category, &query, open_dropdown, window, cx))
        .collect();

    let content = if sections.is_empty() {
        vec![render_empty_state(&query)]
    } else {
        sections
    };

    div()
        .id("settings-content")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .h_full()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p(rems(1.5))
                .pb_4()
                .border_b_1()
                .border_color(theme.border_subtle)
                .child(
                    Label::new("Settings")
                        .text_xl()
                        .text_color(theme.text_emphasis),
                )
                .child(render_search(search)),
        )
        .child(
            div().flex_1().min_h_0().child(
                div()
                    .size_full()
                    .child(
                        div()
                            .px(rems(1.5))
                            .pb(rems(1.5))
                            .flex()
                            .flex_col()
                            .gap_4()
                            .children(content),
                    )
                    .overflow_y_scrollbar(),
            ),
        )
        .into_any_element()
}

fn render_search(search: Option<Entity<InputState>>) -> AnyElement {
    match search {
        Some(search) => div()
            .w_full()
            .min_w_0()
            .max_w(rems(28.0))
            .child(
                Input::new(&search)
                    .prefix(ComponentIconName::Search)
                    .cleanable(true)
                    .w_full(),
            )
            .into_any_element(),
        None => div().into_any_element(),
    }
}

fn render_category<T: SettingsDelegate>(
    category: &'static Category,
    query: &str,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> Option<AnyElement> {
    let theme = *cx.theme();
    let category_match =
        query.is_empty() || text_matches(category.id, query) || text_matches(category.name, query);
    let body = match marketplace_kind(category) {
        Some(kind) => {
            if !category_match && !cards::has_match(kind, query) {
                return None;
            }
            cards::render_marketplace(kind, query, category_match, cx)
        }
        None => {
            if !category_match
                && !category
                    .settings
                    .iter()
                    .any(|setting| setting_matches(setting, query))
            {
                return None;
            }
            render_rows(category, query, category_match, open_dropdown, window, cx)
        }
    };

    Some(
        GroupBox::new()
            .id(SharedString::from(format!(
                "settings-category:{}",
                category.id
            )))
            .outline()
            .title(render_category_title(category, query, theme))
            .child(body)
            .into_any_element(),
    )
}

fn marketplace_kind(category: &Category) -> Option<ToolKind> {
    match category.id {
        "language_servers" => Some(ToolKind::Lsp),
        "formatters" => Some(ToolKind::Formatter),
        "linters" => Some(ToolKind::Linter),
        _ => None,
    }
}

fn render_category_title(
    category: &'static Category,
    query: &str,
    theme: theme::Theme,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(ComponentIcon::empty().path(category.icon.path()).small())
        .child(
            Label::new(category.name)
                .text_lg()
                .text_color(theme.text_emphasis)
                .highlights(query.to_string()),
        )
        .into_any_element()
}

fn render_rows<T: SettingsDelegate>(
    category: &'static Category,
    query: &str,
    include_all: bool,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut current_group: Option<&'static str> = None;

    for setting in category
        .settings
        .iter()
        .filter(|setting| include_all || setting_matches(setting, query))
    {
        if setting.group != current_group {
            current_group = setting.group;
            if let Some(group) = setting.group {
                rows.push(render_group_header(group, query, theme));
            }
        }
        rows.push(render_row(setting, query, open_dropdown, window, cx));
    }

    div()
        .flex()
        .flex_col()
        .gap_4()
        .children(rows)
        .into_any_element()
}

fn render_group_header(name: &'static str, query: &str, theme: theme::Theme) -> AnyElement {
    div()
        .pt_4()
        .pb_1()
        .child(
            Label::new(name)
                .text_lg()
                .text_color(theme.text_emphasis)
                .highlights(query.to_string()),
        )
        .into_any_element()
}

fn render_row<T: SettingsDelegate>(
    setting: &'static Setting,
    query: &str,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let value = cx.settings().value(setting);
    let description = setting.description.map(|desc| {
        Label::new(desc)
            .text_sm()
            .text_color(theme.text_subtle)
            .highlights(query.to_string())
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
                .child(
                    Label::new(setting.name)
                        .text_sm()
                        .text_color(theme.text)
                        .highlights(query.to_string()),
                )
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

fn render_empty_state(query: &str) -> AnyElement {
    let message = if query.is_empty() {
        "No settings available."
    } else {
        "No settings match your search."
    };
    Alert::info("settings-no-results", message)
        .with_size(Size::Small)
        .into_any_element()
}

fn setting_matches(setting: &Setting, query: &str) -> bool {
    query.is_empty()
        || text_matches(setting.id, query)
        || text_matches(setting.name, query)
        || setting
            .description
            .is_some_and(|description| text_matches(description, query))
        || setting
            .group
            .is_some_and(|group| text_matches(group, query))
}

fn text_matches(text: &str, query: &str) -> bool {
    text.to_lowercase().contains(query)
}

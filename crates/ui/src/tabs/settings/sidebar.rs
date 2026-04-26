use gpui::{AnyElement, Context, IntoElement, MouseButton, SharedString, div, prelude::*, rems};

use icons::Icon;
use settings::{Category, Settings};
use theme::ActiveTheme;

use crate::delegate::SettingsDelegate;

pub fn render<T: SettingsDelegate>(active_id: &str, cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let mut items: Vec<AnyElement> = Vec::new();
    for category in Settings::categories() {
        items.push(render_item(category, category.id == active_id, cx));
    }

    div()
        .w(rems(12.5))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.bg_surface)
        .children(items)
        .into_any_element()
}

fn render_item<T: SettingsDelegate>(
    category: &'static Category,
    is_active: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let id = category.id;
    let bg = if is_active {
        theme.bg_selected
    } else {
        theme.bg_surface
    };
    let text_color = if is_active {
        theme.text_emphasis
    } else {
        theme.text_muted
    };

    div()
        .id(SharedString::from(format!("settings-category:{id}")))
        .h(rems(1.875))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(rems(0.3125))
        .bg(bg)
        .text_sm()
        .text_color(text_color)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.select_settings_category(id, cx);
        }))
        .child(Icon::new(category.icon).size(14.0).color(text_color))
        .child(category.name)
        .into_any_element()
}

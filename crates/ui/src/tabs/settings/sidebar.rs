use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, rems};
use gpui_component::{
    Icon as ComponentIcon, Selectable,
    button::{Button, ButtonVariants},
};

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
    let id = category.id;
    Button::new(SharedString::from(format!("settings-category:{id}")))
        .ghost()
        .selected(is_active)
        .icon(ComponentIcon::empty().path(category.icon.path()))
        .label(category.name)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.select_settings_category(id, cx);
        }))
        .into_any_element()
}

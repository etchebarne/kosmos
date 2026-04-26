use gpui::{AnyElement, App};

use tabs::registry;

use super::placeholder;

pub fn render(cx: &mut App) -> AnyElement {
    placeholder::render(registry::SETTINGS.icon, registry::SETTINGS.name, cx)
}

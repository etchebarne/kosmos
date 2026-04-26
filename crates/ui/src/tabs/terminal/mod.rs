use gpui::{AnyElement, App};

use tabs::registry;

use super::placeholder;

pub fn render(cx: &mut App) -> AnyElement {
    placeholder::render(registry::TERMINAL.icon, registry::TERMINAL.name, cx)
}

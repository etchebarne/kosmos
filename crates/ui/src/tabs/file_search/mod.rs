use gpui::{AnyElement, App};

use tabs::registry;

use super::placeholder;

pub fn render(cx: &mut App) -> AnyElement {
    placeholder::render(registry::FILE_SEARCH.icon, registry::FILE_SEARCH.name, cx)
}

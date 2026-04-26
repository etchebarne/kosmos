use gpui::{AnyElement, App};

use tabs::registry;

use super::placeholder;

pub fn render(cx: &mut App) -> AnyElement {
    placeholder::render(registry::FILE_TREE.icon, registry::FILE_TREE.name, cx)
}

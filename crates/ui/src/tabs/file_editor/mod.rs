use gpui::{AnyElement, App};

use tabs::{Tab, registry};

use super::placeholder;

pub fn render(_tab: &Tab, cx: &mut App) -> AnyElement {
    placeholder::render(registry::FILE_EDITOR.icon, registry::FILE_EDITOR.name, cx)
}

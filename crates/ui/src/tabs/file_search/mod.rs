use gpui::{AnyElement, App};

use tabs::registry;

use super::{icon_for_kind, placeholder};

pub fn render(cx: &mut App) -> AnyElement {
    placeholder::render(
        icon_for_kind(registry::FILE_SEARCH.id),
        registry::FILE_SEARCH.name,
        cx,
    )
}

mod pane;
mod pane_tree;
mod tab;

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use theme::ActiveTheme;
use workspace::WorkspaceManager;

use crate::delegate::{PaneDelegate, SettingsDelegate, TabScrollHandles, WorkspaceDelegate};
use crate::layout::landing;

pub fn render<T: PaneDelegate + WorkspaceDelegate + SettingsDelegate>(
    workspaces: &WorkspaceManager,
    tab_scrolls: &TabScrollHandles,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    match workspaces.active_pane_tree() {
        Some(tree) => div()
            .size_full()
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .child(pane_tree::render(tree, tree.root(), tab_scrolls, cx))
            .into_any_element(),
        None => landing::render(cx),
    }
}

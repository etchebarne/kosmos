mod pane;
mod pane_tree;
mod tab;

use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, rems};
use theme::ActiveTheme;
use workspace::WorkspaceManager;

use crate::delegate::{PaneDelegate, SettingsDelegate, TabScrollHandles, WorkspaceDelegate};
use crate::layout::landing;

pub fn render<T: PaneDelegate + WorkspaceDelegate + SettingsDelegate + gpui::Render>(
    workspaces: &WorkspaceManager,
    tab_scrolls: &TabScrollHandles,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    match workspaces.active_workspace() {
        Some(workspace) => div()
            .size_full()
            .rounded(rems(0.5))
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .child(pane_tree::render(
                &workspace.pane_tree,
                workspace.pane_tree.root(),
                workspace.id,
                &workspace.path,
                tab_scrolls,
                window,
                cx,
            ))
            .into_any_element(),
        None => landing::render(cx),
    }
}

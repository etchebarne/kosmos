use gpui::{Context, IntoElement, Render, Window, div, prelude::*};

use pane_tree::PaneTree;
use theme::ActiveTheme;
use ui::delegate::HeaderMenu;
use ui::layout;
use workspace::WorkspaceManager;

pub struct KosmosApp {
    pub(crate) active_menu: Option<HeaderMenu>,
    pub(crate) workspaces: WorkspaceManager,
}

impl KosmosApp {
    pub fn new() -> Self {
        Self {
            active_menu: None,
            workspaces: persistence::load(),
        }
    }

    pub fn start_observing_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.observe_window_bounds(window, |_, window, _| {
            persistence::save_window_bounds(window.window_bounds());
        })
        .detach();
    }

    pub(crate) fn persist_active_workspace(&self) {
        if let Some(workspace) = self.workspaces.active_workspace() {
            persistence::save_workspace(workspace);
        }
    }

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.active_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn mutate_active_tree(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut PaneTree) -> bool,
    ) {
        let Some(tree) = self.workspaces.active_pane_tree_mut() else {
            return;
        };
        if !f(tree) {
            return;
        }
        cx.notify();
        self.persist_active_workspace();
    }
}

impl Render for KosmosApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        div()
            .id("app-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .bg(theme.bg_root)
            .on_click(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .child(layout::header::render(self.active_menu, &self.workspaces, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(layout::main_content::render(&self.workspaces, cx)),
            )
            .child(layout::bottom_bar::render(&theme))
    }
}

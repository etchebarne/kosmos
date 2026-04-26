use gpui::{Context, FocusHandle, IntoElement, Render, Window, div, prelude::*};

use gpui::BorrowAppContext;
use pane_tree::{PaneTree, WirePaneTreeActions};
use theme::ActiveTheme;
use ui::delegate::{HeaderMenu, SettingsUiState, TabScrollHandles};
use ui::layout;
use ui::tabs::settings::SettingsInputs;
use workspace::WorkspaceManager;
use zoom::WireZoomActions;

pub struct KosmosApp {
    pub(crate) active_menu: Option<HeaderMenu>,
    pub(crate) workspaces: WorkspaceManager,
    pub(crate) tab_scrolls: TabScrollHandles,
    focus_handle: FocusHandle,
}

impl KosmosApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        SettingsInputs::install(cx);
        Self {
            active_menu: None,
            workspaces: persistence::load(),
            tab_scrolls: TabScrollHandles::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn start_observing_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window);
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
        let mut changed = self.active_menu.take().is_some();
        cx.update_global::<SettingsUiState, _>(|state, _| {
            if state.open_dropdown.take().is_some() {
                changed = true;
            }
        });
        if changed {
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        zoom::apply(window, cx);
        div()
            .id("app-root")
            .track_focus(&self.focus_handle)
            .key_context(shortcuts::CONTEXT)
            .wire_pane_tree_actions(cx)
            .wire_zoom_actions(cx)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .bg(theme.bg_root)
            .on_click(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .child(layout::header::render(
                self.active_menu,
                &self.workspaces,
                cx,
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(layout::main_content::render(
                        &self.workspaces,
                        &self.tab_scrolls,
                        cx,
                    )),
            )
            .child(layout::bottom_bar::render(&theme))
    }
}

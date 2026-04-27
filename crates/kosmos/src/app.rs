use std::path::PathBuf;

use gpui::{App, AppContext, Context, Entity, FocusHandle, IntoElement, Render, Window, div, prelude::*};

use file_tree::{FileTree, FileTreeState};
use gpui::BorrowAppContext;
use pane_tree::{PaneTree, WirePaneTreeActions};
use settings::{ActiveSettings, SettingValue};
use theme::{ActiveTheme, REGISTRY as THEME_REGISTRY, SETTING_ID as THEME_SETTING_ID, Theme};
use ui::delegate::{HeaderMenu, SettingsUiState, TabScrollHandles, WorkspaceMenuState};
use ui::layout;
use ui::tabs::settings::SettingsInputs;
use workspace::WorkspaceManager;
use zoom::WireZoomActions;

/// Sync the global `Theme` with the user's chosen theme setting.
fn apply_theme(cx: &mut App) {
    let raw = cx
        .settings()
        .get(THEME_SETTING_ID)
        .and_then(SettingValue::as_str)
        .unwrap_or(theme::DEFAULT_ID);
    let id = THEME_REGISTRY
        .iter()
        .find(|c| c.id == raw)
        .map(|c| c.id)
        .unwrap_or(theme::DEFAULT_ID);
    cx.set_global(Theme::by_id(id));
}

pub struct KosmosApp {
    pub(crate) active_menu: Option<HeaderMenu>,
    pub(crate) workspace_menu: Option<WorkspaceMenuState>,
    pub(crate) workspaces: WorkspaceManager,
    pub(crate) tab_scrolls: TabScrollHandles,
    pub(crate) file_tree: Entity<FileTree>,
    focus_handle: FocusHandle,
}

impl KosmosApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        SettingsInputs::install(cx);
        let workspaces = persistence::load();
        let file_tree = cx.new(FileTree::new);
        cx.observe(&file_tree, |_, _, cx| cx.notify()).detach();
        cx.set_global(FileTreeState::new());
        cx.update_global::<FileTreeState, _>(|state, _| {
            state.set_active(Some(file_tree.clone()));
        });
        let mut app = Self {
            active_menu: None,
            workspace_menu: None,
            workspaces,
            tab_scrolls: TabScrollHandles::new(),
            file_tree,
            focus_handle: cx.focus_handle(),
        };
        app.sync_file_tree_root(cx);
        app
    }

    pub(crate) fn sync_file_tree_root(&mut self, cx: &mut Context<Self>) {
        let path: Option<PathBuf> = self
            .workspaces
            .active_workspace()
            .map(|w| w.path.clone());
        if let Some(path) = path {
            self.file_tree.update(cx, |tree, cx| {
                tree.set_root(path, cx);
            });
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
        if self.workspace_menu.take().is_some() {
            changed = true;
        }
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
        apply_theme(cx);
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
                self.workspace_menu,
                window,
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

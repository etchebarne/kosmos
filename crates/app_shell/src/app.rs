use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, IntoElement, Render, Task, Window, div,
    prelude::*,
};

use file_editor::BufferStore;
use file_tree::{FileTree, FileTreeEvent, FileTreeState};
use gpui::BorrowAppContext;
use pane_tree::PaneTree;
use settings::{ActiveSettings, SettingValue};
use theme::{ActiveTheme, REGISTRY as THEME_REGISTRY, SETTING_ID as THEME_SETTING_ID, Theme};
use ui::delegate::{
    HeaderMenu, HeaderMenuAction, HeaderMenuAvailability, SettingsUiState, TabScrollHandles,
    WorkspaceMenuState,
};
use ui::layout;
use ui::pane_tree_actions::WirePaneTreeActions;
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

pub(crate) struct KosmosApp {
    pub(crate) active_menu: Option<HeaderMenu>,
    pub(crate) workspace_menu: Option<WorkspaceMenuState>,
    pub(crate) workspaces: WorkspaceManager,
    pub(crate) tab_scrolls: TabScrollHandles,
    pub(crate) file_tree: Entity<FileTree>,
    focus_handle: FocusHandle,
    workspace_watch_task: Option<Task<()>>,
    /// Set whenever a transient mutation (e.g. a pane resize drag-move) has
    /// changed workspace state without writing to disk. Drained by the next
    /// non-transient mutation or on app quit, so the user's last resize ratio
    /// always lands in the database eventually without thrashing the disk.
    pending_persist: bool,
}

impl KosmosApp {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        SettingsInputs::install(cx);
        let workspaces = persistence::load();
        let file_tree = Self::create_file_tree(cx);
        let mut app = Self {
            active_menu: None,
            workspace_menu: None,
            workspaces,
            tab_scrolls: TabScrollHandles::new(),
            file_tree,
            focus_handle: cx.focus_handle(),
            workspace_watch_task: None,
            pending_persist: false,
        };
        app.sync_file_tree_root(cx);
        app.start_workspace_watch_task(cx);
        app.flush_pending_persist_on_quit(cx);
        app
    }

    fn create_file_tree(cx: &mut Context<Self>) -> Entity<FileTree> {
        let file_tree = cx.new(FileTree::new);
        cx.observe(&file_tree, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&file_tree, |_, _, event, cx| match event {
            FileTreeEvent::FsChanged { paths } => BufferStore::reload_paths(paths.clone(), cx),
        })
        .detach();
        cx.set_global(FileTreeState::new());
        cx.update_global::<FileTreeState, _>(|state, _| {
            state.set_active(Some(file_tree.clone()));
        });
        file_tree
    }

    fn flush_pending_persist_on_quit(&mut self, cx: &mut Context<Self>) {
        cx.on_app_quit(|this, _cx| {
            // Flush any resize-drag mutations that bypassed persistence so
            // the last ratio lands on disk before the process exits.
            this.flush_pending_persist();
            async {}
        })
        .detach();
    }

    /// Periodically verify that every open workspace's backing directory
    /// still exists. If a directory is removed externally, auto-close the
    /// workspace so it doesn't dangle in the UI.
    fn start_workspace_watch_task(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;

                let Ok(paths) = this.update(cx, |this, _| {
                    this.workspaces
                        .workspaces()
                        .iter()
                        .map(|w| (w.id, w.path.clone()))
                        .collect::<Vec<_>>()
                }) else {
                    break;
                };

                if paths.is_empty() {
                    continue;
                }

                let missing = cx
                    .background_executor()
                    .spawn(async move {
                        paths
                            .into_iter()
                            .filter(|(_, path)| matches!(path.try_exists(), Ok(false)))
                            .map(|(id, _)| id)
                            .collect::<Vec<_>>()
                    })
                    .await;

                if missing.is_empty() {
                    continue;
                }

                if this
                    .update(cx, |this, cx| {
                        let mut closed = false;
                        for id in missing {
                            if this.workspaces.close(id) {
                                closed = true;
                            }
                        }
                        if closed {
                            this.sync_file_tree_root(cx);
                            cx.notify();
                            persistence::save_session(&this.workspaces);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        self.workspace_watch_task = Some(task);
    }

    pub(crate) fn sync_file_tree_root(&mut self, cx: &mut Context<Self>) {
        let path: Option<PathBuf> = self.workspaces.active_workspace().map(|w| w.path.clone());
        if let Some(path) = path {
            self.file_tree.update(cx, |tree, cx| {
                tree.set_root(path, cx);
            });
        }
    }

    pub(crate) fn start_observing_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window);
        cx.observe_window_bounds(window, |_, window, _| {
            persistence::save_window_bounds(window.window_bounds());
        })
        .detach();
    }

    pub(crate) fn persist_active_workspace(&mut self) {
        if let Some(workspace) = self.workspaces.active_workspace() {
            persistence::save_workspace(workspace);
        }
        self.pending_persist = false;
    }

    /// Write any deferred state to disk if a transient mutation left a
    /// pending change behind. No-op when the cache is clean.
    pub(crate) fn flush_pending_persist(&mut self) {
        if self.pending_persist {
            self.persist_active_workspace();
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

    /// Mutate the active pane tree without writing to disk. Use for high-rate
    /// drag-driven operations like pane resize, where a per-frame SQLite
    /// transaction (delete-and-rewrite of every pane node) is the dominant
    /// cost. The mutation is marked dirty and gets flushed by the next
    /// regular `mutate_active_tree` call or by `on_app_quit`.
    pub(crate) fn mutate_active_tree_transient(
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
        self.pending_persist = true;
    }

    fn active_tab(&self) -> Option<&tabs::Tab> {
        self.workspaces
            .active_pane_tree()
            .and_then(|tree| tree.active_pane())
            .and_then(|pane| pane.tabs().iter().find(|tab| tab.id == pane.active_tab()))
    }

    fn active_editor_tab(&self) -> Option<&tabs::Tab> {
        self.active_tab()
            .filter(|tab| tab.kind.as_str() == tabs::registry::FILE_EDITOR.id)
            .filter(|tab| tab.path.is_some())
    }

    fn active_editor_tab_parts(&self) -> Option<(usize, PathBuf)> {
        let tab = self.active_editor_tab()?;
        Some((tab.id, tab.path.clone()?))
    }

    pub(crate) fn header_menu_availability(&self, cx: &Context<Self>) -> HeaderMenuAvailability {
        let mut availability = HeaderMenuAvailability {
            any_dirty_file: BufferStore::has_dirty_buffers(cx),
            ..Default::default()
        };

        let Some(tab) = self.active_editor_tab() else {
            return availability;
        };

        availability.active_editor = true;
        availability.can_cut = true;
        availability.can_copy = true;
        availability.can_paste = true;
        availability.can_select_all = true;

        if let Some(path) = tab.path.as_deref() {
            availability.active_editor_dirty = BufferStore::is_path_dirty(path, cx);
        }

        if let Some(view) = file_editor::EditorViewStore::get(tab.id, cx) {
            let view = view.read(cx);
            availability.can_undo = view.can_undo(cx);
            availability.can_redo = view.can_redo(cx);
            availability.can_cut = view.can_cut(cx);
            availability.can_copy = view.can_copy(cx);
            availability.can_paste = view.can_paste();
            availability.can_select_all = view.can_select_all(cx);
        }

        availability
    }

    fn active_editor_view(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Entity<file_editor::EditorView>> {
        let (tab_id, path) = self.active_editor_tab_parts()?;
        let buffer = BufferStore::open(path, cx);
        let view = file_editor::EditorViewStore::for_tab(tab_id, &buffer, cx);
        view.update(cx, |view, cx| view.set_buffer(buffer, cx));
        Some(view)
    }

    pub(crate) fn save_active_editor(&mut self, cx: &mut Context<Self>) {
        let Some((_, path)) = self.active_editor_tab_parts() else {
            return;
        };
        if let Err(err) = BufferStore::save_path(&path, cx) {
            eprintln!("failed to save {}: {err}", path.display());
        }
        cx.notify();
    }

    pub(crate) fn save_all_files(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = BufferStore::save_all(cx) {
            eprintln!("failed to save all files: {err}");
        }
        cx.notify();
    }

    pub(crate) fn run_header_editor_action(
        &mut self,
        action: HeaderMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.active_editor_view(cx) else {
            return;
        };
        let focus_handle = view.read(cx).focus_handle();
        window.focus(&focus_handle);
        view.update(cx, |view, cx| match action {
            HeaderMenuAction::Undo => view.undo(window, cx),
            HeaderMenuAction::Redo => view.redo(window, cx),
            HeaderMenuAction::Cut => view.cut(window, cx),
            HeaderMenuAction::Copy => view.copy(window, cx),
            HeaderMenuAction::Paste => view.paste(window, cx),
            HeaderMenuAction::SelectAll => view.select_all(window, cx),
            HeaderMenuAction::OpenFolder
            | HeaderMenuAction::Save
            | HeaderMenuAction::SaveAll
            | HeaderMenuAction::ExpandSelection
            | HeaderMenuAction::ShrinkSelection => {}
        });
        cx.notify();
    }

    fn save_active_file(&mut self, _: &file_editor::Save, _: &mut Window, cx: &mut Context<Self>) {
        self.save_active_editor(cx);
    }
}

impl Render for KosmosApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        apply_theme(cx);
        let theme = *cx.theme();
        let header_menu_availability = self.header_menu_availability(cx);
        zoom::apply(window, cx);
        div()
            .id("app-root")
            .track_focus(&self.focus_handle)
            .key_context(shortcuts::CONTEXT)
            .wire_pane_tree_actions(cx)
            .wire_zoom_actions(cx)
            .on_action(cx.listener(Self::save_active_file))
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
                header_menu_availability,
                window,
                cx,
            ))
            .child(div().flex_1().min_h_0().child(layout::main_content::render(
                &self.workspaces,
                &self.tab_scrolls,
                window,
                cx,
            )))
            .child(layout::bottom_bar::render(&theme))
            .child(ui::tabs::git::render_modal_overlay(cx))
            .child(ui::components::toast::render(cx))
    }
}

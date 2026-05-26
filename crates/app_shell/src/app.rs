use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, IntoElement, Render, Task, Window, div,
    prelude::*,
};

use file_editor::BufferStore;
use file_tree::{FileTree, FileTreeEvent, FileTreeState};
use gpui::BorrowAppContext;
use gpui_component::input as component_input;
use pane_tree::{PaneNode, PaneTree};
use theme::ActiveTheme;
use ui::delegate::{HeaderMenuAction, HeaderMenuAvailability, SettingsUiState, TabScrollHandles};
use ui::layout;
use ui::pane_tree_actions::WirePaneTreeActions;
use ui::tabs::settings::SettingsInputs;
use workspace::WorkspaceManager;
use zoom::WireZoomActions;

const TERMINAL_CWD_PERSIST_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) struct KosmosApp {
    pub(crate) workspaces: WorkspaceManager,
    pub(crate) tab_scrolls: TabScrollHandles,
    pub(crate) file_tree: Entity<FileTree>,
    focus_handle: FocusHandle,
    workspace_watch_task: Option<Task<()>>,
    terminal_cwd_persist_task: Option<Task<()>>,
    /// Set whenever a transient mutation (e.g. a pane resize drag-move) has
    /// changed workspace state without writing to disk. Drained by the next
    /// non-transient mutation or on app quit, so the user's last resize ratio
    /// always lands in the database eventually without thrashing the disk.
    pending_persist: bool,
}

impl KosmosApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        ui::tabs::file_tree::FileTreeUi::install(window, cx);
        SettingsInputs::install(window, cx);
        let workspaces = persistence::load();
        let file_tree = Self::create_file_tree(cx);
        let mut app = Self {
            workspaces,
            tab_scrolls: TabScrollHandles::new(),
            file_tree,
            focus_handle: cx.focus_handle(),
            workspace_watch_task: None,
            terminal_cwd_persist_task: None,
            pending_persist: false,
        };
        app.sync_file_tree_root(cx);
        app.start_workspace_watch_task(cx);
        app.start_terminal_cwd_persist_task(cx);
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
        cx.on_app_quit(|this, cx| {
            // Flush workspace state on quit so live terminal cwd changes land
            // on disk even if no pane-tree mutation happened after `cd`.
            this.persist_all_workspaces(cx);
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
                                terminal::TerminalStore::drop_workspace(id, cx);
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

    fn start_terminal_cwd_persist_task(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(TERMINAL_CWD_PERSIST_INTERVAL)
                    .await;
                if this
                    .update(cx, |this, cx| this.persist_changed_terminal_directories(cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        self.terminal_cwd_persist_task = Some(task);
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
        self.focus_handle.focus(window, cx);
        cx.observe_window_bounds(window, |_, window, _| {
            persistence::save_window_bounds(window.window_bounds());
        })
        .detach();
    }

    pub(crate) fn persist_active_workspace(&mut self, cx: &mut App) {
        if let Some(workspace_id) = self.workspaces.active_id() {
            self.persist_workspace(workspace_id, cx);
        } else {
            self.pending_persist = false;
        }
    }

    pub(crate) fn persist_workspace(&mut self, workspace_id: usize, cx: &mut App) {
        self.sync_terminal_paths_for_workspace(workspace_id, cx);
        if let Some(workspace) = self.workspaces.workspace(workspace_id) {
            persistence::save_workspace(workspace);
        }
        if self.workspaces.active_id() == Some(workspace_id) {
            self.pending_persist = false;
        }
    }

    pub(crate) fn persist_all_workspaces(&mut self, cx: &mut App) {
        let workspace_ids = self
            .workspaces
            .workspaces()
            .iter()
            .map(|workspace| workspace.id)
            .collect::<Vec<_>>();
        for workspace_id in workspace_ids {
            self.persist_workspace(workspace_id, cx);
        }
        persistence::save_session(&self.workspaces);
        self.pending_persist = false;
    }

    fn persist_changed_terminal_directories(&mut self, cx: &mut App) {
        let workspace_ids = self
            .workspaces
            .workspaces()
            .iter()
            .map(|workspace| workspace.id)
            .collect::<Vec<_>>();
        for workspace_id in workspace_ids {
            if !self.sync_terminal_paths_for_workspace(workspace_id, cx) {
                continue;
            }
            if let Some(workspace) = self.workspaces.workspace(workspace_id) {
                persistence::save_workspace(workspace);
            }
            if self.workspaces.active_id() == Some(workspace_id) {
                self.pending_persist = false;
            }
        }
    }

    fn sync_terminal_paths_for_workspace(&mut self, workspace_id: usize, cx: &mut App) -> bool {
        let tab_ids = self
            .workspaces
            .pane_tree(workspace_id)
            .map(|tree| terminal_tab_ids(tree.root()))
            .unwrap_or_default();
        let updates = tab_ids
            .into_iter()
            .filter_map(|tab_id| {
                let key = terminal::TerminalKey::new(workspace_id, tab_id);
                let cwd = terminal::TerminalStore::cwd(key, cx)?;
                (cwd.is_absolute() && cwd.is_dir()).then_some((tab_id, cwd))
            })
            .collect::<Vec<_>>();
        if updates.is_empty() {
            return false;
        }
        let mut changed = false;
        if let Some(tree) = self.workspaces.pane_tree_mut(workspace_id) {
            for (tab_id, cwd) in updates {
                changed |= tree.set_tab_path(tab_id, Some(cwd));
            }
        }
        changed
    }

    /// Write any deferred state to disk if a transient mutation left a
    /// pending change behind. No-op when the cache is clean.
    pub(crate) fn flush_pending_persist(&mut self, cx: &mut App) {
        if self.pending_persist {
            self.persist_active_workspace(cx);
        }
    }

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        cx.update_global::<SettingsUiState, _>(|state, _| {
            if state.open_dropdown.take().is_some() {
                changed = true;
            }
        });
        cx.update_global::<ui::tabs::terminal::TerminalUi, _>(|state, _| {
            if state.close_shell_picker() {
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
        self.persist_active_workspace(cx);
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
        availability.can_undo = true;
        availability.can_redo = true;
        availability.can_cut = true;
        availability.can_copy = true;
        availability.can_paste = true;
        availability.can_select_all = true;

        if let Some(path) = tab.path.as_deref() {
            availability.active_editor_dirty = BufferStore::is_path_dirty(path, cx);
        }

        availability
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
        match action {
            HeaderMenuAction::Undo => window.dispatch_action(Box::new(component_input::Undo), cx),
            HeaderMenuAction::Redo => window.dispatch_action(Box::new(component_input::Redo), cx),
            HeaderMenuAction::Cut => window.dispatch_action(Box::new(component_input::Cut), cx),
            HeaderMenuAction::Copy => window.dispatch_action(Box::new(component_input::Copy), cx),
            HeaderMenuAction::Paste => window.dispatch_action(Box::new(component_input::Paste), cx),
            HeaderMenuAction::SelectAll => {
                window.dispatch_action(Box::new(component_input::SelectAll), cx)
            }
            HeaderMenuAction::OpenFolder
            | HeaderMenuAction::Save
            | HeaderMenuAction::SaveAll
            | HeaderMenuAction::ExpandSelection
            | HeaderMenuAction::ShrinkSelection => {}
        }
        cx.notify();
    }

    fn save_active_file(&mut self, _: &file_editor::Save, _: &mut Window, cx: &mut Context<Self>) {
        self.save_active_editor(cx);
    }
}

impl Render for KosmosApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                &self.workspaces,
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
            .child(ui::tabs::git::render_modal_overlay(window, cx))
            .child(render_component_layers(window, cx))
    }
}

fn render_component_layers(window: &mut Window, cx: &mut App) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .children(gpui_component::Root::render_sheet_layer(window, cx))
        .children(gpui_component::Root::render_dialog_layer(window, cx))
        .children(gpui_component::Root::render_notification_layer(window, cx))
}

fn terminal_tab_ids(node: &PaneNode) -> Vec<usize> {
    match node {
        PaneNode::Leaf(pane) => pane
            .tabs()
            .iter()
            .filter(|tab| tab.kind.as_str() == tabs::registry::TERMINAL.id)
            .map(|tab| tab.id)
            .collect(),
        PaneNode::Split { first, second, .. } => {
            let mut ids = terminal_tab_ids(first);
            ids.extend(terminal_tab_ids(second));
            ids
        }
    }
}

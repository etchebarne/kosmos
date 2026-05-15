use std::path::{Path, PathBuf};

use gpui::{BorrowAppContext, Context, PathPromptOptions, Pixels, Point};

use pane_tree::{DropZone, PaneTree};
use settings::{SettingValue, Settings};
use tabs::Tab;
use ui::delegate::{
    HeaderDelegate, HeaderMenu, PaneDelegate, SettingsDelegate, SettingsUiState, TabScrollHandles,
    WorkspaceDelegate, WorkspaceMenuState,
};
use ui::drag::TabDrag;
use ui::pane_tree_actions::PaneTreeActionDelegate;

use crate::app::KosmosApp;

fn scroll_tabs_to_end(tab_scrolls: &TabScrollHandles, pane_id: usize, tab_count: usize) {
    if tab_count == 0 {
        return;
    }
    // The scrollable strip is: tab, divider, tab, ..., tab, plus-button —
    // `n` tabs + `n - 1` dividers + 1 plus button = `2 * n` children.
    // Scroll to the plus button so the new active tab is visible too.
    tab_scrolls.scroll_to_index(pane_id, 2 * tab_count - 1);
}

fn file_editor_tab(tab_id: usize, path: PathBuf) -> Tab {
    let title = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Tab::new(tab_id, &tabs::registry::FILE_EDITOR)
        .with_title(title)
        .with_path(path)
}

fn is_file_editor_tab(tab: &Tab, path: &Path) -> bool {
    tab.kind.as_str() == tabs::registry::FILE_EDITOR.id && tab.path.as_deref() == Some(path)
}

fn tab_count(tree: &PaneTree, pane_id: usize) -> usize {
    tree.pane(pane_id).map(|p| p.tabs().len()).unwrap_or(0)
}

impl HeaderDelegate for KosmosApp {
    fn toggle_header_menu(&mut self, menu: HeaderMenu, cx: &mut Context<Self>) {
        self.active_menu = if self.active_menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }
}

impl WorkspaceDelegate for KosmosApp {
    fn open_workspace_picker(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Workspace".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.workspaces.add(path);
                this.sync_file_tree_root(cx);
                cx.notify();
                this.persist_active_workspace();
                persistence::save_session(&this.workspaces);
            });
        })
        .detach();
    }

    fn select_workspace(&mut self, id: usize, cx: &mut Context<Self>) {
        if self.workspaces.select(id) {
            self.sync_file_tree_root(cx);
            cx.notify();
            persistence::save_session(&self.workspaces);
        }
    }

    fn move_workspace_before(&mut self, drag_id: usize, target_id: usize, cx: &mut Context<Self>) {
        if self.workspaces.reorder_before(drag_id, target_id) {
            cx.notify();
            persistence::save_session(&self.workspaces);
        }
    }

    fn move_workspace_to_end(&mut self, drag_id: usize, cx: &mut Context<Self>) {
        if self.workspaces.move_to_end(drag_id) {
            cx.notify();
            persistence::save_session(&self.workspaces);
        }
    }

    fn open_workspace_menu(&mut self, id: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.workspace_menu = Some(WorkspaceMenuState { id, position });
        cx.notify();
    }

    fn close_workspace_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace_menu.take().is_some() {
            cx.notify();
        }
    }

    fn close_workspace(&mut self, id: usize, cx: &mut Context<Self>) {
        if !self.workspaces.close(id) {
            return;
        }
        self.sync_file_tree_root(cx);
        cx.notify();
        persistence::save_session(&self.workspaces);
    }
}

impl PaneDelegate for KosmosApp {
    fn focus_pane(&mut self, pane_id: usize, _cx: &mut Context<Self>) {
        if let Some(tree) = self.workspaces.active_pane_tree_mut() {
            tree.focus_pane(pane_id);
        }
    }

    fn add_tab(&mut self, pane_id: usize, kind_id: &'static str, cx: &mut Context<Self>) {
        let Some(kind) = tabs::registry::get(kind_id) else {
            return;
        };
        let mut new_count: Option<usize> = None;
        self.mutate_active_tree(cx, |tree| {
            if !tree.add_tab(pane_id, kind) {
                return false;
            }
            new_count = tree.active_pane().map(|p| p.tabs().len());
            true
        });
        if let Some(count) = new_count {
            scroll_tabs_to_end(&self.tab_scrolls, pane_id, count);
        }
    }

    fn replace_tab_kind(
        &mut self,
        pane_id: usize,
        tab_id: usize,
        kind_id: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = tabs::registry::get(kind_id) else {
            return;
        };
        self.mutate_active_tree(cx, |tree| tree.replace_tab_kind(pane_id, tab_id, kind));
    }

    fn select_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| tree.select_tab(pane_id, tab_id));
    }

    fn close_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        let mut closed = false;
        self.mutate_active_tree(cx, |tree| {
            if tree.close_tab(pane_id, tab_id) {
                closed = true;
                true
            } else {
                false
            }
        });
        if closed {
            file_editor::EditorViewStore::drop_tab(tab_id, cx);
        }
    }

    fn move_tab_before(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        target_tab_id: usize,
        cx: &mut Context<Self>,
    ) {
        self.mutate_active_tree(cx, |tree| {
            tree.move_tab_before(drag.source_pane_id, drag.id, target_pane_id, target_tab_id)
        });
    }

    fn move_tab_to_pane(&mut self, drag: TabDrag, target_pane_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| {
            tree.move_tab_to_pane(drag.source_pane_id, drag.id, target_pane_id)
        });
    }

    fn move_tab_to_end(&mut self, drag: TabDrag, target_pane_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| {
            tree.move_tab_to_end(drag.source_pane_id, drag.id, target_pane_id)
        });
    }

    fn split_pane(
        &mut self,
        drag: TabDrag,
        target_pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    ) {
        self.mutate_active_tree(cx, |tree| {
            tree.split_pane(drag.source_pane_id, drag.id, target_pane_id, drop_zone)
        });
    }

    fn resize_split(&mut self, split_id: usize, ratio: f32, cx: &mut Context<Self>) {
        self.mutate_active_tree_transient(cx, |tree| tree.resize_split(split_id, ratio));
    }

    fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let mut opened: Option<(usize, usize)> = None;
        self.mutate_active_tree(cx, |tree| {
            if let Some((pane_id, tab_id)) = tree.find_tab(|tab| is_file_editor_tab(tab, &path)) {
                if !tree.select_tab(pane_id, tab_id) {
                    return false;
                }
                opened = Some((pane_id, tab_count(tree, pane_id)));
                return true;
            }

            let pane_id = tree.biggest_pane_id();
            let path = path.clone();
            opened = tree.append_new_tab(pane_id, |id| file_editor_tab(id, path));
            opened.is_some()
        });
        if let Some((pane_id, count)) = opened {
            scroll_tabs_to_end(&self.tab_scrolls, pane_id, count);
        }
    }

    fn open_file_in_pane(&mut self, path: PathBuf, target_pane_id: usize, cx: &mut Context<Self>) {
        let mut opened: Option<(usize, usize)> = None;
        self.mutate_active_tree(cx, |tree| {
            let existing = tree.pane(target_pane_id).and_then(|pane| {
                pane.tabs()
                    .iter()
                    .find(|tab| is_file_editor_tab(tab, &path))
                    .map(|tab| tab.id)
            });
            if let Some(tab_id) = existing {
                if !tree.select_tab(target_pane_id, tab_id) {
                    return false;
                }
                opened = Some((target_pane_id, tab_count(tree, target_pane_id)));
                return true;
            }

            let path = path.clone();
            opened = tree.append_new_tab(target_pane_id, |id| file_editor_tab(id, path));
            opened.is_some()
        });
        if let Some((pane_id, count)) = opened {
            scroll_tabs_to_end(&self.tab_scrolls, pane_id, count);
        }
    }

    fn open_file_before(
        &mut self,
        path: PathBuf,
        target_pane_id: usize,
        target_tab_id: usize,
        cx: &mut Context<Self>,
    ) {
        let mut opened: Option<(usize, usize)> = None;
        self.mutate_active_tree(cx, |tree| {
            let Some(pane) = tree.pane(target_pane_id) else {
                return false;
            };
            if !pane.has_tab(target_tab_id) {
                return false;
            }
            let existing = pane
                .tabs()
                .iter()
                .find(|tab| is_file_editor_tab(tab, &path))
                .map(|tab| tab.id);
            if let Some(tab_id) = existing {
                if !tree.select_tab(target_pane_id, tab_id) {
                    return false;
                }
                opened = Some((target_pane_id, tab_count(tree, target_pane_id)));
                return true;
            }

            let path = path.clone();
            opened = tree.insert_new_tab_before(target_pane_id, target_tab_id, |id| {
                file_editor_tab(id, path)
            });
            opened.is_some()
        });
        if let Some((pane_id, count)) = opened {
            scroll_tabs_to_end(&self.tab_scrolls, pane_id, count);
        }
    }

    fn split_pane_with_file(
        &mut self,
        path: PathBuf,
        target_pane_id: usize,
        drop_zone: DropZone,
        cx: &mut Context<Self>,
    ) {
        let mut opened: Option<(usize, usize)> = None;
        self.mutate_active_tree(cx, |tree| {
            let path = path.clone();
            opened = tree
                .split_pane_with_new_tab(target_pane_id, drop_zone, |id| file_editor_tab(id, path));
            opened.is_some()
        });
        if let Some((pane_id, count)) = opened {
            scroll_tabs_to_end(&self.tab_scrolls, pane_id, count);
        }
    }
}

impl SettingsDelegate for KosmosApp {
    fn select_settings_category(&mut self, category_id: &'static str, cx: &mut Context<Self>) {
        let mut changed = false;
        cx.update_global::<SettingsUiState, _>(|state, _| {
            if state.active_category != category_id {
                state.active_category = category_id;
                state.open_dropdown = None;
                changed = true;
            }
        });
        if changed {
            cx.notify();
        }
    }

    fn toggle_settings_dropdown(&mut self, setting_id: &'static str, cx: &mut Context<Self>) {
        cx.update_global::<SettingsUiState, _>(|state, _| {
            state.open_dropdown = if state.open_dropdown == Some(setting_id) {
                None
            } else {
                Some(setting_id)
            };
        });
        cx.notify();
    }

    fn set_setting_value(
        &mut self,
        key: &'static str,
        value: SettingValue,
        cx: &mut Context<Self>,
    ) {
        cx.update_global::<Settings, _>(|settings, _| {
            settings.set(key, value);
        });
        cx.notify();
    }

    fn install_tool(&mut self, entry: &'static registry::RegistryEntry, cx: &mut Context<Self>) {
        let tool_id = entry.id;
        let already = cx.global::<SettingsUiState>().installing.contains(tool_id);
        if already {
            return;
        }
        cx.update_global::<SettingsUiState, _>(|state, _| {
            state.installing.insert(tool_id);
            state.install_errors.remove(tool_id);
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { installer::ensure(entry) })
                .await;
            let _ = this.update(cx, |_, cx| {
                cx.update_global::<SettingsUiState, _>(|state, _| {
                    state.installing.remove(tool_id);
                    if let Err(err) = &result {
                        state
                            .install_errors
                            .insert(tool_id, format!("{err}").into());
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn uninstall_tool(&mut self, entry: &'static registry::RegistryEntry, cx: &mut Context<Self>) {
        let dir = installer::tool_dir(entry);
        let tool_id = entry.id;
        let result = std::fs::remove_dir_all(&dir);
        cx.update_global::<SettingsUiState, _>(|state, _| {
            state.install_errors.remove(tool_id);
            if let Err(err) = result
                && err.kind() != std::io::ErrorKind::NotFound
            {
                state
                    .install_errors
                    .insert(tool_id, format!("uninstall failed: {err}").into());
            }
        });
        cx.notify();
    }
}

impl PaneTreeActionDelegate for KosmosApp {
    fn with_active_tree(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut PaneTree) -> bool) {
        self.mutate_active_tree(cx, f);
    }

    fn on_tab_appended(&mut self, pane_id: usize, new_tab_count: usize, _cx: &mut Context<Self>) {
        scroll_tabs_to_end(&self.tab_scrolls, pane_id, new_tab_count);
    }
}

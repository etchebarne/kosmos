use std::path::PathBuf;

use gpui::Context;
use pane_tree::DropZone;
use ui::delegate::PaneDelegate;
use ui::drag::TabDrag;

use crate::app::KosmosApp;

use super::{
    anchor_tabs_to_end_during_open_animation, file_editor_tab, is_file_editor_tab,
    scroll_tabs_to_end, tab_count, terminal_tab, terminal_tab_key,
};

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
        let mut new_tab_id: Option<usize> = None;
        let terminal_cwd = self.workspaces.active_workspace().map(|w| w.path.clone());
        self.mutate_active_tree(cx, |tree| {
            let tab_id = tree.next_tab_id();
            let added = if kind_id == tabs::registry::TERMINAL.id {
                let Some(cwd) = terminal_cwd.clone() else {
                    return false;
                };
                tree.append_new_tab(pane_id, |id| terminal_tab(id, cwd))
                    .is_some()
            } else {
                tree.add_tab(pane_id, kind)
            };
            if !added {
                return false;
            }
            new_tab_id = Some(tab_id);
            new_count = tree.active_pane().map(|p| p.tabs().len());
            true
        });
        if let Some(tab_id) = new_tab_id {
            self.start_tab_open_animation(pane_id, tab_id, cx);
        }
        if let Some(count) = new_count {
            anchor_tabs_to_end_during_open_animation(&self.tab_scrolls, pane_id, count, cx);
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
        let old_terminal_key = (kind_id != tabs::registry::TERMINAL.id)
            .then(|| terminal_tab_key(&self.workspaces, pane_id, tab_id))
            .flatten();
        let terminal_cwd = self.workspaces.active_workspace().map(|w| w.path.clone());
        self.mutate_active_tree(cx, |tree| {
            if !tree.replace_tab_kind(pane_id, tab_id, kind) {
                return false;
            }
            if kind_id == tabs::registry::TERMINAL.id
                && let Some(cwd) = terminal_cwd.clone()
            {
                tree.set_tab_path(tab_id, Some(cwd));
            }
            true
        });
        if let Some(key) = old_terminal_key {
            terminal::TerminalStore::drop_tab(key, cx);
        }
    }

    fn select_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        self.mutate_active_tree(cx, |tree| tree.select_tab(pane_id, tab_id));
    }

    fn close_tab(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) {
        self.start_tab_close_animation(pane_id, tab_id, cx);
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

    fn finish_resize_split(&mut self, cx: &mut Context<Self>) {
        self.flush_pending_persist(cx);
    }

    fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let mut opened: Option<(usize, usize)> = None;
        let mut new_tab: Option<(usize, usize)> = None;
        self.mutate_active_tree(cx, |tree| {
            if let Some((pane_id, tab_id)) = tree.find_tab(|tab| is_file_editor_tab(tab, &path)) {
                if !tree.select_tab(pane_id, tab_id) {
                    return false;
                }
                opened = Some((pane_id, tab_count(tree, pane_id)));
                return true;
            }

            let pane_id = tree.biggest_pane_id();
            let tab_id = tree.next_tab_id();
            let path = path.clone();
            opened = tree.append_new_tab(pane_id, |id| file_editor_tab(id, path));
            if opened.is_some() {
                new_tab = Some((pane_id, tab_id));
            }
            opened.is_some()
        });
        if let Some((pane_id, tab_id)) = new_tab {
            self.start_tab_open_animation(pane_id, tab_id, cx);
        }
        if let Some((pane_id, count)) = opened {
            scroll_tabs_to_end(&self.tab_scrolls, pane_id, count);
        }
    }

    fn open_file_in_pane(&mut self, path: PathBuf, target_pane_id: usize, cx: &mut Context<Self>) {
        let mut opened: Option<(usize, usize)> = None;
        let mut new_tab: Option<(usize, usize)> = None;
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

            let tab_id = tree.next_tab_id();
            let path = path.clone();
            opened = tree.append_new_tab(target_pane_id, |id| file_editor_tab(id, path));
            if opened.is_some() {
                new_tab = Some((target_pane_id, tab_id));
            }
            opened.is_some()
        });
        if let Some((pane_id, tab_id)) = new_tab {
            self.start_tab_open_animation(pane_id, tab_id, cx);
        }
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
        let mut new_tab: Option<(usize, usize)> = None;
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

            let tab_id = tree.next_tab_id();
            let path = path.clone();
            opened = tree.insert_new_tab_before(target_pane_id, target_tab_id, |id| {
                file_editor_tab(id, path)
            });
            if opened.is_some() {
                new_tab = Some((target_pane_id, tab_id));
            }
            opened.is_some()
        });
        if let Some((pane_id, tab_id)) = new_tab {
            self.start_tab_open_animation(pane_id, tab_id, cx);
        }
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
        let mut new_tab: Option<(usize, usize)> = None;
        self.mutate_active_tree(cx, |tree| {
            let tab_id = tree.next_tab_id();
            let path = path.clone();
            opened = tree
                .split_pane_with_new_tab(target_pane_id, drop_zone, |id| file_editor_tab(id, path));
            if let Some((pane_id, _)) = opened {
                new_tab = Some((pane_id, tab_id));
            }
            opened.is_some()
        });
        if let Some((pane_id, tab_id)) = new_tab {
            self.start_tab_open_animation(pane_id, tab_id, cx);
        }
        if let Some((pane_id, count)) = opened {
            scroll_tabs_to_end(&self.tab_scrolls, pane_id, count);
        }
    }
}

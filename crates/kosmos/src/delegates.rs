use gpui::{Context, PathPromptOptions};

use pane_tree::{DropZone, PaneTree, PaneTreeContext};
use ui::delegate::{HeaderDelegate, HeaderMenu, PaneDelegate, WorkspaceDelegate};
use ui::drag::TabDrag;

use crate::app::KosmosApp;

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
                cx.notify();
                this.persist_active_workspace();
                persistence::save_session(&this.workspaces);
            });
        })
        .detach();
    }

    fn select_workspace(&mut self, id: usize, cx: &mut Context<Self>) {
        if self.workspaces.select(id) {
            cx.notify();
            persistence::save_session(&self.workspaces);
        }
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
        self.mutate_active_tree(cx, |tree| tree.add_tab(pane_id, kind));
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
        self.mutate_active_tree(cx, |tree| tree.close_tab(pane_id, tab_id));
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
        self.mutate_active_tree(cx, |tree| tree.resize_split(split_id, ratio));
    }
}

impl PaneTreeContext for KosmosApp {
    fn with_active_tree(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut PaneTree) -> bool,
    ) {
        self.mutate_active_tree(cx, f);
    }
}

use gpui::{Context, PathPromptOptions};
use ui::delegate::WorkspaceDelegate;

use crate::app::KosmosApp;

impl WorkspaceDelegate for KosmosApp {
    fn open_workspace_picker(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.sync_active_file_tree_expanded_dirs(cx);
                this.workspaces.add(path);
                this.sync_file_tree_root(cx);
                cx.notify();
                this.persist_active_workspace(cx);
                persistence::save_session(&this.workspaces);
            });
        })
        .detach();
    }

    fn select_workspace(&mut self, id: usize, cx: &mut Context<Self>) {
        self.sync_active_file_tree_expanded_dirs(cx);
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

    fn close_workspace(&mut self, id: usize, cx: &mut Context<Self>) {
        self.sync_active_file_tree_expanded_dirs(cx);
        if !self.workspaces.close(id) {
            return;
        }
        terminal::TerminalStore::drop_workspace(id, cx);
        self.sync_file_tree_root(cx);
        cx.notify();
        persistence::save_session(&self.workspaces);
    }
}

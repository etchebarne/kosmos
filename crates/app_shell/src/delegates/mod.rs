mod header;
mod pane;
mod pane_tree_actions;
mod settings;
mod workspace;

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::Context;
use pane_tree::PaneTree;
use tabs::Tab;
use ui::delegate::{TabAnimationState, TabScrollHandles};
use ui::metrics::TAB_ANIMATION_DURATION_MS;

use crate::app::KosmosApp;

impl KosmosApp {
    pub(crate) fn start_tab_open_animation(
        &mut self,
        pane_id: usize,
        tab_id: usize,
        cx: &mut Context<Self>,
    ) {
        if !cx
            .default_global::<TabAnimationState>()
            .start_opening(pane_id, tab_id)
        {
            return;
        }

        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(TAB_ANIMATION_DURATION_MS))
                .await;
            let _ = this.update(cx, move |_, cx| {
                if cx
                    .default_global::<TabAnimationState>()
                    .finish_opening(pane_id, tab_id)
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn start_tab_close_animation(
        &mut self,
        pane_id: usize,
        tab_id: usize,
        cx: &mut Context<Self>,
    ) {
        let can_close = self.workspaces.active_pane_tree().is_some_and(|tree| {
            tree.total_tabs() > 1 && tree.pane(pane_id).is_some_and(|pane| pane.has_tab(tab_id))
        });
        if !can_close {
            return;
        }

        if !cx
            .default_global::<TabAnimationState>()
            .start_closing(pane_id, tab_id)
        {
            return;
        }

        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(TAB_ANIMATION_DURATION_MS))
                .await;
            let _ = this.update(cx, move |this, cx| {
                let should_close = cx
                    .default_global::<TabAnimationState>()
                    .finish_closing(pane_id, tab_id);
                if should_close && !this.finish_tab_close(pane_id, tab_id, cx) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn finish_tab_close(&mut self, pane_id: usize, tab_id: usize, cx: &mut Context<Self>) -> bool {
        let mut closed = false;
        self.mutate_active_tree(cx, |tree| {
            closed = tree.close_tab(pane_id, tab_id);
            closed
        });
        if closed {
            file_editor::EditorViewStore::drop_tab(tab_id, cx);
        }
        closed
    }
}

fn scroll_tabs_to_end(tab_scrolls: &TabScrollHandles, pane_id: usize, tab_count: usize) {
    if tab_count == 0 {
        return;
    }
    // The scrollable strip is: tab, divider, tab, ..., tab, plus-button:
    // `n` tabs + `n - 1` dividers + 1 plus button = `2 * n` children.
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

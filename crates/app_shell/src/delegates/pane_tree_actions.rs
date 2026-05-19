use gpui::Context;
use pane_tree::PaneTree;
use ui::pane_tree_actions::PaneTreeActionDelegate;

use crate::app::KosmosApp;

use super::scroll_tabs_to_end;

impl PaneTreeActionDelegate for KosmosApp {
    fn with_active_tree(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut PaneTree) -> bool) {
        self.mutate_active_tree(cx, f);
    }

    fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        let tab = self.workspaces.active_pane_tree().and_then(|tree| {
            let pane_id = tree.active_pane_id();
            let tab_id = tree.active_pane()?.active_tab();
            Some((pane_id, tab_id))
        });
        if let Some((pane_id, tab_id)) = tab {
            self.start_tab_close_animation(pane_id, tab_id, cx);
        }
    }

    fn on_tab_appended(
        &mut self,
        pane_id: usize,
        tab_id: usize,
        new_tab_count: usize,
        cx: &mut Context<Self>,
    ) {
        self.start_tab_open_animation(pane_id, tab_id, cx);
        scroll_tabs_to_end(&self.tab_scrolls, pane_id, new_tab_count);
    }
}

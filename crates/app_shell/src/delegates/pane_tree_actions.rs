use gpui::Context;
use pane_tree::PaneTree;
use ui::pane_tree_actions::PaneTreeActionDelegate;

use crate::app::KosmosApp;

use super::scroll_tabs_to_end;

impl PaneTreeActionDelegate for KosmosApp {
    fn with_active_tree(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut PaneTree) -> bool) {
        self.mutate_active_tree(cx, f);
    }

    fn on_tab_appended(&mut self, pane_id: usize, new_tab_count: usize, _cx: &mut Context<Self>) {
        scroll_tabs_to_end(&self.tab_scrolls, pane_id, new_tab_count);
    }
}

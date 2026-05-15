use gpui::{Context, InteractiveElement, actions};
use pane_tree::PaneTree;
use tabs::{Tab, registry};

actions!(pane_tree, [CloseTab, NewTab]);

pub trait PaneTreeActionDelegate: Sized + 'static {
    fn with_active_tree(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut PaneTree) -> bool);

    fn on_tab_appended(&mut self, _pane_id: usize, _new_tab_count: usize, _cx: &mut Context<Self>) {
    }
}

pub trait WirePaneTreeActions: Sized {
    fn wire_pane_tree_actions<T: PaneTreeActionDelegate>(self, cx: &mut Context<T>) -> Self;
}

impl<E: InteractiveElement + 'static> WirePaneTreeActions for E {
    fn wire_pane_tree_actions<T: PaneTreeActionDelegate>(self, cx: &mut Context<T>) -> Self {
        self.on_action(cx.listener(|this, _: &CloseTab, _, cx| {
            this.with_active_tree(cx, |tree| tree.close_active_tab());
        }))
        .on_action(cx.listener(|this, _: &NewTab, _, cx| {
            let mut appended = None;
            this.with_active_tree(cx, |tree| {
                let pane_id = tree.active_pane_id();
                let result = tree.append_new_tab(pane_id, |id| Tab::new(id, &registry::BLANK));
                appended = result;
                result.is_some()
            });
            if let Some((pane_id, count)) = appended {
                this.on_tab_appended(pane_id, count, cx);
            }
        }))
    }
}

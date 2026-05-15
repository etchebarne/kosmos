mod header;
mod pane;
mod pane_tree_actions;
mod settings;
mod workspace;

use std::path::{Path, PathBuf};

use pane_tree::PaneTree;
use tabs::Tab;
use ui::delegate::TabScrollHandles;

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

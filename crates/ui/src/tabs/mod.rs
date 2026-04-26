mod blank;
mod file_editor;
mod file_search;
mod file_tree;
mod git;
mod placeholder;
mod settings;
mod terminal;

use gpui::{AnyElement, Context, IntoElement, div};

use tabs::Tab;

use crate::delegate::PaneDelegate;

pub fn render<T: PaneDelegate>(pane_id: usize, tab: &Tab, cx: &mut Context<T>) -> AnyElement {
    match tab.kind.as_ref() {
        "blank" => blank::render(pane_id, tab.id, cx),
        "terminal" => terminal::render(cx),
        "file_tree" => file_tree::render(cx),
        "file_search" => file_search::render(cx),
        "git" => git::render(cx),
        "settings" => settings::render(cx),
        "file_editor" => file_editor::render(tab, cx),
        _ => div().into_any_element(),
    }
}

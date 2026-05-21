use std::path::PathBuf;

use gpui::{App, Entity, Focusable, Window};

use file_tree::FileTree;

use crate::tabs::file_tree::state::ActiveFileTreeUi;

pub fn begin_rename(
    path: PathBuf,
    file_tree: &Entity<FileTree>,
    window: &mut Window,
    cx: &mut App,
) {
    let original = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    file_tree.update(cx, |tree, cx| {
        tree.start_rename(path, cx);
    });

    if let Some(input) = cx.file_tree_ui().map(|ui| ui.input()) {
        input.update(cx, |input, cx| {
            input.set_value(original, cx);
        });
        input.focus_handle(cx).focus(window, cx);
    }
}

pub fn focus_new_entry_input(window: &mut Window, cx: &mut App) {
    if let Some(input) = cx.file_tree_ui().map(|ui| ui.input()) {
        input.update(cx, |input, cx| {
            input.set_value("", cx);
        });
        input.focus_handle(cx).focus(window, cx);
    }
}

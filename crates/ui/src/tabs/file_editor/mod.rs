include!("parts/actions_and_elements.rs");
include!("parts/render.rs");
include!("parts/tests.rs");

pub(super) fn drop_tab(tab_id: usize, cx: &mut gpui::App) {
    ComponentEditorStore::drop_tab(tab_id, cx);
}

pub(super) fn request_reveal(
    path: std::path::PathBuf,
    line: usize,
    column: usize,
    cx: &mut gpui::App,
) {
    ComponentEditorStore::request_reveal(path, line, column, cx);
}

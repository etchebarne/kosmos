pub fn indent_guides(depth: usize, theme: Theme) -> AnyElement {
    if depth == 0 {
        return div().flex_none().into_any_element();
    }
    let mut row = div().flex().flex_none().h(ROW_HEIGHT);
    for _ in 0..depth {
        row = row.child(
            div()
                .relative()
                .w(rems(INDENT_REM))
                .h(ROW_HEIGHT)
                .flex_none()
                .child(
                    div()
                        .absolute()
                        .left(rems(GUIDE_OFFSET_REM))
                        .top_0()
                        .bottom_0()
                        .w(rems(GUIDE_WIDTH_REM))
                        .bg(gpui::Hsla::from(theme.text).opacity(0.1)),
                ),
        );
    }
    row.into_any_element()
}

/// Build the path list for a drag started on `row_path`. If the row belongs to
/// the active multi-selection, the drag carries every selected path; otherwise
/// it carries just the row's own path.
fn drag_paths_for<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    row_path: &Path,
    cx: &Context<T>,
) -> Vec<PathBuf> {
    let tree = entity.read(cx);
    if tree.is_selected(row_path) && tree.selected_count() > 1 {
        tree.selected_paths().iter().cloned().collect()
    } else {
        vec![row_path.to_path_buf()]
    }
}

/// Drop predicate shared by directory and file rows: allow the drop if at
/// least one source can land in `dest_dir` (i.e. dest is not inside any
/// source, and at least one source is not already a direct child).
fn can_drop_into_dir(drag: &FileNodeDrag, dest_dir: &Path) -> bool {
    if drag.paths.is_empty() {
        return false;
    }
    if drag.paths.iter().any(|p| dest_dir.starts_with(p)) {
        return false;
    }
    drag.paths.iter().any(|p| p.parent() != Some(dest_dir))
}

pub fn path_id(prefix: &'static str, path: &Path) -> gpui::ElementId {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    let hash = hasher.finish() as usize;
    gpui::ElementId::from((prefix, hash))
}

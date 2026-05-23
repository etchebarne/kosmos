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

pub fn path_id(prefix: &'static str, path: &Path) -> gpui::ElementId {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    let hash = hasher.finish() as usize;
    gpui::ElementId::from((prefix, hash))
}

fn change_list<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    summary: &RepositorySummary,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let tree = build_change_tree(&summary.files);
    div()
        .id("git-change-list")
        .flex_1()
        .min_h_0()
        .bg(theme.bg_surface)
        .overflow_y_scroll()
        .when(summary.files.is_empty(), |this| {
            this.flex().items_center().justify_center()
        })
        .when(!summary.files.is_empty(), |this| {
            this.child(
                div()
                    .flex_none()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .text_xs()
                    .text_color(theme.text_subtle)
                    .child("TRACKED"),
            )
        })
        .when(summary.files.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(theme.text_subtle)
                    .child("No changes"),
            )
        })
        .children(
            tree.dirs
                .into_values()
                .map(|node| change_dir_row(root.clone(), node, 0, true, cx)),
        )
        .children(
            tree.files
                .into_iter()
                .map(|change| change_file_row(root.clone(), change, 0, cx)),
        )
        .into_any_element()
}

#[derive(Default)]
struct ChangeTreeNode {
    name: String,
    path: String,
    dirs: std::collections::BTreeMap<String, ChangeTreeNode>,
    files: Vec<FileChange>,
}

fn build_change_tree(files: &[FileChange]) -> ChangeTreeNode {
    let mut root = ChangeTreeNode::default();
    for change in files {
        let mut parts = change.path.split('/').collect::<Vec<_>>();
        let Some(file_name) = parts.pop() else {
            continue;
        };
        let mut node = &mut root;
        let mut path = String::new();
        for part in parts {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            node = node
                .dirs
                .entry(part.to_string())
                .or_insert_with(|| ChangeTreeNode {
                    name: part.to_string(),
                    path: path.clone(),
                    ..Default::default()
                });
        }
        let mut file = change.clone();
        file.path = if node.path.is_empty() {
            file_name.to_string()
        } else {
            format!("{}/{}", node.path, file_name)
        };
        node.files.push(file);
    }
    root
}

fn change_dir_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    mut node: ChangeTreeNode,
    depth: usize,
    keep_separate: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let mut label = node.name.clone();
    while !keep_separate && node.files.is_empty() && node.dirs.len() == 1 {
        let (_, child) = node.dirs.into_iter().next().unwrap();
        label = format!("{label}/{}", child.name);
        node = child;
    }
    let stats = node_stats(&node);
    let path = node.path.clone();
    let is_expanded = !cx
        .global::<GitUiState>()
        .collapsed_change_dirs
        .contains(&path);
    let toggle_path = path.clone();

    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .h(rems(CHANGE_ROW_HEIGHT_REM))
                .px(rems(CHANGE_ROW_PADDING_REM))
                .hover(move |this| this.bg(theme.bg_hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _, _, cx| {
                        toggle_change_dir(&toggle_path, cx);
                    }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .child(change_indent_guides(depth, theme))
                        .child(
                            div()
                                .w(rems(CHANGE_ICON_WIDTH_REM))
                                .h(rems(CHANGE_ROW_HEIGHT_REM))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(if is_expanded {
                                        IconName::FolderOpened
                                    } else {
                                        IconName::Folder
                                    })
                                    .size(14.0)
                                    .color(theme.text_muted),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .pl(rems(CHANGE_LABEL_PADDING_REM))
                                .text_sm()
                                .text_color(theme.text)
                                .child(label),
                        ),
                )
                .child(stage_checkbox(
                    SharedString::from(format!("git-folder-toggle:{path}")),
                    stats.staged == stats.total,
                    root.clone(),
                    path,
                    cx,
                )),
        )
        .when(is_expanded, |this| {
            this.children(
                node.dirs
                    .into_values()
                    .map(|child| change_dir_row(root.clone(), child, depth + 1, false, cx)),
            )
            .children(
                node.files
                    .into_iter()
                    .map(|change| change_file_row(root.clone(), change, depth + 1, cx)),
            )
        })
        .into_any_element()
}

fn change_file_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    change: FileChange,
    depth: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let name = change
        .path
        .rsplit_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| change.path.clone());
    let icon_name = icon_for_git_file(Path::new(&change.path));
    let icon_color = match change.kind {
        FileChangeKind::Created => rgb(0x22c55e),
        FileChangeKind::Modified => theme.text_muted,
        FileChangeKind::Deleted => theme.danger,
        FileChangeKind::Renamed => rgb(0xa855f7),
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .h(rems(CHANGE_ROW_HEIGHT_REM))
        .px(rems(CHANGE_ROW_PADDING_REM))
        .hover(move |this| this.bg(theme.bg_hover))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .child(change_indent_guides(depth, theme))
                .child(
                    div()
                        .w(rems(CHANGE_ICON_WIDTH_REM))
                        .h(rems(CHANGE_ROW_HEIGHT_REM))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon_name).size(14.0).color(icon_color)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .pl(rems(CHANGE_LABEL_PADDING_REM))
                        .text_sm()
                        .text_color(if change.kind == FileChangeKind::Deleted {
                            theme.text_subtle
                        } else {
                            theme.text
                        })
                        .child(name),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_3()
                .child(change_stats(&change, theme))
                .child(stage_checkbox(
                    SharedString::from(format!("git-file-toggle:{}", change.path)),
                    change.staged,
                    root,
                    change.path,
                    cx,
                )),
        )
        .into_any_element()
}

#[derive(Default)]
struct ChangeNodeStats {
    total: usize,
    staged: usize,
}

fn node_stats(node: &ChangeTreeNode) -> ChangeNodeStats {
    let mut stats = ChangeNodeStats::default();
    for file in &node.files {
        stats.total += 1;
        if file.staged {
            stats.staged += 1;
        }
    }
    for child in node.dirs.values() {
        let child_stats = node_stats(child);
        stats.total += child_stats.total;
        stats.staged += child_stats.staged;
    }
    stats
}

fn change_indent_guides(depth: usize, theme: theme::Theme) -> AnyElement {
    if depth == 0 {
        return div().flex_none().into_any_element();
    }

    let mut row = div().flex().flex_none().h(rems(CHANGE_ROW_HEIGHT_REM));
    for _ in 0..depth {
        row = row.child(
            div()
                .relative()
                .w(rems(CHANGE_INDENT_REM))
                .h(rems(CHANGE_ROW_HEIGHT_REM))
                .flex_none()
                .child(
                    div()
                        .absolute()
                        .left(rems(CHANGE_GUIDE_OFFSET_REM))
                        .top_0()
                        .bottom_0()
                        .w(rems(CHANGE_GUIDE_WIDTH_REM))
                        .bg(gpui::Hsla::from(theme.text).opacity(0.1)),
                ),
        );
    }
    row.into_any_element()
}

fn icon_for_git_file(path: &Path) -> IconName {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(icon) = IconName::for_file_name(name)
    {
        return icon;
    }

    language::from_path(path)
        .and_then(|id| IconName::for_language(id.as_str()))
        .unwrap_or(IconName::File)
}

fn change_stats(change: &FileChange, theme: theme::Theme) -> AnyElement {
    let added = rgb(0x22c55e);
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_sm()
        .when(change.insertions > 0, |this| {
            this.child(
                div()
                    .text_color(added)
                    .child(format!("+{}", change.insertions)),
            )
        })
        .when(change.deletions > 0, |this| {
            this.child(
                div()
                    .text_color(theme.danger)
                    .child(format!("-{}", change.deletions)),
            )
        })
        .when(change.insertions == 0 && change.deletions == 0, |this| {
            this.child(div().text_color(theme.text_subtle).child("0"))
        })
        .into_any_element()
}


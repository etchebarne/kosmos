fn change_list<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    summary: &RepositorySummary,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let change_tree = build_change_tree(&summary.files);
    let Some(tree_state) = cx.global::<GitUiState>().change_tree_state.clone() else {
        return div().into_any_element();
    };

    let items = change_tree_items(&change_tree, cx);
    tree_state.update(cx, |state, cx| state.set_items(items, cx));

    let file_changes = summary
        .files
        .iter()
        .cloned()
        .map(|change| (change.path.clone(), change))
        .collect::<std::collections::BTreeMap<_, _>>();
    let root_path = root.to_path_buf();

    div()
        .id("git-change-list")
        .relative()
        .flex_1()
        .min_h_0()
        .bg(theme.bg_surface)
        .child(
            div()
                .id("git-change-list-scroll")
                .size_full()
                .min_h_0()
                .flex()
                .flex_col()
                .when(summary.files.is_empty(), |this| {
                    this.flex().flex_col().items_center().justify_center()
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
                .when(!summary.files.is_empty(), |this| {
                    this.child(
                        div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(
                            component_tree(&tree_state, move |ix, entry, _, _, cx| {
                                change_tree_row(
                                    ix,
                                    entry,
                                    root_path.clone(),
                                    file_changes.clone(),
                                    cx,
                                )
                            })
                            .size_full(),
                        ),
                    )
                }),
        )
        .into_any_element()
}

const CHANGE_DIR_PREFIX: &str = "dir:";
const CHANGE_FILE_PREFIX: &str = "file:";

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
        let (dir_path, file_name) = change
            .path
            .rsplit_once('/')
            .unwrap_or(("", change.path.as_str()));
        let mut node = &mut root;
        let mut current_path = String::new();
        if !dir_path.is_empty() {
            for dir_name in dir_path.split('/') {
                if !current_path.is_empty() {
                    current_path.push('/');
                }
                current_path.push_str(dir_name);
                node = node
                    .dirs
                    .entry(dir_name.to_string())
                    .or_insert_with(|| ChangeTreeNode {
                        name: dir_name.to_string(),
                        path: current_path.clone(),
                        ..Default::default()
                    });
            }
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

fn change_tree_items<T: PaneDelegate + SettingsDelegate>(
    node: &ChangeTreeNode,
    cx: &mut Context<T>,
) -> Vec<TreeItem> {
    let mut items = Vec::new();
    items.extend(
        node.dirs
            .values()
            .map(|child| change_dir_tree_item(child, true, cx)),
    );
    items.extend(node.files.iter().map(change_file_tree_item));
    items
}

fn change_dir_tree_item<T: PaneDelegate + SettingsDelegate>(
    node: &ChangeTreeNode,
    keep_separate: bool,
    cx: &mut Context<T>,
) -> TreeItem {
    let mut label = node.name.clone();
    let mut display_node = node;
    while !keep_separate && display_node.files.is_empty() && display_node.dirs.len() == 1 {
        let child = display_node.dirs.values().next().unwrap();
        label = format!("{label}/{}", child.name);
        display_node = child;
    }

    let is_expanded = !cx
        .global::<GitUiState>()
        .collapsed_change_dirs
        .contains(&display_node.path);
    let children = display_node
        .dirs
        .values()
        .map(|child| change_dir_tree_item(child, false, cx))
        .chain(display_node.files.iter().map(change_file_tree_item));

    TreeItem::new(format!("{CHANGE_DIR_PREFIX}{}", display_node.path), label)
        .expanded(is_expanded)
        .children(children)
}

fn change_file_tree_item(change: &FileChange) -> TreeItem {
    TreeItem::new(
        format!("{CHANGE_FILE_PREFIX}{}", change.path),
        change
            .path
            .rsplit_once('/')
            .map(|(_, name)| name.to_string())
            .unwrap_or_else(|| change.path.clone()),
    )
}

fn change_tree_row(
    ix: usize,
    entry: &TreeEntry,
    root: PathBuf,
    file_changes: std::collections::BTreeMap<String, FileChange>,
    cx: &mut App,
) -> ListItem {
    let item = entry.item();
    let id = item.id.as_str();
    if let Some(path) = id.strip_prefix(CHANGE_DIR_PREFIX) {
        let path = path.to_string();
        return change_tree_dir_row(ix, entry, root, path, cx);
    }

    let path = id.strip_prefix(CHANGE_FILE_PREFIX).unwrap_or(id);
    let change = file_changes.get(path).cloned();
    change_tree_file_row(ix, entry, root, change, cx)
}

fn change_tree_dir_row(
    ix: usize,
    entry: &TreeEntry,
    root: PathBuf,
    path: String,
    cx: &mut App,
) -> ListItem {
    let stats = cx
        .global::<GitUiState>()
        .summary
        .as_ref()
        .map(|summary| node_stats_for_path(&summary.files, &path))
        .unwrap_or_default();
    let icon = if entry.is_expanded() {
        IconName::FolderOpened
    } else {
        IconName::Folder
    };
    let toggle_path = path.clone();

    ListItem::new(ix)
        .h(rems(CHANGE_ROW_HEIGHT_REM))
        .px(rems(CHANGE_ROW_PADDING_REM))
        .py_0()
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .child(change_row_label(
                    entry.depth(),
                    icon,
                    entry.item().label.clone(),
                    cx,
                ))
                .child(stage_checkbox_app(
                    SharedString::from(format!("git-folder-toggle:{path}")),
                    stats.staged == stats.total,
                    stats.conflict_paths.clone(),
                    root.clone(),
                    path.clone(),
                    cx,
                )),
            )
        .on_click(move |_, _, cx| toggle_change_dir_app(&toggle_path, cx))
}

fn change_tree_file_row(
    ix: usize,
    entry: &TreeEntry,
    root: PathBuf,
    change: Option<FileChange>,
    cx: &mut App,
) -> ListItem {
    let theme = *cx.theme();
    let path = change
        .as_ref()
        .map(|change| change.path.clone())
        .unwrap_or_else(|| entry.item().label.to_string());
    let icon_name = icon_for_git_file(Path::new(&path));
    let icon_color = change
        .as_ref()
        .map(|change| match change.kind {
            FileChangeKind::Created => theme.success,
            FileChangeKind::Modified => theme.text_muted,
            FileChangeKind::Deleted => theme.danger,
            FileChangeKind::Renamed => theme.accent_secondary,
            FileChangeKind::Conflicted => theme.accent_secondary,
        })
        .unwrap_or(theme.text_muted);

    ListItem::new(ix)
        .h(rems(CHANGE_ROW_HEIGHT_REM))
        .px(rems(CHANGE_ROW_PADDING_REM))
        .py_0()
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .child(change_row_label_with_color(
                    entry.depth(),
                    icon_name,
                    icon_color.into(),
                    entry.item().label.clone(),
                    cx,
                ))
                .when_some(change, |this, change| {
                    this.child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_1()
                            .child(file_diff_stats(&change, cx))
                            .child(stage_checkbox_app(
                                SharedString::from(format!("git-file-toggle:{}", change.path)),
                                change.staged,
                                (change.kind == FileChangeKind::Conflicted)
                                    .then(|| vec![change.path.clone()])
                                    .unwrap_or_default(),
                                root.clone(),
                                change.path.clone(),
                                cx,
                            )),
                    )
                }),
        )
}

fn file_diff_stats(change: &FileChange, cx: &mut App) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .child(diff_stat_text(format!("+{}", change.insertions), theme.success))
        .child(diff_stat_text(format!("-{}", change.deletions), theme.danger))
        .into_any_element()
}

#[derive(Default)]
struct ChangeNodeStats {
    staged: usize,
    total: usize,
    conflict_paths: Vec<String>,
}

fn node_stats_for_path(files: &[FileChange], path: &str) -> ChangeNodeStats {
    let prefix = format!("{path}/");
    files
        .iter()
        .filter(|file| file.path.starts_with(&prefix))
        .fold(ChangeNodeStats::default(), |mut stats, file| {
            stats.total += 1;
            if file.staged {
                stats.staged += 1;
            }
            if file.kind == FileChangeKind::Conflicted {
                stats.conflict_paths.push(file.path.clone());
            }
            stats
        })
}

fn change_row_label(
    depth: usize,
    icon_name: IconName,
    label: SharedString,
    cx: &mut App,
) -> AnyElement {
    let theme = *cx.theme();
    change_row_label_with_color(depth, icon_name, theme.text_muted, label, cx)
}

fn change_row_label_with_color(
    depth: usize,
    icon_name: IconName,
    icon_color: gpui::Rgba,
    label: SharedString,
    cx: &mut App,
) -> AnyElement {
    let theme = *cx.theme();
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
                .child(Icon::new(icon_name).size_rem(0.875).color(icon_color)),
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
        )
        .into_any_element()
}

fn stage_checkbox_app(
    id: SharedString,
    staged: bool,
    conflict_paths: Vec<String>,
    root: PathBuf,
    path: String,
    _cx: &mut App,
) -> AnyElement {
    Checkbox::new(id)
        .large()
        .flex_none()
        .tab_stop(false)
        .checked(staged)
        .on_click(move |_: &bool, _, cx| {
            cx.stop_propagation();
            let path = path.clone();
            if staged {
                run_git_action_app(
                    root.clone(),
                    move |root| kosmos_git::unstage_file(root, &path),
                    cx,
                );
            } else if !conflict_paths.is_empty() {
                open_resolve_conflicts_modal_app(root.clone(), conflict_paths.clone(), false, cx);
            } else {
                run_git_action_app(
                    root.clone(),
                    move |root| kosmos_git::stage_file(root, &path),
                    cx,
                );
            }
        })
        .into_any_element()
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

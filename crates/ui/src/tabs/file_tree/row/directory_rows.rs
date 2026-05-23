use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, App, Entity, IntoElement, KeyDownEvent, MouseButton, Rgba, SharedString, Window,
    div, prelude::*, rems,
};
use gpui_component::{
    Icon as ComponentIcon, Sizable,
    button::{Button, ButtonVariants},
    list::ListItem,
    tree::TreeEntry,
};

use file_tree::{FileTree, NewEntryDraft, NodeKind};
use icons::{Icon, IconName};
use theme::{ActiveTheme, Theme};

use crate::delegate::{PaneDelegate, SettingsDelegate};
use crate::tabs::file_tree::drag::FileNodeDrag;
use crate::tabs::file_tree::state::{ActiveFileTreeUi, FileTreeUi, PendingFileTreeDrop};

pub const ROW_HEIGHT: gpui::Rems = gpui::Rems(1.625);
pub const INDENT_REM: f32 = 1.25;
const GUIDE_OFFSET_REM: f32 = 0.625;
const GUIDE_WIDTH_REM: f32 = 0.0625;
const ICON_WIDTH_REM: f32 = 1.25;

#[derive(Clone, Copy, Default)]
pub struct RowState {
    pub is_expanded: bool,
    pub is_selected: bool,
    pub is_renaming: bool,
}

pub fn render_tree_entry<T: PaneDelegate + SettingsDelegate>(
    ix: usize,
    entry: &TreeEntry,
    entity: &Entity<FileTree>,
    delegate: &Entity<T>,
    new_entry: &Option<NewEntryDraft>,
    _window: &mut Window,
    cx: &mut App,
) -> ListItem {
    let item = entry.item();
    let id = item.id.as_str();
    if id.starts_with(super::FILE_TREE_NEW_PREFIX) {
        if let Some(draft) = new_entry.as_ref() {
            return render_new_entry_item(ix, draft, entity, entry.depth(), cx);
        }
        return ListItem::new(ix).disabled(true);
    }

    let (path, kind) = if let Some(path) = id.strip_prefix(super::FILE_TREE_DIR_PREFIX) {
        (PathBuf::from(path), NodeKind::Directory)
    } else if let Some(path) = id.strip_prefix(super::FILE_TREE_FILE_PREFIX) {
        (PathBuf::from(path), NodeKind::File)
    } else {
        (PathBuf::from(id), NodeKind::File)
    };
    let state = compute_row_state_app(entity, &path, cx);
    let icon_name = match kind {
        NodeKind::Directory if state.is_expanded => IconName::FolderOpened,
        NodeKind::Directory => IconName::Folder,
        NodeKind::File => icon_for_file(&path),
    };
    if state.is_renaming {
        return ListItem::new(ix)
            .h(ROW_HEIGHT)
            .px(rems(0.375))
            .py_0()
            .disabled(true)
            .child(rename_input_body_app(entity, entry.depth(), icon_name, cx));
    }

    let theme = *cx.theme();
    let path_for_click = path.clone();
    let path_for_drag = path.clone();
    let name_for_drag = item.label.clone();
    let entity_for_click = entity.clone();
    let delegate_for_click = delegate.clone();
    let is_root = kind == NodeKind::Directory && entry.depth() == 0;
    let entity_for_suffix = entity.clone();

    let body = draggable_node_body(
        entity,
        &path_for_drag,
        entry.depth(),
        icon_name,
        name_for_drag.clone(),
        state.is_selected,
        kind,
        theme,
        cx,
    );

    ListItem::new(ix)
        .h(ROW_HEIGHT)
        .px(rems(0.375))
        .py_0()
        .when(state.is_selected, |item| item.bg(theme.bg_hover))
        .text_color(if state.is_selected {
            theme.text_emphasis
        } else if kind == NodeKind::Directory && entry.depth() == 0 {
            theme.text_header
        } else {
            theme.text
        })
        .child(body)
        .on_click(move |event, _window, cx| {
            let target = path_for_click.clone();
            let shift = event.modifiers().shift;
            entity_for_click.update(cx, |tree, cx| {
                if shift {
                    tree.extend_selection_to(target.clone(), cx);
                } else {
                    tree.select(target.clone(), cx);
                    if kind == NodeKind::Directory {
                        tree.toggle_expand(&target, cx);
                    }
                }
            });
            if !shift && kind == NodeKind::File {
                delegate_for_click.update(cx, |delegate, cx| delegate.open_file(target, cx));
            }
        })
        .suffix(move |window, cx| {
            if is_root {
                root_actions_app::<T>(&entity_for_suffix, window, cx)
            } else {
                div().into_any_element()
            }
        })
}

fn root_actions_app<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    _window: &mut Window,
    _cx: &mut App,
) -> AnyElement {
    let new_file_entity = entity.clone();
    let new_dir_entity = entity.clone();
    let refresh_entity = entity.clone();
    let collapse_entity = entity.clone();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(root_action_button(
            "ft-action-new-file",
            IconName::FileAdd,
            move |window, cx| {
                let anchor = new_file_entity
                    .read(cx)
                    .selected()
                    .map(|path| path.to_path_buf());
                new_file_entity.update(cx, |tree, cx| {
                    tree.start_new_entry(anchor.as_deref(), NodeKind::File, cx);
                });
                super::actions::focus_new_entry_input(window, cx);
            },
        ))
        .child(root_action_button(
            "ft-action-new-folder",
            IconName::FolderAdd,
            move |window, cx| {
                let anchor = new_dir_entity
                    .read(cx)
                    .selected()
                    .map(|path| path.to_path_buf());
                new_dir_entity.update(cx, |tree, cx| {
                    tree.start_new_entry(anchor.as_deref(), NodeKind::Directory, cx);
                });
                super::actions::focus_new_entry_input(window, cx);
            },
        ))
        .child(root_action_button(
            "ft-action-refresh",
            IconName::Refresh,
            move |_, cx| {
                refresh_entity.update(cx, |tree, cx| {
                    if let Some(root) = tree.root().map(Path::to_path_buf) {
                        tree.reload_dir(&root);
                    }
                    cx.notify();
                });
            },
        ))
        .child(root_action_button(
            "ft-action-collapse-all",
            IconName::CollapseAll,
            move |_, cx| {
                collapse_entity.update(cx, |tree, cx| tree.collapse_all(cx));
            },
        ))
        .into_any_element()
}

fn root_action_button(
    id: &'static str,
    icon: IconName,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    Button::new(id)
        .ghost()
        .tab_stop(false)
        .size(rems(1.375))
        .icon(ComponentIcon::empty().path(icon.path()).small())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_click(window, cx);
        })
        .into_any_element()
}

fn compute_row_state_app(entity: &Entity<FileTree>, path: &Path, cx: &App) -> RowState {
    let tree = entity.read(cx);
    RowState {
        is_expanded: tree.is_expanded(path),
        is_selected: tree.is_selected(path),
        is_renaming: tree
            .rename_target()
            .is_some_and(|rename| rename.path.as_path() == path),
    }
}

fn draggable_node_body(
    entity: &Entity<FileTree>,
    path: &Path,
    depth: usize,
    icon_name: IconName,
    name: SharedString,
    is_selected: bool,
    kind: NodeKind,
    theme: Theme,
    cx: &mut App,
) -> AnyElement {
    let icon_color = if is_selected {
        theme.text
    } else {
        theme.text_muted
    };
    let drag_paths = drag_paths_for_app(entity, path, cx);
    let drop_entity = entity.clone();
    let pending_drop_entity = entity.clone();
    let drop_destination = match kind {
        NodeKind::Directory => Some(path.to_path_buf()),
        NodeKind::File => path.parent().map(Path::to_path_buf),
    };
    let pending_drop_destination = drop_destination.clone();
    let drag_overlay_id = path_id("file-tree-node-drag", path);
    div()
        .id(path_id("file-tree-node-body", path))
        .relative()
        .w_full()
        .h(ROW_HEIGHT)
        .flex()
        .items_center()
        .child(node_label(
            depth,
            icon_name,
            name.clone(),
            is_selected,
            theme,
        ))
        .child(
            div()
                .id(drag_overlay_id)
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .right_0()
                .drag_over::<FileNodeDrag>(move |style, _, _, _| {
                    style.bg(gpui::Hsla::from(theme.accent).opacity(0.18))
                })
                .on_drag_move(move |event: &gpui::DragMoveEvent<FileNodeDrag>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let Some(destination) = pending_drop_destination.clone() else {
                        cx.update_global::<FileTreeUi, _>(|ui, _| ui.clear_pending_drop());
                        return;
                    };
                    let drag = event.drag(cx);
                    if !can_drop_into_dir_app(drag, &destination) {
                        cx.update_global::<FileTreeUi, _>(|ui, _| ui.clear_pending_drop());
                        return;
                    }
                    let paths = drag.paths.clone();
                    cx.update_global::<FileTreeUi, _>(|ui, _| {
                        ui.set_pending_drop(PendingFileTreeDrop {
                            tree: pending_drop_entity.clone(),
                            paths,
                            destination,
                            bounds: event.bounds,
                        });
                    });
                })
                .on_drop(move |drag: &FileNodeDrag, _, cx| {
                    cx.stop_propagation();
                    cx.update_global::<FileTreeUi, _>(|ui, _| ui.clear_pending_drop());
                    let Some(destination) = drop_destination.clone() else {
                        return;
                    };
                    if !can_drop_into_dir_app(drag, &destination) {
                        return;
                    }
                    let paths = drag.paths.clone();
                    drop_entity.update(cx, |tree, cx| tree.move_into(paths, destination, cx));
                })
                .on_drag(
                    FileNodeDrag::new(drag_paths, name, icon_name, kind),
                    |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
                ),
        )
        .child(div().hidden().child(file_tree_icon(icon_name, icon_color)))
        .into_any_element()
}

fn drag_paths_for_app(entity: &Entity<FileTree>, row_path: &Path, cx: &App) -> Vec<PathBuf> {
    let tree = entity.read(cx);
    if tree.is_selected(row_path) && tree.selected_count() > 1 {
        tree.selected_paths().iter().cloned().collect()
    } else {
        vec![row_path.to_path_buf()]
    }
}

fn can_drop_into_dir_app(drag: &FileNodeDrag, dest_dir: &Path) -> bool {
    if drag.paths.is_empty() {
        return false;
    }
    if drag.paths.iter().any(|path| dest_dir.starts_with(path)) {
        return false;
    }
    drag.paths
        .iter()
        .any(|path| path.parent() != Some(dest_dir))
}

fn render_new_entry_item(
    ix: usize,
    draft: &NewEntryDraft,
    entity: &Entity<FileTree>,
    depth: usize,
    cx: &mut App,
) -> ListItem {
    let icon_name = match draft.kind {
        NodeKind::Directory => IconName::Folder,
        NodeKind::File => IconName::File,
    };
    ListItem::new(ix)
        .h(ROW_HEIGHT)
        .px(rems(0.375))
        .py_0()
        .disabled(true)
        .child(new_entry_input_body_app(entity, depth, icon_name, cx))
}

fn new_entry_input_body_app(
    entity: &Entity<FileTree>,
    depth: usize,
    icon_name: IconName,
    cx: &mut App,
) -> AnyElement {
    let theme = *cx.theme();
    let Some(input) = cx.file_tree_ui().map(|ui| ui.input()) else {
        return div().into_any_element();
    };
    let entity_submit = entity.clone();
    let entity_cancel = entity.clone();
    let input_for_submit = input.clone();

    div()
        .flex()
        .items_center()
        .h(ROW_HEIGHT)
        .w_full()
        .child(indent_guides(depth, theme))
        .child(
            div()
                .w(rems(ICON_WIDTH_REM))
                .h(ROW_HEIGHT)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(file_tree_icon(icon_name, theme.text_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .capture_key_down(move |event: &KeyDownEvent, _: &mut Window, cx: &mut App| {
                    match event.keystroke.key.as_str() {
                        "enter" => {
                            cx.stop_propagation();
                            let value = input_for_submit.read(cx).value().to_string();
                            entity_submit.update(cx, |tree, cx| tree.apply_new_entry(value, cx));
                        }
                        "escape" => {
                            cx.stop_propagation();
                            entity_cancel.update(cx, |tree, cx| tree.cancel_new_entry(cx));
                        }
                        _ => {}
                    }
                })
                .child(Input::new(&input)),
        )
        .into_any_element()
}

fn rename_input_body_app(
    entity: &Entity<FileTree>,
    depth: usize,
    icon_name: IconName,
    cx: &mut App,
) -> AnyElement {
    let theme = *cx.theme();
    let Some(input) = cx.file_tree_ui().map(|ui| ui.input()) else {
        return div().into_any_element();
    };
    let entity_submit = entity.clone();
    let entity_cancel = entity.clone();
    let input_for_submit = input.clone();

    div()
        .flex()
        .items_center()
        .h(ROW_HEIGHT)
        .w_full()
        .child(indent_guides(depth, theme))
        .child(
            div()
                .w(rems(ICON_WIDTH_REM))
                .h(ROW_HEIGHT)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(file_tree_icon(icon_name, theme.text_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .capture_key_down(move |event: &KeyDownEvent, _: &mut Window, cx: &mut App| {
                    match event.keystroke.key.as_str() {
                        "enter" => {
                            cx.stop_propagation();
                            let value = input_for_submit.read(cx).value().to_string();
                            entity_submit.update(cx, |tree, cx| tree.apply_rename(value, cx));
                        }
                        "escape" => {
                            cx.stop_propagation();
                            entity_cancel.update(cx, |tree, cx| tree.cancel_rename(cx));
                        }
                        _ => {}
                    }
                })
                .child(Input::new(&input)),
        )
        .into_any_element()
}

fn file_tree_icon(icon: IconName, color: Rgba) -> Icon {
    Icon::new(icon).size_rem(0.875).color(color)
}

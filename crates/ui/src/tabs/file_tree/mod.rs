mod actions;
mod drag;
mod menu;
mod row;
mod state;

pub use actions::begin_rename;
pub use state::FileTreeUi;

use std::path::Path;

use gpui::{
    AnyElement, ClickEvent, Context, Entity, IntoElement, MouseButton, SharedString, div,
    prelude::*, rems,
};

use file_tree::{ActiveFileTree, FileTree, NewEntryDraft, NodeKind};
use icons::{Icon, IconName};
use tabs::registry;
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, SettingsDelegate};
use crate::tabs::file_tree::state::ActiveFileTreeUi;

pub fn render<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let entity = match cx.file_tree().cloned() {
        Some(e) => e,
        None => return empty_state(cx),
    };

    let (root, error, new_entry, context_menu, clipboard, root_expanded) = {
        let tree = entity.read(cx);
        let Some(root) = tree.root().map(Path::to_path_buf) else {
            return empty_state(cx);
        };
        (
            root.clone(),
            tree.error().cloned(),
            tree.new_entry_draft().cloned(),
            tree.context_menu().cloned(),
            tree.clipboard().map(|(op, p)| (op, p.to_path_buf())),
            tree.is_expanded(&root),
        )
    };

    let root_label: SharedString = root
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| root.display().to_string())
        .into();

    let root_state = compute_row_state(&entity, &root, cx);
    let mut rows: Vec<AnyElement> = Vec::new();
    rows.push(row::render_root::<T>(
        &entity,
        root.clone(),
        root_label,
        root_state,
        actions_row::<T>(&entity, cx),
        cx,
    ));
    if root_expanded {
        collect_rows::<T>(&entity, &root, 0, &mut rows, &new_entry, cx);
    }

    let scroll_handle = cx
        .file_tree_ui()
        .map(|ui| ui.scroll())
        .unwrap_or_default();

    let entity_for_dismiss = entity.clone();

    let menu_overlay = context_menu.as_ref().map(|state| {
        let has_clipboard = clipboard.is_some();
        let cut_active = matches!(clipboard.as_ref(), Some((file_tree::ClipboardOp::Cut, _)));
        menu::render::<T>(
            &entity,
            state.target.clone(),
            state.position,
            has_clipboard,
            cut_active,
            cx,
        )
    });

    let dismiss_layer = context_menu.as_ref().map(|_| {
        div()
            .id("file-tree-menu-dismiss")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _, _, cx| {
                    let entity = entity_for_dismiss.clone();
                    entity.update(cx, |t, cx| t.close_context_menu(cx));
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let entity = entity.clone();
                    move |_, _, _, cx| {
                        let entity = entity.clone();
                        entity.update(cx, |t, cx| t.close_context_menu(cx));
                    }
                }),
            )
            .into_any_element()
    });

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .child(error_banner(error.clone(), &entity, cx))
        .child(
            div()
                .id("file-tree-scroll")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .track_scroll(&scroll_handle)
                .children(rows),
        )
        .when_some(dismiss_layer, |this, layer| this.child(layer))
        .when_some(menu_overlay, |this, menu| this.child(menu))
        .into_any_element()
}

fn collect_rows<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    dir: &Path,
    depth: usize,
    out: &mut Vec<AnyElement>,
    new_entry: &Option<NewEntryDraft>,
    cx: &mut Context<T>,
) {
    let snapshot = {
        let tree = entity.read(cx);
        let Some(children) = tree.children_of(dir).map(|c| c.to_vec()) else {
            return;
        };
        children
    };
    let new_here = match new_entry {
        Some(draft) if draft.parent.as_path() == dir => Some(draft.clone()),
        _ => None,
    };

    if let Some(draft) = new_here.as_ref().filter(|d| d.kind == NodeKind::Directory) {
        out.push(row::render_new_entry::<T>(draft, entity, depth + 1, cx));
    }

    for node in snapshot
        .iter()
        .filter(|n| matches!(n.kind, file_tree::NodeKind::Directory))
    {
        let row_state = compute_row_state(entity, &node.path, cx);
        out.push(row::render_dir::<T>(
            entity,
            node.path.clone(),
            node.name.clone(),
            depth + 1,
            row_state,
            cx,
        ));
        if row_state.is_expanded {
            collect_rows::<T>(entity, &node.path, depth + 1, out, new_entry, cx);
        }
    }

    if let Some(draft) = new_here.as_ref().filter(|d| d.kind == NodeKind::File) {
        out.push(row::render_new_entry::<T>(draft, entity, depth + 1, cx));
    }

    for node in snapshot
        .iter()
        .filter(|n| matches!(n.kind, file_tree::NodeKind::File))
    {
        let row_state = compute_row_state(entity, &node.path, cx);
        out.push(row::render_file::<T>(
            entity,
            node.path.clone(),
            node.name.clone(),
            depth + 1,
            row_state,
            cx,
        ));
    }
}

fn compute_row_state<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    path: &Path,
    cx: &Context<T>,
) -> row::RowState {
    let tree = entity.read(cx);
    row::RowState {
        is_expanded: tree.is_expanded(path),
        is_selected: tree.selected().is_some_and(|s| s == path),
        is_renaming: tree
            .rename_target()
            .is_some_and(|r| r.path.as_path() == path),
    }
}

/// Builds the four-button cluster that lives at the right edge of the root
/// row (new file, new folder, refresh, collapse all).
fn actions_row<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    cx: &mut Context<T>,
) -> AnyElement {
    let entity_new_file = entity.clone();
    let entity_new_dir = entity.clone();
    let entity_refresh = entity.clone();
    let entity_collapse = entity.clone();

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(action_button::<T>(
            "ft-action-new-file",
            IconName::FileAdd,
            "New File",
            cx.listener(move |_, _, window, cx| {
                let entity = entity_new_file.clone();
                let anchor = entity
                    .read(cx)
                    .selected()
                    .map(|p| p.to_path_buf());
                entity.update(cx, |t, cx| {
                    t.start_new_entry(anchor.as_deref(), NodeKind::File, cx);
                });
                actions::focus_new_entry_input(window, cx);
            }),
            cx,
        ))
        .child(action_button::<T>(
            "ft-action-new-folder",
            IconName::FolderAdd,
            "New Folder",
            cx.listener(move |_, _, window, cx| {
                let entity = entity_new_dir.clone();
                let anchor = entity
                    .read(cx)
                    .selected()
                    .map(|p| p.to_path_buf());
                entity.update(cx, |t, cx| {
                    t.start_new_entry(anchor.as_deref(), NodeKind::Directory, cx);
                });
                actions::focus_new_entry_input(window, cx);
            }),
            cx,
        ))
        .child(action_button::<T>(
            "ft-action-refresh",
            IconName::Refresh,
            "Refresh",
            cx.listener(move |_, _, _, cx| {
                let entity = entity_refresh.clone();
                entity.update(cx, |t, cx| {
                    if let Some(root) = t.root().map(Path::to_path_buf) {
                        t.reload_dir(&root);
                        cx.notify();
                    }
                });
            }),
            cx,
        ))
        .child(action_button::<T>(
            "ft-action-collapse",
            IconName::CollapseAll,
            "Collapse All",
            cx.listener(move |_, _, _, cx| {
                let entity = entity_collapse.clone();
                entity.update(cx, |t, cx| t.collapse_all(cx));
            }),
            cx,
        ))
        .into_any_element()
}

fn action_button<T: PaneDelegate + SettingsDelegate>(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    listener: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let _ = tooltip;
    let _ = cx;
    div()
        .id(id)
        .size(rems(1.375))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.25))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        })
        .child(Icon::new(icon).size(14.0).color(theme.text_muted))
        .into_any_element()
}

fn error_banner<T: PaneDelegate + SettingsDelegate>(
    error: Option<SharedString>,
    entity: &Entity<FileTree>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let Some(message) = error else {
        return div().into_any_element();
    };
    let entity = entity.clone();
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .py_1()
        .bg(gpui::Hsla::from(theme.danger).opacity(0.15))
        .text_xs()
        .text_color(theme.text)
        .child(div().flex_1().min_w_0().child(message))
        .child(
            div()
                .id("ft-error-dismiss")
                .size(rems(1.25))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.25))
                .text_color(theme.text_muted)
                .hover(move |s| s.bg(theme.bg_hover))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let entity = entity.clone();
                    entity.update(cx, |t, _| t.clear_error());
                    cx.notify();
                }))
                .child(Icon::new(IconName::Close).size(12.0).color(theme.text_muted)),
        )
        .into_any_element()
}

fn empty_state<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(theme.text_subtle)
        .child(
            Icon::new(registry::FILE_TREE.icon)
                .size(28.0)
                .color(theme.text_muted),
        )
        .child(div().text_sm().child("No workspace open"))
        .into_any_element()
}

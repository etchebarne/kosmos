mod actions;
pub mod drag;
mod menu;
mod row;
mod state;

pub use actions::begin_rename;
pub use state::{ActiveFileTreeUi, FileTreeUi};

use std::path::Path;

use gpui::{AnyElement, Context, Entity, IntoElement, SharedString, Window, div, prelude::*};

use file_tree::{ActiveFileTree, FileTree, NewEntryDraft, NodeKind};
use gpui_component::{
    Icon as ComponentIcon, Sizable, Size,
    alert::Alert,
    tree::{TreeEntry, TreeItem, tree as component_tree},
};
use icons::IconName;
use tabs::registry;
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, SettingsDelegate};

pub fn render<T: PaneDelegate + SettingsDelegate>(
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    render_content(window, cx)
}

fn render_content<T: PaneDelegate + SettingsDelegate>(
    _window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let entity = match cx.file_tree().cloned() {
        Some(e) => e,
        None => return empty_state(cx),
    };

    let (root, error, new_entry) = {
        let tree = entity.read(cx);
        let Some(root) = tree.root().map(Path::to_path_buf) else {
            return empty_state(cx);
        };
        (
            root.clone(),
            tree.error().cloned(),
            tree.new_entry_draft().cloned(),
        )
    };

    let root_label: SharedString = root
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| root.display().to_string())
        .into();

    let Some(tree_state) = cx.file_tree_ui().map(|ui| ui.tree()) else {
        return empty_state(cx);
    };
    let root_item = build_tree_item(
        &entity,
        &root,
        root_label,
        NodeKind::Directory,
        &new_entry,
        cx,
    );
    tree_state.update(cx, |state, cx| state.set_items(vec![root_item], cx));
    let delegate = cx.entity().clone();
    let entity_for_rows = entity.clone();
    let new_entry_for_rows = new_entry.clone();

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .child(error_banner(error.clone(), &entity, cx))
        .child(
            div().relative().flex_1().min_h_0().child(
                div().id("file-tree-scroll").size_full().child(
                    component_tree(&tree_state, move |ix, entry, _, window, cx| {
                        row::render_tree_entry::<T>(
                            ix,
                            entry,
                            &entity_for_rows,
                            &delegate,
                            &new_entry_for_rows,
                            window,
                            cx,
                        )
                    })
                    .context_menu({
                        let entity = entity.clone();
                        move |_, entry, popup_menu, window, cx| {
                            let Some(path) = entry_path(entry) else {
                                return popup_menu;
                            };
                            menu::build(entity.clone(), path, popup_menu, window, cx)
                        }
                    })
                    .size_full(),
                ),
            ),
        )
        .into_any_element()
}

fn entry_path(entry: &TreeEntry) -> Option<std::path::PathBuf> {
    let id = entry.item().id.as_str();
    id.strip_prefix(FILE_TREE_DIR_PREFIX)
        .or_else(|| id.strip_prefix(FILE_TREE_FILE_PREFIX))
        .map(std::path::PathBuf::from)
}

pub(super) const FILE_TREE_DIR_PREFIX: &str = "dir:";
pub(super) const FILE_TREE_FILE_PREFIX: &str = "file:";
pub(super) const FILE_TREE_NEW_PREFIX: &str = "new:";

fn build_tree_item<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    path: &Path,
    name: SharedString,
    kind: NodeKind,
    new_entry: &Option<NewEntryDraft>,
    cx: &mut Context<T>,
) -> TreeItem {
    let state = compute_row_state(entity, path, cx);
    let id = match kind {
        NodeKind::Directory => format!("{FILE_TREE_DIR_PREFIX}{}", path.to_string_lossy()),
        NodeKind::File => format!("{FILE_TREE_FILE_PREFIX}{}", path.to_string_lossy()),
    };
    let mut item = TreeItem::new(id, name).expanded(state.is_expanded);
    if state.is_renaming {
        item = item.disabled(true);
    }
    if kind == NodeKind::File {
        return item;
    }

    let snapshot = {
        let tree = entity.read(cx);
        tree.children_of(path)
            .map(|c| c.to_vec())
            .unwrap_or_default()
    };
    let new_here = new_entry
        .as_ref()
        .filter(|draft| draft.parent.as_path() == path);
    let mut children = Vec::new();
    if let Some(draft) = new_here.filter(|draft| draft.kind == NodeKind::Directory) {
        children.push(new_entry_tree_item(draft));
    }
    children.extend(
        snapshot
            .iter()
            .filter(|n| matches!(n.kind, file_tree::NodeKind::Directory))
            .map(|node| {
                build_tree_item(
                    entity,
                    &node.path,
                    node.name.clone(),
                    node.kind,
                    new_entry,
                    cx,
                )
            }),
    );
    if let Some(draft) = new_here.filter(|draft| draft.kind == NodeKind::File) {
        children.push(new_entry_tree_item(draft));
    }
    children.extend(
        snapshot
            .iter()
            .filter(|n| matches!(n.kind, file_tree::NodeKind::File))
            .map(|node| {
                build_tree_item(
                    entity,
                    &node.path,
                    node.name.clone(),
                    node.kind,
                    new_entry,
                    cx,
                )
            }),
    );

    item.children(children)
}

fn new_entry_tree_item(draft: &NewEntryDraft) -> TreeItem {
    TreeItem::new(
        format!(
            "{FILE_TREE_NEW_PREFIX}{}:{}",
            match draft.kind {
                NodeKind::Directory => "dir",
                NodeKind::File => "file",
            },
            draft.parent.to_string_lossy()
        ),
        "",
    )
    .disabled(true)
}
fn compute_row_state<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    path: &Path,
    cx: &Context<T>,
) -> row::RowState {
    let tree = entity.read(cx);
    row::RowState {
        is_expanded: tree.is_expanded(path),
        is_selected: tree.is_selected(path),
        is_renaming: tree
            .rename_target()
            .is_some_and(|r| r.path.as_path() == path),
    }
}

fn error_banner<T: PaneDelegate + SettingsDelegate>(
    error: Option<SharedString>,
    entity: &Entity<FileTree>,
    _cx: &mut Context<T>,
) -> AnyElement {
    let Some(message) = error else {
        return div().into_any_element();
    };
    let entity = entity.clone();
    Alert::error("file-tree-error", message)
        .banner()
        .with_size(Size::Small)
        .icon(ComponentIcon::empty().path(IconName::Close.path()))
        .on_close(move |_, _, cx| {
            entity.update(cx, |tree, _| tree.clear_error());
            cx.refresh_windows();
        })
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
            ComponentIcon::empty()
                .path(super::icon_for_kind(registry::FILE_TREE.id).path())
                .text_color(theme.text_muted),
        )
        .child(div().text_sm().child("No workspace open"))
        .into_any_element()
}

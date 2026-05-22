use std::{path::PathBuf, rc::Rc};

use gpui::{App, ClickEvent, Context, Entity, Window, div, prelude::*, rems};
use gpui_component::{
    Icon as ComponentIcon, Sizable,
    menu::{PopupMenu, PopupMenuItem},
};

use file_tree::{ClipboardOp, FileTree, NodeKind};
use icons::IconName;
use theme::ActiveTheme;

use crate::tabs::file_tree::actions;

const MENU_WIDTH_REM: f32 = 13.0;

type FileTreeMenuHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub fn build<T: 'static>(
    entity: Entity<FileTree>,
    target: PathBuf,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<T>,
) -> PopupMenu {
    select_context_target(&entity, &target, cx);

    // Operations apply to the full multi-selection if the right-clicked target
    // belongs to it; otherwise they apply only to the target.
    let (op_paths, has_clipboard, cut_active) = {
        let tree = entity.read(cx);
        let op_paths = if tree.is_selected(&target) && tree.selected_count() > 1 {
            tree.selected_paths().iter().cloned().collect()
        } else {
            vec![target.clone()]
        };
        let clipboard = tree.clipboard().map(|(op, _)| op);
        (
            op_paths,
            clipboard.is_some(),
            matches!(clipboard, Some(ClipboardOp::Cut)),
        )
    };
    let multi = op_paths.len() > 1;
    let menu_width = rems(MENU_WIDTH_REM).to_pixels(window.rem_size());

    menu.min_w(menu_width)
        .item(menu_item(
            IconName::FileAdd,
            "New File",
            true,
            false,
            new_entry_handler(entity.clone(), target.clone(), NodeKind::File),
        ))
        .item(menu_item(
            IconName::FolderAdd,
            "New Folder",
            true,
            false,
            new_entry_handler(entity.clone(), target.clone(), NodeKind::Directory),
        ))
        .separator()
        .item(menu_item(
            IconName::Edit,
            if multi { "Cut Selection" } else { "Cut" },
            !op_paths.is_empty(),
            false,
            cut_handler(entity.clone(), op_paths.clone()),
        ))
        .item(menu_item(
            IconName::Copy,
            if multi { "Copy Selection" } else { "Copy" },
            !op_paths.is_empty(),
            false,
            copy_handler(entity.clone(), op_paths.clone()),
        ))
        .item(menu_item(
            IconName::Clippy,
            if cut_active { "Paste (move)" } else { "Paste" },
            has_clipboard,
            false,
            paste_handler(entity.clone(), target.clone()),
        ))
        .separator()
        .item(menu_item(
            IconName::Edit,
            "Rename",
            !multi,
            false,
            rename_handler(entity.clone(), target.clone()),
        ))
        .item(menu_item(
            IconName::Folder,
            "Reveal in File Explorer",
            true,
            false,
            reveal_handler(target.clone()),
        ))
        .separator()
        .item(menu_item(
            IconName::Trash,
            "Move to Trash",
            !op_paths.is_empty(),
            true,
            trash_handler(entity.clone(), op_paths.clone()),
        ))
        .item(menu_item(
            IconName::Close,
            "Delete Permanently",
            !op_paths.is_empty(),
            true,
            delete_handler(entity, op_paths),
        ))
}

fn select_context_target<T: 'static>(
    entity: &Entity<FileTree>,
    target: &PathBuf,
    cx: &mut Context<T>,
) {
    entity.update(cx, |tree, cx| {
        if !tree.is_selected(target) {
            tree.select(target.clone(), cx);
        }
    });
}

fn new_entry_handler(
    entity: Entity<FileTree>,
    target: PathBuf,
    kind: NodeKind,
) -> FileTreeMenuHandler {
    Rc::new(move |_, window, cx| {
        cx.stop_propagation();
        entity.update(cx, |tree, cx| {
            tree.start_new_entry(Some(&target), kind, cx);
        });
        actions::focus_new_entry_input(window, cx);
    })
}

fn cut_handler(entity: Entity<FileTree>, paths: Vec<PathBuf>) -> FileTreeMenuHandler {
    Rc::new(move |_, _, cx| {
        cx.stop_propagation();
        if !paths.is_empty() {
            entity.update(cx, |tree, cx| tree.cut(paths.clone(), cx));
        }
    })
}

fn copy_handler(entity: Entity<FileTree>, paths: Vec<PathBuf>) -> FileTreeMenuHandler {
    Rc::new(move |_, _, cx| {
        cx.stop_propagation();
        if !paths.is_empty() {
            entity.update(cx, |tree, cx| tree.copy(paths.clone(), cx));
        }
    })
}

fn paste_handler(entity: Entity<FileTree>, target: PathBuf) -> FileTreeMenuHandler {
    Rc::new(move |_, _, cx| {
        cx.stop_propagation();
        entity.update(cx, |tree, cx| tree.paste_into(target.clone(), cx));
    })
}

fn rename_handler(entity: Entity<FileTree>, target: PathBuf) -> FileTreeMenuHandler {
    Rc::new(move |_, window, cx| {
        cx.stop_propagation();
        actions::begin_rename(target.clone(), &entity, window, cx);
    })
}

fn reveal_handler(target: PathBuf) -> FileTreeMenuHandler {
    Rc::new(move |_, _, cx| {
        cx.stop_propagation();
        cx.reveal_path(&target);
    })
}

fn trash_handler(entity: Entity<FileTree>, paths: Vec<PathBuf>) -> FileTreeMenuHandler {
    Rc::new(move |_, _, cx| {
        cx.stop_propagation();
        if !paths.is_empty() {
            entity.update(cx, |tree, cx| tree.trash(paths.clone(), cx));
        }
    })
}

fn delete_handler(entity: Entity<FileTree>, paths: Vec<PathBuf>) -> FileTreeMenuHandler {
    Rc::new(move |_, _, cx| {
        cx.stop_propagation();
        if !paths.is_empty() {
            entity.update(cx, |tree, cx| tree.delete(paths.clone(), cx));
        }
    })
}

fn menu_item(
    icon: IconName,
    label: &'static str,
    enabled: bool,
    danger: bool,
    listener: FileTreeMenuHandler,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let theme = *cx.theme();
        let text_color = if !enabled {
            theme.text_subtle
        } else if danger {
            theme.danger
        } else {
            theme.text
        };
        let icon_color = if !enabled {
            theme.text_subtle
        } else if danger {
            theme.danger
        } else {
            theme.text_muted
        };

        div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .text_color(text_color)
            .child(
                div()
                    .w(rems(1.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(icon_color)
                    .child(ComponentIcon::empty().path(icon.path()).small()),
            )
            .child(label)
    })
    .disabled(!enabled)
    .on_click(move |event, window, cx| listener(event, window, cx))
}

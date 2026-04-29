use std::path::PathBuf;

use gpui::{
    AnyElement, App, Context, Entity, IntoElement, MouseButton, Pixels, Point, SharedString,
    Window, anchored, deferred, div, prelude::*, rems,
};

use file_tree::{ClipboardOp, FileTree, NodeKind};
use icons::{Icon, IconName};
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, SettingsDelegate};
use crate::tabs::file_tree::actions;

pub fn render<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    target: Option<PathBuf>,
    position: Point<Pixels>,
    has_clipboard: bool,
    cut_active: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let target_is_some = target.is_some();
    let target_for_paste = target.clone();

    // Operations apply to the full multi-selection if the right-clicked target
    // belongs to it; otherwise they apply only to the target.
    let op_paths: Vec<PathBuf> = {
        let tree = entity.read(cx);
        match &target {
            Some(t) if tree.is_selected(t) && tree.selected_count() > 1 => {
                tree.selected_paths().iter().cloned().collect()
            }
            Some(t) => vec![t.clone()],
            None => Vec::new(),
        }
    };
    let multi = op_paths.len() > 1;
    let any_target = !op_paths.is_empty();

    let mut items: Vec<AnyElement> = Vec::new();

    items.push(menu_item::<T>(
        "ft-menu-new-file",
        IconName::FileAdd,
        "New File",
        true,
        {
            let entity = entity.clone();
            let target = target.clone();
            cx.listener(move |_, _, window, cx| {
                cx.stop_propagation();
                let anchor = target.clone();
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                    tree.start_new_entry(anchor.as_deref(), NodeKind::File, cx);
                });
                actions::focus_new_entry_input(window, cx);
            })
        },
        cx,
    ));

    items.push(menu_item::<T>(
        "ft-menu-new-folder",
        IconName::FolderAdd,
        "New Folder",
        true,
        {
            let entity = entity.clone();
            let target = target.clone();
            cx.listener(move |_, _, window, cx| {
                cx.stop_propagation();
                let anchor = target.clone();
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                    tree.start_new_entry(anchor.as_deref(), NodeKind::Directory, cx);
                });
                actions::focus_new_entry_input(window, cx);
            })
        },
        cx,
    ));

    items.push(separator(theme));

    items.push(menu_item::<T>(
        "ft-menu-cut",
        IconName::Edit,
        if multi { "Cut Selection" } else { "Cut" },
        any_target,
        {
            let entity = entity.clone();
            let paths = op_paths.clone();
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let entity = entity.clone();
                if paths.is_empty() {
                    return;
                }
                let paths = paths.clone();
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                    tree.cut(paths, cx);
                });
            })
        },
        cx,
    ));

    items.push(menu_item::<T>(
        "ft-menu-copy",
        IconName::Copy,
        if multi { "Copy Selection" } else { "Copy" },
        any_target,
        {
            let entity = entity.clone();
            let paths = op_paths.clone();
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let entity = entity.clone();
                if paths.is_empty() {
                    return;
                }
                let paths = paths.clone();
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                    tree.copy(paths, cx);
                });
            })
        },
        cx,
    ));

    items.push(menu_item::<T>(
        "ft-menu-paste",
        IconName::Clippy,
        if cut_active { "Paste (move)" } else { "Paste" },
        has_clipboard,
        {
            let entity = entity.clone();
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let entity = entity.clone();
                let dest = match &target_for_paste {
                    Some(path) => path.clone(),
                    None => match entity.read(cx).root() {
                        Some(root) => root.to_path_buf(),
                        None => return,
                    },
                };
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                    tree.paste_into(dest, cx);
                });
            })
        },
        cx,
    ));

    items.push(separator(theme));

    items.push(menu_item::<T>(
        "ft-menu-rename",
        IconName::Edit,
        "Rename",
        target_is_some && !multi,
        {
            let entity = entity.clone();
            let target = target.clone();
            cx.listener(move |_, _, window, cx| {
                cx.stop_propagation();
                let Some(path) = target.clone() else { return };
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                });
                actions::begin_rename(path, &entity, window, cx);
            })
        },
        cx,
    ));

    items.push(menu_item::<T>(
        "ft-menu-reveal",
        IconName::Folder,
        "Reveal in File Explorer",
        target_is_some,
        {
            let entity = entity.clone();
            let target = target.clone();
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let entity = entity.clone();
                let Some(path) = target.clone() else { return };
                entity.update(cx, |tree, cx| tree.close_context_menu(cx));
                cx.reveal_path(&path);
            })
        },
        cx,
    ));

    items.push(separator(theme));

    items.push(menu_item::<T>(
        "ft-menu-trash",
        IconName::Trash,
        "Move to Trash",
        any_target,
        {
            let entity = entity.clone();
            let paths = op_paths.clone();
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let entity = entity.clone();
                if paths.is_empty() {
                    return;
                }
                let paths = paths.clone();
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                    tree.trash(paths, cx);
                });
            })
        },
        cx,
    ));

    items.push(menu_item::<T>(
        "ft-menu-delete",
        IconName::Close,
        "Delete Permanently",
        any_target,
        {
            let entity = entity.clone();
            let paths = op_paths.clone();
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let entity = entity.clone();
                if paths.is_empty() {
                    return;
                }
                let paths = paths.clone();
                entity.update(cx, |tree, cx| {
                    tree.close_context_menu(cx);
                    tree.delete(paths, cx);
                });
            })
        },
        cx,
    ));

    let _ = ClipboardOp::Cut;

    deferred(
        anchored().position(position).snap_to_window().child(
            div()
                .id("file-tree-context-menu")
                .min_w(rems(13.0))
                .p_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .rounded(rems(0.375))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.bg_elevated)
                .shadow_lg()
                .text_sm()
                .text_color(theme.text)
                .block_mouse_except_scroll()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .children(items),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

fn menu_item<T: PaneDelegate + SettingsDelegate>(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    enabled: bool,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let hover_bg = theme.bg_selected;
    let hover_text = theme.text_emphasis;
    let text_color = if enabled {
        theme.text
    } else {
        theme.text_subtle
    };
    let icon_color = if enabled {
        theme.text_muted
    } else {
        theme.text_subtle
    };
    let label_text: SharedString = label.into();
    let _ = cx;

    let mut row = div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .h(rems(1.625))
        .px_2()
        .rounded(rems(0.25))
        .text_color(text_color)
        .child(
            div()
                .w(rems(1.0))
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).size(14.0).color(icon_color)),
        )
        .child(label_text);
    if enabled {
        row = row
            .hover(move |this| this.bg(hover_bg).text_color(hover_text))
            .on_click(listener);
    }
    row.into_any_element()
}

fn separator(theme: theme::Theme) -> AnyElement {
    div()
        .h(rems(0.0625))
        .my(rems(0.25))
        .bg(theme.border_subtle)
        .into_any_element()
}

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClickEvent, Context, Entity, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, SharedString, Window, div, prelude::*, rems,
};

use file_tree::{FileTree, NewEntryDraft, NodeKind};
use icons::{Icon, IconName};
use theme::{ActiveTheme, Theme};

use crate::delegate::{PaneDelegate, SettingsDelegate};
use crate::tabs::file_tree::drag::FileNodeDrag;
use crate::tabs::file_tree::state::ActiveFileTreeUi;

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

pub fn render_root<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    path: PathBuf,
    name: SharedString,
    state: RowState,
    actions_cluster: AnyElement,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let icon_name = if state.is_expanded {
        IconName::FolderOpened
    } else {
        IconName::Folder
    };

    let entity_click = entity.clone();
    let entity_secondary = entity.clone();
    let entity_drop = entity.clone();
    let click_path = path.clone();
    let secondary_path = path.clone();
    let drop_path = path.clone();
    let drop_filter_path = path.clone();
    let drop_highlight = drop_highlight_color(theme);

    let label = node_label(0, icon_name, name, state.is_selected, theme);

    div()
        .id(path_id("file-tree-root-row", &path))
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(ROW_HEIGHT)
        .px(rems(0.375))
        .text_sm()
        .text_color(if state.is_selected {
            theme.text_emphasis
        } else {
            theme.text_header
        })
        .hover(move |s| s.bg(theme.bg_hover))
        .when(state.is_selected, |s| s.bg(theme.bg_selected))
        .drag_over::<FileNodeDrag>(move |style, _, _, _| style.bg(drop_highlight))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let position = event.position;
                let target = secondary_path.clone();
                entity_secondary.update(cx, |t, cx| {
                    if !t.is_selected(&target) {
                        t.select(target.clone(), cx);
                    }
                    t.open_context_menu(Some(target), position, cx);
                });
            }),
        )
        .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
            let target = click_path.clone();
            let shift = event.modifiers().shift;
            entity_click.update(cx, |t, cx| {
                if shift {
                    t.extend_selection_to(target, cx);
                } else {
                    t.select(target.clone(), cx);
                    t.toggle_expand(&target, cx);
                }
            });
        }))
        .can_drop(move |drag, _, _| {
            drag.downcast_ref::<FileNodeDrag>()
                .is_some_and(|d| can_drop_into_dir(d, &drop_filter_path))
        })
        .on_drop(cx.listener(move |_, drag: &FileNodeDrag, _, cx| {
            cx.stop_propagation();
            let dest = drop_path.clone();
            let srcs = drag.paths.clone();
            entity_drop.update(cx, |t, cx| t.move_into(srcs, dest, cx));
        }))
        .child(div().flex_1().min_w_0().child(label))
        .child(actions_cluster)
        .into_any_element()
}

pub fn render_dir<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    path: PathBuf,
    name: SharedString,
    depth: usize,
    state: RowState,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let icon_name = if state.is_expanded {
        IconName::FolderOpened
    } else {
        IconName::Folder
    };

    let body = if state.is_renaming {
        rename_input_body::<T>(entity, depth, icon_name, cx)
    } else {
        node_label(depth, icon_name, name.clone(), state.is_selected, theme)
    };

    let entity_click = entity.clone();
    let entity_secondary = entity.clone();
    let entity_drop = entity.clone();
    let click_path = path.clone();
    let secondary_path = path.clone();
    let drop_path = path.clone();
    let drop_filter_path = path.clone();
    let drag_path = path.clone();
    let drag_name = name.clone();
    let drop_highlight = drop_highlight_color(theme);

    let mut row = div()
        .id(path_id("file-tree-dir-row", &path))
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(ROW_HEIGHT)
        .px(rems(0.375))
        .text_sm()
        .text_color(if state.is_selected {
            theme.text_emphasis
        } else {
            theme.text
        })
        .when(!state.is_renaming, |this| {
            this.hover(move |s| s.bg(theme.bg_hover))
                .when(state.is_selected, |s| s.bg(theme.bg_selected))
                .drag_over::<FileNodeDrag>(move |style, _, _, _| style.bg(drop_highlight))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let position = event.position;
                let target = secondary_path.clone();
                entity_secondary.update(cx, |t, cx| {
                    if !t.is_selected(&target) {
                        t.select(target.clone(), cx);
                    }
                    t.open_context_menu(Some(target), position, cx);
                });
            }),
        )
        .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
            let target = click_path.clone();
            let shift = event.modifiers().shift;
            entity_click.update(cx, |t, cx| {
                if shift {
                    t.extend_selection_to(target, cx);
                } else {
                    t.select(target.clone(), cx);
                    t.toggle_expand(&target, cx);
                }
            });
        }))
        .can_drop(move |drag, _, _| {
            drag.downcast_ref::<FileNodeDrag>()
                .is_some_and(|d| can_drop_into_dir(d, &drop_filter_path))
        })
        .on_drop(cx.listener(move |_, drag: &FileNodeDrag, _, cx| {
            cx.stop_propagation();
            let dest = drop_path.clone();
            let srcs = drag.paths.clone();
            entity_drop.update(cx, |t, cx| t.move_into(srcs, dest, cx));
        }))
        .child(body);

    if !state.is_renaming {
        let paths = drag_paths_for(entity, &drag_path, cx);
        row = row.on_drag(
            FileNodeDrag::new(paths, drag_name, icon_name, NodeKind::Directory),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        );
    }

    row.into_any_element()
}


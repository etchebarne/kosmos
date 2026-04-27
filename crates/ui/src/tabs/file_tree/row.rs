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
                    t.select(target.clone(), cx);
                    t.open_context_menu(Some(target), position, cx);
                });
            }),
        )
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
            let target = click_path.clone();
            entity_click.update(cx, |t, cx| {
                t.select(target.clone(), cx);
                t.toggle_expand(&target, cx);
            });
        }))
        .can_drop(move |drag, _, _| {
            drag.downcast_ref::<FileNodeDrag>()
                .is_some_and(|d| !drop_filter_path.starts_with(&d.path))
        })
        .on_drop(cx.listener(move |_, drag: &FileNodeDrag, _, cx| {
            cx.stop_propagation();
            let dest = drop_path.clone();
            let src = drag.path.clone();
            entity_drop.update(cx, |t, cx| t.move_into(src, dest, cx));
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
                    t.select(target.clone(), cx);
                    t.open_context_menu(Some(target), position, cx);
                });
            }),
        )
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
            let target = click_path.clone();
            entity_click.update(cx, |t, cx| {
                t.select(target.clone(), cx);
                t.toggle_expand(&target, cx);
            });
        }))
        .can_drop(move |drag, _, _| {
            let Some(d) = drag.downcast_ref::<FileNodeDrag>() else {
                return false;
            };
            // Rejected: target is the source itself or a descendant of source.
            if drop_filter_path.starts_with(&d.path) {
                return false;
            }
            // Rejected: source is already a direct child of this directory.
            if d.path.parent() == Some(drop_filter_path.as_path()) {
                return false;
            }
            true
        })
        .on_drop(cx.listener(move |_, drag: &FileNodeDrag, _, cx| {
            cx.stop_propagation();
            let dest = drop_path.clone();
            let src = drag.path.clone();
            entity_drop.update(cx, |t, cx| t.move_into(src, dest, cx));
        }))
        .child(body);

    if !state.is_renaming {
        row = row.on_drag(
            FileNodeDrag::new(drag_path, drag_name, icon_name),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        );
    }

    row.into_any_element()
}

pub fn render_file<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    path: PathBuf,
    name: SharedString,
    depth: usize,
    state: RowState,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let icon_name = IconName::File;

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
    let drag_path = path.clone();
    let drag_name = name.clone();
    let drop_filter_path = path.clone();
    let drop_target_dir = path.parent().map(Path::to_path_buf);
    let drop_highlight = drop_highlight_color(theme);

    let mut row = div()
        .id(path_id("file-tree-file-row", &path))
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(ROW_HEIGHT)
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
                    t.select(target.clone(), cx);
                    t.open_context_menu(Some(target), position, cx);
                });
            }),
        )
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
            let target = click_path.clone();
            entity_click.update(cx, |t, cx| t.select(target, cx));
        }))
        .can_drop({
            let parent = drop_target_dir.clone();
            move |drag, _, _| {
                let (Some(d), Some(parent)) = (drag.downcast_ref::<FileNodeDrag>(), parent.as_ref())
                else {
                    return false;
                };
                // Already a direct child of this destination.
                if d.path.parent() == Some(parent.as_path()) {
                    return false;
                }
                // Destination is the source itself or a descendant of source.
                if parent.starts_with(&d.path) {
                    return false;
                }
                true
            }
        })
        .on_drop(cx.listener(move |_, drag: &FileNodeDrag, _, cx| {
            cx.stop_propagation();
            let Some(dest) = drop_target_dir.clone() else {
                return;
            };
            if drag.path.parent() == Some(dest.as_path()) {
                return;
            }
            if dest.starts_with(&drag.path) {
                return;
            }
            let src = drag.path.clone();
            entity_drop.update(cx, |t, cx| t.move_into(src, dest, cx));
        }))
        .child(body);

    let _ = drop_filter_path;

    if !state.is_renaming {
        row = row.on_drag(
            FileNodeDrag::new(drag_path, drag_name, icon_name),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        );
    }

    row.into_any_element()
}

pub fn render_new_entry<T: PaneDelegate + SettingsDelegate>(
    draft: &NewEntryDraft,
    entity: &Entity<FileTree>,
    depth: usize,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let icon_name = match draft.kind {
        NodeKind::Directory => IconName::Folder,
        NodeKind::File => IconName::File,
    };
    let entity_submit = entity.clone();
    let entity_cancel = entity.clone();

    let Some(input) = cx.file_tree_ui().map(|ui| ui.input()) else {
        return div().into_any_element();
    };
    let input_for_submit = input.clone();

    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(ROW_HEIGHT)
        .text_sm()
        .bg(theme.bg_hover)
        .child(indent_guides(depth, theme))
        .child(
            div()
                .w(rems(ICON_WIDTH_REM))
                .h(ROW_HEIGHT)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon_name).size(14.0).color(theme.text_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .capture_key_down(cx.listener(
                    move |_, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<T>| {
                        match event.keystroke.key.as_str() {
                            "enter" => {
                                cx.stop_propagation();
                                let value =
                                    input_for_submit.read(cx).value().to_string();
                                entity_submit
                                    .update(cx, |t, cx| t.apply_new_entry(value, cx));
                            }
                            "escape" => {
                                cx.stop_propagation();
                                entity_cancel
                                    .update(cx, |t, cx| t.cancel_new_entry(cx));
                            }
                            _ => {}
                        }
                    },
                ))
                .child(input),
        )
        .into_any_element()
}

fn rename_input_body<T: PaneDelegate + SettingsDelegate>(
    entity: &Entity<FileTree>,
    depth: usize,
    icon_name: IconName,
    cx: &mut Context<T>,
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
                .child(Icon::new(icon_name).size(14.0).color(theme.text_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .capture_key_down(cx.listener(
                    move |_, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<T>| {
                        match event.keystroke.key.as_str() {
                            "enter" => {
                                cx.stop_propagation();
                                let value = input_for_submit.read(cx).value().to_string();
                                entity_submit.update(cx, |t, cx| t.apply_rename(value, cx));
                            }
                            "escape" => {
                                cx.stop_propagation();
                                entity_cancel.update(cx, |t, cx| t.cancel_rename(cx));
                            }
                            _ => {}
                        }
                    },
                ))
                .child(input),
        )
        .into_any_element()
}

fn node_label(
    depth: usize,
    icon: IconName,
    name: SharedString,
    is_selected: bool,
    theme: Theme,
) -> AnyElement {
    let icon_color = if is_selected {
        theme.text
    } else {
        theme.text_muted
    };

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
                .child(Icon::new(icon).size(14.0).color(icon_color)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pl(rems(0.25))
                .pr(rems(0.5))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(name),
        )
        .into_any_element()
}

fn drop_highlight_color(theme: Theme) -> gpui::Hsla {
    gpui::Hsla::from(theme.accent).opacity(0.18)
}

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
                        .bg(theme.border_subtle),
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

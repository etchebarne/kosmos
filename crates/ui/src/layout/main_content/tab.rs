use std::time::Duration;

use file_tree::NodeKind;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, IntoElement, MouseButton, SharedString, Window,
    div, ease_in_out, prelude::*, rems, rgb,
};

use file_editor::BufferStore;
use icons::{Icon, IconName};
use panes::Pane;
use tabs::Tab;
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, TabAnimationState};
use crate::drag::TabDrag;
use crate::metrics::{TAB_ANIMATION_DURATION_MS, TAB_HEIGHT, TAB_RADIUS};
use crate::tabs::file_tree::drag::FileNodeDrag;

const TAB_HORIZONTAL_PADDING_REM: f32 = 1.0;
const TAB_ICON_WIDTH_REM: f32 = 1.0;
const TAB_CLOSE_BUTTON_WIDTH_REM: f32 = 1.25;
const TAB_CONTENT_GAP_REM: f32 = 0.5;
const TAB_DIRTY_DOT_WIDTH_REM: f32 = 0.375;

pub fn render<T: PaneDelegate>(
    workspace_id: usize,
    pane: &Pane,
    tab: &Tab,
    can_close: bool,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let pane_id = pane.id();
    let id = tab.id;
    let is_active = pane.active_tab() == id;
    let hover_group = SharedString::from(format!("tab-{pane_id}-{id}"));
    let title = SharedString::from(tab.title());
    let icon_name = crate::tabs::icon_for_tab(tab);
    let accent = theme.accent;
    let is_dirty = tab
        .path
        .as_deref()
        .is_some_and(|path| BufferStore::is_path_dirty(path, cx));
    let animation_phase = cx
        .try_global::<TabAnimationState>()
        .and_then(|state| state.phase(workspace_id, pane_id, id));
    let target_width = tab_width_rems(window, title.as_ref(), is_dirty);

    let content = div()
        .h_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .child(
            Icon::new(icon_name)
                .size(16.0)
                .color(if is_active {
                    theme.text
                } else {
                    theme.text_subtle
                })
                .into_any_element(),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(title.clone()),
        )
        .when(is_dirty, |this| {
            this.child(
                div()
                    .size(rems(0.375))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(0xffffff)),
            )
        })
        .child(
            div()
                .id(("close-tab", id))
                .size(rems(1.25))
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.25))
                .text_color(theme.text)
                .invisible()
                .when(can_close, |this| {
                    let close_hover_bg = theme.bg_close_hover;
                    this.group_hover(hover_group.clone(), |this| this.visible())
                        .hover(move |this| this.bg(close_hover_bg))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_tab(pane_id, id, cx);
                        }))
                })
                .child(Icon::new(IconName::Close).size(14.0).color(theme.text)),
        );
    let content = if let Some(phase) = animation_phase {
        let animation_id = SharedString::from(format!(
            "tab-content-{workspace_id}-{pane_id}-{id}-{}",
            phase.key()
        ));
        content
            .with_animation(animation_id, tab_animation(), move |this, delta| {
                this.opacity(phase.progress(delta))
            })
            .into_any_element()
    } else {
        content.into_any_element()
    };

    let tab = div()
        .id(("tab", id))
        .group(hover_group.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(TAB_HEIGHT)
        .w(rems(target_width))
        .overflow_hidden()
        .rounded(TAB_RADIUS)
        .when(is_active, |this| this.bg(theme.bg_selected))
        .text_color(if is_active {
            theme.text_emphasis
        } else {
            theme.text_muted
        })
        .text_sm()
        .hover(move |this| this.bg(theme.bg_hover))
        .can_drop(move |drag, _, _| {
            if drag
                .downcast_ref::<TabDrag>()
                .is_some_and(|drag| drag.id != id)
            {
                return true;
            }
            drag.downcast_ref::<FileNodeDrag>()
                .is_some_and(|d| d.kind == NodeKind::File)
        })
        .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
            cx.stop_propagation();
            this.move_tab_before(drag.clone(), pane_id, id, cx);
        }))
        .on_drop(cx.listener(move |this, drag: &FileNodeDrag, _, cx| {
            if drag.kind != NodeKind::File {
                return;
            }
            let Some(path) = drag.paths.first().cloned() else {
                return;
            };
            cx.stop_propagation();
            this.open_file_before(path, pane_id, id, cx);
        }))
        .on_drag(
            TabDrag::new(id, pane_id, title.clone(), icon_name),
            |drag, position, _, cx| cx.new(|_| drag.clone().position(position)),
        )
        .when(can_close, |this| {
            this.on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_tab(pane_id, id, cx);
                }),
            )
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_tab(pane_id, id, cx);
        }))
        .child(
            div()
                .absolute()
                .left(rems(0.0))
                .top(rems(0.25))
                .bottom(rems(0.25))
                .w(rems(0.125))
                .rounded_full()
                // No-op hover forces GPUI to insert a hitbox; without it,
                // group_drag_over styles are skipped and the line never paints.
                .hover(|s| s)
                .group_drag_over::<TabDrag>(hover_group.clone(), move |s| s.bg(accent))
                .group_drag_over::<FileNodeDrag>(hover_group.clone(), move |s| s.bg(accent)),
        )
        .child(content);

    if let Some(phase) = animation_phase {
        let animation_id = SharedString::from(format!(
            "tab-width-{workspace_id}-{pane_id}-{id}-{}",
            phase.key()
        ));
        tab.with_animation(animation_id, tab_animation(), move |this, delta| {
            this.w(rems(target_width * phase.progress(delta)))
        })
        .into_any_element()
    } else {
        tab.into_any_element()
    }
}

fn tab_animation() -> Animation {
    Animation::new(Duration::from_millis(TAB_ANIMATION_DURATION_MS)).with_easing(ease_in_out)
}

fn tab_width_rems(window: &mut Window, title: &str, is_dirty: bool) -> f32 {
    let fixed_content_width = TAB_HORIZONTAL_PADDING_REM
        + TAB_ICON_WIDTH_REM
        + TAB_CLOSE_BUTTON_WIDTH_REM
        + (2.0 * TAB_CONTENT_GAP_REM);
    let dirty_indicator_width = if is_dirty {
        TAB_DIRTY_DOT_WIDTH_REM + TAB_CONTENT_GAP_REM
    } else {
        0.0
    };
    fixed_content_width + dirty_indicator_width + measure_text_rems(window, title)
}

fn measure_text_rems(window: &mut Window, text: &str) -> f32 {
    let style = window.text_style();
    let run = style.to_run(text.len());
    let rem_size = window.rem_size();
    let font_size = rem_size * 0.875_f32;
    let layout = window
        .text_system()
        .layout_line(text, font_size, &[run], None);
    f32::from(layout.width) / f32::from(rem_size)
}

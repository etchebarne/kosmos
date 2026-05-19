fn loading_state<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    empty_panel("Loading Git status", cx)
}

fn commit_panel<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    summary: Option<&RepositorySummary>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let commit_message = cx
        .global::<GitUiState>()
        .commit_message
        .as_ref()
        .unwrap()
        .clone();
    let has_staged = summary.is_some_and(|summary| summary.files.iter().any(|file| file.staged));
    let root_commit = root.to_path_buf();
    let message_input = commit_message.clone();

    div()
        .flex_none()
        .border_t_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface)
        .child(
            div()
                .min_w_0()
                .w_full()
                .h(rems(COMMIT_PANEL_HEIGHT_REM))
                .flex()
                .flex_col()
                .pb(rems(COMMIT_CONTROLS_INSET_BOTTOM_REM))
                .child(sync_action_panel(root, summary, cx))
                .child(commit_message)
                .child(
                    div()
                        .flex_none()
                        .w_full()
                        .px(rems(COMMIT_CONTROLS_INSET_X_REM))
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(commit_button(
                            has_staged,
                            cx.listener(move |_, _, _, cx| {
                                let message = message_input.read(cx).value().to_string();
                                commit_tracked(
                                    root_commit.clone(),
                                    message,
                                    message_input.clone(),
                                    cx,
                                );
                            }),
                            cx,
                        )),
                ),
        )
        .into_any_element()
}

fn sync_action_panel<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    summary: Option<&RepositorySummary>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let action = cx.global::<GitUiState>().last_sync_action;
    let branch = summary
        .and_then(|summary| summary.branch.as_deref())
        .unwrap_or("Detached HEAD")
        .to_string();
    let root_branch = root.to_path_buf();
    let root_action = root.to_path_buf();

    div()
        .id("git-sync-panel")
        .flex_none()
        .w_full()
        .border_b_1()
        .border_color(theme.border_subtle)
        .px(rems(SYNC_PANEL_INSET_X_REM))
        .py_1p5()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(branch_button(root_branch, branch, cx)),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .child(sync_action_button(
                    "git-sync-primary-action",
                    action,
                    move |_, _, cx| run_sync_action(root_action.clone(), action, false, cx),
                    cx,
                ))
                .child(sync_more_button(cx)),
        )
        .into_any_element()
}

fn branch_button<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    branch: String,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("git-current-branch")
        .min_w_0()
        .max_w_full()
        .max_w(rems(12.0))
        .rounded(rems(0.3125))
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg_elevated)
        .px_2()
        .py_1()
        .text_sm()
        .text_color(theme.text)
        .hover(move |this| this.bg(theme.bg_hover))
        .on_click(cx.listener(move |_, _, _, cx| {
            open_modal(root.clone(), GitModal::Branches, cx);
        }))
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1p5()
                .child(
                    Icon::new(IconName::SourceControl)
                        .size(14.0)
                        .color(theme.text_muted),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(branch),
                ),
        )
        .into_any_element()
}

fn sync_action_button<T: PaneDelegate + SettingsDelegate>(
    id: &'static str,
    action: GitSyncAction,
    listener: impl Fn(&ClickEvent, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let color = if action.is_danger() {
        theme.danger
    } else {
        theme.text
    };

    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1p5()
        .rounded(rems(0.3125))
        .border_1()
        .border_color(if action.is_danger() {
            theme.danger
        } else {
            theme.border
        })
        .bg(theme.bg_elevated)
        .px_2()
        .py_1()
        .text_sm()
        .text_color(color)
        .hover(move |this| this.bg(theme.bg_hover))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        }))
        .child(Icon::new(action.icon()).size(14.0).color(color))
        .child(action.label())
        .into_any_element()
}

fn sync_more_button<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let menu_anchor = Rc::new(RefCell::new(None::<Point<Pixels>>));
    let paint_anchor = menu_anchor.clone();
    let click_anchor = menu_anchor.clone();

    div()
        .flex_none()
        .on_children_prepainted(move |bounds, window, _| {
            let gap = rems(SYNC_MENU_GAP_REM).to_pixels(window.rem_size());
            *paint_anchor.borrow_mut() = bounds.first().map(|bounds| {
                Point::new(bounds.right(), bounds.top() - gap)
            });
        })
        .child(
            div()
                .id("git-sync-more")
                .size(rems(1.875))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.3125))
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg_elevated)
                .text_color(theme.text_muted)
                .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let position = (*click_anchor.borrow()).unwrap_or(event.position);
                        cx.update_global::<GitUiState, _>(|state, _| {
                            state.menu_position = None;
                            state.sync_menu_position = match state.sync_menu_position {
                                Some(_) => None,
                                None => Some(position),
                            };
                        });
                        cx.notify();
                    }),
                )
                .child(
                    Icon::new(IconName::Ellipsis)
                        .size(14.0)
                        .color(theme.text_muted),
                ),
        )
        .into_any_element()
}

fn latest_commit_panel<T: PaneDelegate + SettingsDelegate>(
    commit: CommitInfo,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let subject = commit
        .subject
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    if subject.is_empty() {
        return div().into_any_element();
    }

    div()
        .flex_none()
        .border_t_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface)
        .px_3()
        .py_2()
        .flex()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_xs()
                .text_color(theme.text_subtle)
                .child(SharedString::from(subject)),
        )
        .into_any_element()
}

fn empty_panel<T: PaneDelegate + SettingsDelegate>(
    message: &'static str,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .items_center()
        .gap_2()
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_elevated)
        .p_3()
        .text_sm()
        .text_color(theme.text_subtle)
        .child(
            Icon::new(IconName::SourceControl)
                .size(14.0)
                .color(theme.text_muted),
        )
        .child(message)
        .into_any_element()
}

fn diff_stats<T: PaneDelegate + SettingsDelegate>(
    summary: &RepositorySummary,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let added = rgb(0x22c55e);
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .when(summary.insertions > 0, |this| {
            this.child(
                div()
                    .rounded(rems(0.25))
                    .bg(gpui::Hsla::from(added).opacity(0.12))
                    .px_1p5()
                    .py_0p5()
                    .text_color(added)
                    .child(format!("+{}", summary.insertions)),
            )
        })
        .when(summary.deletions > 0, |this| {
            this.child(
                div()
                    .rounded(rems(0.25))
                    .bg(gpui::Hsla::from(theme.danger).opacity(0.12))
                    .px_1p5()
                    .py_0p5()
                    .text_color(theme.danger)
                    .child(format!("-{}", summary.deletions)),
            )
        })
        .into_any_element()
}

fn icon_button<T: PaneDelegate + SettingsDelegate>(
    id: &'static str,
    icon: IconName,
    tooltip: Option<&'static str>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let _ = cx;
    let button = div()
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
        .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        }))
        .child(Icon::new(icon).size(14.0).color(theme.text_muted));

    match tooltip {
        Some(tooltip) => Tooltip::new(format!("{id}-tooltip"), tooltip, button)
            .position(TooltipPosition::Bottom)
            .into_any_element(),
        None => button.into_any_element(),
    }
}

fn more_button<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    div()
        .id("git-more")
        .size(rems(1.375))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rems(0.25))
        .text_color(theme.text_muted)
        .hover(move |this| this.bg(theme.bg_hover).text_color(theme.text_emphasis))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let position = event.position;
                cx.update_global::<GitUiState, _>(|state, _| {
                    state.sync_menu_position = None;
                    state.menu_position = match state.menu_position {
                        Some(_) => None,
                        None => Some(position),
                    };
                });
                cx.notify();
            }),
        )
        .child(
            Icon::new(IconName::Ellipsis)
                .size(14.0)
                .color(theme.text_muted),
        )
        .into_any_element()
}

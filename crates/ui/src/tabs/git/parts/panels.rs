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
                .child(
                    Input::new(&commit_message)
                        .h(rems(COMMIT_MESSAGE_HEIGHT_REM))
                        .appearance(false)
                        .px(rems(COMMIT_MESSAGE_PADDING_X_REM))
                        .pt(rems(COMMIT_MESSAGE_PADDING_TOP_REM))
                        .pb(rems(COMMIT_MESSAGE_PADDING_BOTTOM_REM)),
                )
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
                            cx.listener(move |_, _, window, cx| {
                                let message = message_input.read(cx).value().to_string();
                                commit_tracked(
                                    root_commit.clone(),
                                    message,
                                    message_input.clone(),
                                    window,
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
    let (action, sync_action_running) = {
        let state = cx.global::<GitUiState>();
        (state.last_sync_action, state.sync_action_running.is_some())
    };
    let branch_sync = summary.map(|summary| summary.branch_sync);
    let branch = summary
        .and_then(|summary| summary.branch.as_deref())
        .unwrap_or("Detached HEAD")
        .to_string();
    let root_branch = root.to_path_buf();
    let root_action = root.to_path_buf();
    let root_more = root.to_path_buf();

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
                .flex()
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
                    branch_sync,
                    sync_action_running,
                    move |_, _, cx| run_sync_action(root_action.clone(), action, false, cx),
                    cx,
                ))
                .child(sync_more_button(root_more, sync_action_running, cx)),
        )
        .into_any_element()
}

fn branch_button<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    branch: String,
    cx: &mut Context<T>,
) -> AnyElement {
    Button::new("git-current-branch")
        .secondary()
        .min_w_0()
        .max_w(rems(12.0))
        .on_click(cx.listener(move |_, _, _, cx| {
            open_modal(root.clone(), GitModal::Branches, cx);
        }))
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1p5()
                .child(component_icon(IconName::SourceControl).small())
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
    branch_sync: Option<BranchSyncStatus>,
    loading: bool,
    listener: impl Fn(&ClickEvent, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let branch_sync = branch_sync.unwrap_or_default();
    let indicators = (!branch_sync.is_synced()).then(|| sync_status_indicators(branch_sync));
    let tooltip = sync_status_tooltip(branch_sync);

    Button::new(id)
        .secondary()
        .when(action.is_danger(), |this| this.danger())
        .icon(component_icon(action.icon()))
        .loading_icon(component_icon(IconName::Refresh))
        .label(action.label())
        .loading(loading)
        .when_some(indicators, |this, indicators| this.child(indicators))
        .when_some(tooltip, |this, tooltip| this.tooltip(tooltip))
        .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        }))
        .into_any_element()
}

fn sync_status_indicators(branch_sync: BranchSyncStatus) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .when(branch_sync.ahead > 0, |this| {
            this.child(sync_status_indicator(IconName::ArrowUp, branch_sync.ahead))
        })
        .when(branch_sync.behind > 0, |this| {
            this.child(sync_status_indicator(IconName::ArrowDown, branch_sync.behind))
        })
        .into_any_element()
}

fn sync_status_indicator(icon: IconName, count: usize) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .opacity(0.6)
        .child(component_icon(icon).xsmall())
        .child(div().text_xs().child(count.to_string()))
        .into_any_element()
}

fn sync_status_tooltip(branch_sync: BranchSyncStatus) -> Option<String> {
    if branch_sync.is_synced() {
        return None;
    }

    let mut parts = Vec::new();
    if branch_sync.ahead > 0 {
        parts.push(format!(
            "{} commit{} to push",
            branch_sync.ahead,
            plural(branch_sync.ahead)
        ));
    }
    if branch_sync.behind > 0 {
        parts.push(format!(
            "{} commit{} to pull",
            branch_sync.behind,
            plural(branch_sync.behind)
        ));
    }
    Some(parts.join(", "))
}

fn sync_more_button<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    disabled: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let actions = GitSyncAction::ALL
        .into_iter()
        .map(|action| {
            let root = root.clone();
            let listener: PopupMenuHandler = Rc::new(
                cx.listener(move |_, _, _, cx| run_sync_action(root.clone(), action, true, cx)),
            );
            (
                action.icon(),
                action.label(),
                true,
                action.is_danger(),
                listener,
            )
        })
        .collect::<Vec<_>>();

    Button::new("git-sync-more")
        .secondary()
        .tab_stop(false)
        .disabled(disabled)
        .icon(component_icon(IconName::Ellipsis))
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, _| {
            let menu_width = rems(11.0).to_pixels(window.rem_size());
            actions.iter().fold(
                menu.min_w(menu_width),
                |menu, (icon, label, enabled, danger, listener)| {
                    menu.item(popup_menu_item(
                        *icon,
                        label,
                        *enabled,
                        *danger,
                        listener.clone(),
                    ))
                },
            )
        })
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
    let _ = cx;
    Alert::new("git-empty-panel", message)
        .with_size(Size::Small)
        .icon(component_icon(IconName::SourceControl))
        .into_any_element()
}

fn init_repository_panel<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    cx: &mut Context<T>,
) -> AnyElement {
    let root = root.to_path_buf();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            Button::new("git-init-repository")
                .secondary()
                .label("Initialize Repository")
                .on_click(cx.listener(move |_, _, _, cx| {
                    run_git_action(root.clone(), kosmos_git::init, cx);
                })),
        )
        .into_any_element()
}

fn diff_stats<T: PaneDelegate + SettingsDelegate>(
    summary: &RepositorySummary,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .items_center()
        .gap_1()
        .when(summary.insertions > 0, |this| {
            this.child(diff_stat_text(format!("+{}", summary.insertions), theme.success))
        })
        .when(summary.deletions > 0, |this| {
            this.child(diff_stat_text(format!("-{}", summary.deletions), theme.danger))
        })
        .into_any_element()
}

fn diff_stat_text(label: impl Into<SharedString>, color: gpui::Rgba) -> AnyElement {
    div()
        .text_xs()
        .text_color(color)
        .child(label.into())
        .into_any_element()
}

fn icon_button<T: PaneDelegate + SettingsDelegate>(
    id: &'static str,
    icon: IconName,
    tooltip: Option<&'static str>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    Button::new(id)
        .ghost()
        .small()
        .tab_stop(false)
        .size(rems(1.375))
        .icon(ComponentIcon::empty().path(icon.path()))
        .when_some(tooltip, |this, tooltip| this.tooltip(tooltip))
        .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
            cx.stop_propagation();
            listener(event, window, cx);
        }))
        .into_any_element()
}

fn more_button<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    cx: &mut Context<T>,
) -> AnyElement {
    let root_branches = root.to_path_buf();
    let root_remotes = root.to_path_buf();
    let root_stashes = root.to_path_buf();
    let root_tags = root.to_path_buf();
    let root_discard_selected = root.to_path_buf();
    let root_discard = root.to_path_buf();
    let has_selected_changes = cx
        .global::<GitUiState>()
        .summary
        .as_ref()
        .is_some_and(|summary| summary.files.iter().any(|file| file.staged));

    let items = vec![
        (
            IconName::SourceControl,
            "Branches",
            true,
            false,
            Rc::new(cx.listener(move |_, _, _, cx| {
                open_modal(root_branches.clone(), GitModal::Branches, cx)
            })) as PopupMenuHandler,
        ),
        (
            IconName::Server,
            "Remotes",
            true,
            false,
            Rc::new(cx.listener(move |_, _, _, cx| {
                open_modal(root_remotes.clone(), GitModal::Remotes, cx)
            })) as PopupMenuHandler,
        ),
        (
            IconName::Archive,
            "Stashes",
            true,
            false,
            Rc::new(cx.listener(move |_, _, _, cx| {
                open_modal(root_stashes.clone(), GitModal::Stashes, cx)
            })) as PopupMenuHandler,
        ),
        (
            IconName::Tag,
            "Tags",
            true,
            false,
            Rc::new(
                cx.listener(move |_, _, _, cx| open_modal(root_tags.clone(), GitModal::Tags, cx)),
            ) as PopupMenuHandler,
        ),
    ];
    let danger_items = vec![
        (
            IconName::Trash,
            "Discard Selected Changes",
            has_selected_changes,
            true,
            Rc::new(cx.listener(move |_, _, _, cx| {
                open_modal(
                    root_discard_selected.clone(),
                    GitModal::ConfirmDiscardSelected,
                    cx,
                )
            })) as PopupMenuHandler,
        ),
        (
            IconName::Trash,
            "Discard All Changes",
            true,
            true,
            Rc::new(cx.listener(move |_, _, _, cx| {
                open_modal(root_discard.clone(), GitModal::ConfirmDiscard, cx)
            })) as PopupMenuHandler,
        ),
    ];

    Button::new("git-more")
        .ghost()
        .small()
        .tab_stop(false)
        .size(rems(1.375))
        .icon(component_icon(IconName::Ellipsis))
        .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, window, _| {
            let menu_width = rems(11.0).to_pixels(window.rem_size());
            let menu = items.iter().fold(
                menu.min_w(menu_width),
                |menu, (icon, label, enabled, danger, listener)| {
                    menu.item(popup_menu_item(
                        *icon,
                        label,
                        *enabled,
                        *danger,
                        listener.clone(),
                    ))
                },
            );
            danger_items.iter().fold(
                menu.separator(),
                |menu, (icon, label, enabled, danger, listener)| {
                    menu.item(popup_menu_item(
                        *icon,
                        label,
                        *enabled,
                        *danger,
                        listener.clone(),
                    ))
                },
            )
        })
        .into_any_element()
}

fn popup_menu_item(
    icon: IconName,
    label: &'static str,
    enabled: bool,
    danger: bool,
    listener: PopupMenuHandler,
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
                    .child(component_icon(icon).small()),
            )
            .child(label)
    })
    .disabled(!enabled)
    .on_click(move |event, window, cx| listener(event, window, cx))
}

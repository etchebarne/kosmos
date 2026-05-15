fn more_menu<T: PaneDelegate + SettingsDelegate>(
    root: &PathBuf,
    position: Point<Pixels>,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let root_push = root.clone();
    let root_force_push = root.clone();
    let root_pull = root.clone();
    let root_pull_rebase = root.clone();
    let root_fetch = root.clone();
    let root_branches = root.clone();
    let root_remotes = root.clone();
    let root_stashes = root.clone();
    let root_tags = root.clone();
    let root_discard_selected = root.clone();
    let root_discard = root.clone();
    let has_selected_changes = cx
        .global::<GitUiState>()
        .summary
        .as_ref()
        .is_some_and(|summary| summary.files.iter().any(|file| file.staged));
    deferred(
        anchored().position(position).snap_to_window().child(
            div()
                .id("git-more-menu")
                .min_w(rems(11.0))
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
                .child(menu_item::<T>(
                    "git-menu-push",
                    IconName::ArrowUp,
                    "Push",
                    true,
                    false,
                    move |_, _, cx| run_git_action(root_push.clone(), kosmos_git::push, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-force-push",
                    IconName::ArrowUp,
                    "Force Push",
                    true,
                    true,
                    move |_, _, cx| {
                        run_git_action(root_force_push.clone(), kosmos_git::force_push, cx)
                    },
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-pull",
                    IconName::ArrowDown,
                    "Pull",
                    true,
                    false,
                    move |_, _, cx| run_git_action(root_pull.clone(), kosmos_git::pull, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-pull-rebase",
                    IconName::ArrowDown,
                    "Pull (Rebase)",
                    true,
                    false,
                    move |_, _, cx| {
                        run_git_action(root_pull_rebase.clone(), kosmos_git::pull_rebase, cx)
                    },
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-fetch",
                    IconName::Refresh,
                    "Fetch",
                    true,
                    false,
                    move |_, _, cx| run_git_action(root_fetch.clone(), kosmos_git::fetch, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-branches",
                    IconName::SourceControl,
                    "Branches",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_branches.clone(), GitModal::Branches, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-remotes",
                    IconName::Server,
                    "Remotes",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_remotes.clone(), GitModal::Remotes, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-stashes",
                    IconName::Archive,
                    "Stashes",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_stashes.clone(), GitModal::Stashes, cx),
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-tags",
                    IconName::Tag,
                    "Tags",
                    true,
                    false,
                    move |_, _, cx| open_modal(root_tags.clone(), GitModal::Tags, cx),
                    cx,
                ))
                .child(menu_separator(theme))
                .child(menu_item::<T>(
                    "git-menu-discard-selected",
                    IconName::Trash,
                    "Discard Selected Changes",
                    has_selected_changes,
                    true,
                    move |_, _, cx| {
                        open_modal(
                            root_discard_selected.clone(),
                            GitModal::ConfirmDiscardSelected,
                            cx,
                        )
                    },
                    cx,
                ))
                .child(menu_item::<T>(
                    "git-menu-discard-all",
                    IconName::Trash,
                    "Discard All Changes",
                    true,
                    true,
                    move |_, _, cx| open_modal(root_discard.clone(), GitModal::ConfirmDiscard, cx),
                    cx,
                )),
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
    danger: bool,
    listener: impl Fn(&ClickEvent, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let text_color = if enabled {
        if danger { theme.danger } else { theme.text }
    } else {
        theme.text_subtle
    };
    let icon_color = if enabled {
        if danger {
            theme.danger
        } else {
            theme.text_muted
        }
    } else {
        theme.text_subtle
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .h(rems(1.625))
        .px_2()
        .rounded(rems(0.25))
        .text_color(text_color)
        .when(enabled, |this| {
            this.hover(move |this| this.bg(theme.bg_selected).text_color(theme.text_emphasis))
                .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    listener(event, window, cx);
                }))
        })
        .child(
            div()
                .w(rems(1.0))
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).size(14.0).color(icon_color)),
        )
        .child(label)
        .into_any_element()
}

fn menu_separator(theme: theme::Theme) -> AnyElement {
    div()
        .h(rems(0.0625))
        .my(rems(0.25))
        .bg(theme.border_subtle)
        .into_any_element()
}

fn menu_dismiss_layer<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    div()
        .id("git-menu-dismiss")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| close_menu(cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|_, _, _, cx| close_menu(cx)),
        )
        .into_any_element()
}


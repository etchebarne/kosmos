fn render_git_modal<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    modal_state: GitModal,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    match modal_state {
        GitModal::Branches => modal::render(
            "git-branches-modal",
            "Git Branches",
            branches_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::CreateBranch => modal::render(
            "git-create-branch-modal",
            "Create Branch",
            create_branch_modal_body(cx),
            create_branch_modal_footer(root, cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::Remotes => modal::render(
            "git-remotes-modal",
            "Git Remotes",
            remotes_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::Stashes => modal::render(
            "git-stashes-modal",
            "Git Stashes",
            stashes_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::Tags => modal::render(
            "git-tags-modal",
            "Git Tags",
            tags_modal_body(root, cx),
            modal_footer(close_modal_button(cx), cx),
            theme,
            cx.listener(|_, _, _, cx| close_modal(cx)),
        ),
        GitModal::ConfirmDiscardSelected => {
            let root = root.to_path_buf();
            let selected_paths = selected_change_paths(cx);
            let selected_count = selected_paths.len();
            confirm_git_action_modal(
                ConfirmGitActionModal {
                    modal_id: "git-discard-selected-modal",
                    title: "Discard Selected Changes",
                    message: format!(
                        "This will permanently discard {selected_count} selected working tree change{}. This action cannot be undone.",
                        plural(selected_count)
                    ),
                    button_id: "git-confirm-discard-selected",
                    button_label: "Discard Selected",
                    confirm: cx.listener(move |_, _, _, cx| {
                        close_modal(cx);
                        run_git_action(
                            root.clone(),
                            {
                                let selected_paths = selected_paths.clone();
                                move |root| kosmos_git::discard_files(root, &selected_paths)
                            },
                            cx,
                        );
                    }),
                },
                theme,
                cx,
            )
        }
        GitModal::ConfirmDiscard => {
            let root = root.to_path_buf();
            confirm_git_action_modal(
                ConfirmGitActionModal {
                    modal_id: "git-discard-modal",
                    title: "Discard All Changes",
                    message: "This will permanently discard all tracked and untracked working tree changes. This action cannot be undone.".to_string(),
                    button_id: "git-confirm-discard",
                    button_label: "Discard All",
                    confirm: cx.listener(move |_, _, _, cx| {
                        close_modal(cx);
                        run_git_action(root.clone(), kosmos_git::discard_all_changes, cx);
                    }),
                },
                theme,
                cx,
            )
        }
    }
}

struct ConfirmGitActionModal<C> {
    modal_id: &'static str,
    title: &'static str,
    message: String,
    button_id: &'static str,
    button_label: &'static str,
    confirm: C,
}

fn confirm_git_action_modal<T, C>(
    config: ConfirmGitActionModal<C>,
    theme: theme::Theme,
    cx: &mut Context<T>,
) -> AnyElement
where
    T: PaneDelegate + SettingsDelegate,
    C: Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
{
    modal::render(
        config.modal_id,
        config.title,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_sm()
            .child(config.message)
            .into_any_element(),
        div()
            .flex()
            .justify_end()
            .gap_2()
            .child(close_modal_button(cx))
            .child(action_button(
                config.button_id,
                config.button_label,
                true,
                config.confirm,
                cx,
            ))
            .into_any_element(),
        theme,
        cx.listener(|_, _, _, cx| close_modal(cx)),
    )
}

fn branches_modal_body<T: PaneDelegate + SettingsDelegate>(
    root: &Path,
    cx: &mut Context<T>,
) -> AnyElement {
    let (branch_search, branches, last_error) = {
        let state = cx.global::<GitUiState>();
        (
            state.branch_search.as_ref().unwrap().clone(),
            state.branches.clone(),
            state.last_error.clone(),
        )
    };
    let theme = *cx.theme();
    let query = branch_search.read(cx).value().trim().to_lowercase();
    let has_branches = !branches.is_empty();
    let branches = if query.is_empty() {
        branches
    } else {
        branches
            .into_iter()
            .filter(|branch| {
                branch.name.to_lowercase().contains(&query)
                    || (if branch.remote { "remote" } else { "local" }).contains(&query)
            })
            .collect()
    };

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(input_row("Search Branches", branch_search))
        .when_some(last_error, |this, error| {
            this.child(
                div()
                    .rounded(rems(0.375))
                    .border_1()
                    .border_color(gpui::Hsla::from(theme.danger).opacity(0.35))
                    .bg(gpui::Hsla::from(theme.danger).opacity(0.12))
                    .p_2()
                    .text_xs()
                    .text_color(theme.text)
                    .child(error),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(create_branch_row(root.to_path_buf(), cx))
                .when(branches.is_empty(), |this| {
                    this.child(
                        div()
                            .rounded(rems(0.375))
                            .border_1()
                            .border_color(theme.border_subtle)
                            .p_3()
                            .text_sm()
                            .text_color(theme.text_subtle)
                            .child(if has_branches {
                                "No branches match your search"
                            } else {
                                "No branches"
                            }),
                    )
                })
                .when(!branches.is_empty(), |this| {
                    this.children(
                        branches
                            .into_iter()
                            .map(|branch| branch_row(root.to_path_buf(), branch, cx)),
                    )
                }),
        )
        .into_any_element()
}

fn create_branch_row<T: PaneDelegate + SettingsDelegate>(
    root: PathBuf,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let branch_name = cx
        .global::<GitUiState>()
        .branch_name
        .as_ref()
        .unwrap()
        .clone();
    div()
        .id("git-create-branch-row")
        .flex()
        .items_start()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_subtle)
        .p_2p5()
        .text_sm()
        .text_color(theme.text)
        .hover(move |this| this.bg(theme.bg_hover))
        .on_click(cx.listener(move |_, _, _, cx| {
            branch_name.update(cx, |input, cx| input.set_value("", cx));
            open_modal(root.clone(), GitModal::CreateBranch, cx);
        }))
        .child(
            div()
                .size(rems(1.25))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(rems(0.25))
                .bg(gpui::Hsla::from(theme.accent).opacity(0.14))
                .child(Icon::new(IconName::Add).size(14.0).color(theme.accent)),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_color(theme.text_emphasis).child("Create Branch"))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .child("Create a new local branch"),
                ),
        )
        .into_any_element()
}

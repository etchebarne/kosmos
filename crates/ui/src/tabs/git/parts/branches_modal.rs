fn render_git_modal(
    dialog: Dialog,
    window: &mut Window,
    cx: &mut App,
) -> Dialog {
    let modal = cx
        .try_global::<GitUiState>()
        .and_then(|state| Some((state.root.clone()?, state.modal?)));
    let Some((root, modal_state)) = modal else {
        return dialog.overlay(false).close_button(false);
    };

    let rem_size = window.rem_size();
    let viewport_size = window.viewport_size();
    let margin_top = viewport_size.height / 10.0;
    let width = rems(30.0)
        .to_pixels(rem_size)
        .min((viewport_size.width - rems(2.0).to_pixels(rem_size)).max(rems(16.0).to_pixels(rem_size)));
    let max_height =
        (viewport_size.height - margin_top - margin_top).max(rems(12.0).to_pixels(rem_size));

    match modal_state {
        GitModal::Branches => dialog
            .margin_top(margin_top)
            .w(width)
            .max_h(max_height)
            .overflow_hidden()
            .title("Git Branches")
            .footer(modal_footer(close_modal_button(cx), cx))
            .on_close(|_, _, cx| close_modal(cx))
            .child(branches_modal_body(&root, cx)),
        GitModal::CreateBranch => dialog
            .margin_top(margin_top)
            .w(width)
            .max_h(max_height)
            .overflow_hidden()
            .title("Create Branch")
            .footer(create_branch_modal_footer(&root, cx))
            .on_close(|_, _, cx| close_modal(cx))
            .child(create_branch_modal_body(cx))
            ,
        GitModal::Remotes => dialog
            .margin_top(margin_top)
            .w(width)
            .max_h(max_height)
            .overflow_hidden()
            .title("Git Remotes")
            .footer(modal_footer(close_modal_button(cx), cx))
            .on_close(|_, _, cx| close_modal(cx))
            .child(remotes_modal_body(&root, cx)),
        GitModal::Stashes => dialog
            .margin_top(margin_top)
            .w(width)
            .max_h(max_height)
            .overflow_hidden()
            .title("Git Stashes")
            .footer(modal_footer(close_modal_button(cx), cx))
            .on_close(|_, _, cx| close_modal(cx))
            .child(stashes_modal_body(&root, cx)),
        GitModal::Tags => dialog
            .margin_top(margin_top)
            .w(width)
            .max_h(max_height)
            .overflow_hidden()
            .title("Git Tags")
            .footer(modal_footer(close_modal_button(cx), cx))
            .on_close(|_, _, cx| close_modal(cx))
            .child(tags_modal_body(&root, cx)),
        GitModal::ConfirmDiscardSelected => {
            let selected_paths = selected_change_paths(cx);
            let selected_count = selected_paths.len();
            confirm_git_action_modal(
                dialog,
                ConfirmGitActionModal {
                    title: "Discard Selected Changes",
                    message: format!(
                        "This will permanently discard {selected_count} selected working tree change{}. This action cannot be undone.",
                        plural(selected_count)
                    ),
                    button_id: "git-confirm-discard-selected",
                    button_label: "Discard Selected",
                    danger: true,
                    confirm: Rc::new(move |_, window, cx| {
                        close_modal(cx);
                        window.close_dialog(cx);
                        run_git_action_app(
                            root.clone(),
                            {
                                let selected_paths = selected_paths.clone();
                                move |root| kosmos_git::discard_files(root, &selected_paths)
                            },
                            cx,
                        );
                    }),
                },
                margin_top,
                width,
                max_height,
                cx,
            )
        }
        GitModal::ConfirmDiscard => {
            confirm_git_action_modal(
                dialog,
                ConfirmGitActionModal {
                    title: "Discard All Changes",
                    message: "This will permanently discard all tracked and untracked working tree changes. This action cannot be undone.".to_string(),
                    button_id: "git-confirm-discard",
                    button_label: "Discard All",
                    danger: true,
                    confirm: Rc::new(move |_, window, cx| {
                        close_modal(cx);
                        window.close_dialog(cx);
                        run_git_action_app(root.clone(), kosmos_git::discard_all_changes, cx);
                    }),
                },
                margin_top,
                width,
                max_height,
                cx,
            )
        }
        GitModal::ConfirmResolveConflicts => {
            let (conflict_paths, stage_all_changes) = {
                let state = cx.global::<GitUiState>();
                (
                    state.pending_conflict_paths.clone(),
                    state.pending_conflict_resolution_stages_all,
                )
            };
            let conflict_count = conflict_paths.len();
            let (message, button_label) = if stage_all_changes {
                (
                    format!(
                        "This will stage all changes, including {conflict_count} conflicted file{}. Staging conflicted files tells Git the conflict{} resolved. Make sure the conflict markers are handled before continuing.",
                        plural(conflict_count),
                        plural(conflict_count)
                    ),
                    "Stage All",
                )
            } else {
                (
                    format!(
                        "This will stage {conflict_count} conflicted file{} and tell Git the conflict{} resolved. Make sure the conflict markers are handled before continuing.",
                        plural(conflict_count),
                        plural(conflict_count)
                    ),
                    "Mark Resolved",
                )
            };
            confirm_git_action_modal(
                dialog,
                ConfirmGitActionModal {
                    title: "Mark Conflicts Resolved",
                    message,
                    button_id: "git-confirm-resolve-conflicts",
                    button_label,
                    danger: false,
                    confirm: Rc::new(move |_, window, cx| {
                        close_modal(cx);
                        window.close_dialog(cx);
                        if stage_all_changes {
                            run_git_action_app(root.clone(), kosmos_git::stage_all, cx);
                        } else {
                            run_git_action_app(
                                root.clone(),
                                {
                                    let conflict_paths = conflict_paths.clone();
                                    move |root| kosmos_git::stage_files(root, &conflict_paths)
                                },
                                cx,
                            );
                        }
                    }),
                },
                margin_top,
                width,
                max_height,
                cx,
            )
        }
    }
}

struct ConfirmGitActionModal {
    title: &'static str,
    message: String,
    button_id: &'static str,
    button_label: &'static str,
    danger: bool,
    confirm: PopupMenuHandler,
}

fn confirm_git_action_modal(
    dialog: Dialog,
    config: ConfirmGitActionModal,
    margin_top: Pixels,
    width: Pixels,
    max_height: Pixels,
    cx: &mut App,
) -> Dialog
{
    let _ = cx;
    let confirm = config.confirm.clone();
    dialog
        .margin_top(margin_top)
        .w(width)
        .max_h(max_height)
        .overflow_hidden()
        .title(config.title)
        .footer(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(close_modal_button(cx))
                .child(action_button(
                    config.button_id,
                    config.button_label,
                    config.danger,
                    move |event, window, cx| confirm(event, window, cx),
                    cx,
                )),
        )
        .on_close(|_, _, cx| close_modal(cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .child(config.message),
        )
}

fn branches_modal_body(root: &Path, cx: &mut App) -> AnyElement {
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
            this.child(error_alert("git-branches-error", error))
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

fn create_branch_row(root: PathBuf, cx: &mut App) -> AnyElement {
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
        .on_click(move |_, window, cx| {
            branch_name.update(cx, |input, cx| input.set_value("", window, cx));
            open_modal_app(root.clone(), GitModal::CreateBranch, cx);
        })
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

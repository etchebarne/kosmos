fn branch_row(root: PathBuf, branch: Branch, cx: &mut App) -> AnyElement {
    let theme = *cx.theme();
    let name = branch.name.clone();
    let is_current = branch.current;
    let is_remote = branch.remote;
    let row_id = SharedString::from(format!("git-branch:{name}"));
    let delete_id = SharedString::from(format!("git-delete-branch:{name}"));
    let switch_branch = name.clone();
    let delete_branch = name.clone();
    let root_switch = root.clone();
    let root_delete = root.clone();

    div()
        .id(row_id)
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(if is_current {
            gpui::Hsla::from(theme.accent)
        } else {
            gpui::Hsla::from(theme.border_subtle)
        })
        .p_2p5()
        .text_sm()
        .text_color(if is_current {
            theme.text_emphasis
        } else {
            theme.text
        })
        .when(!is_current, |this| {
            this.hover(move |this| this.bg(theme.bg_hover))
                .on_click(move |_, _, cx| {
                    let branch = switch_branch.clone();
                    run_modal_action_app(
                        root_switch.clone(),
                        GitModal::Branches,
                        move |root| {
                            if is_remote {
                                kosmos_git::switch_remote_branch(root, &branch)
                            } else {
                                kosmos_git::switch_branch(root, &branch)
                            }
                        },
                        cx,
                    );
                })
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_start()
                .gap_2()
                .child(
                    div().mt(rems(0.1875)).child(
                        Icon::new(IconName::SourceControl)
                            .size(14.0)
                            .color(theme.text_muted),
                    ),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(name),
                        )
                        .when(is_remote, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.text_subtle)
                                    .child("Remote"),
                            )
                        }),
                ),
        )
        .when(!is_current && !is_remote, |this| {
            this.child(div().flex_none().child(delete_button(
                delete_id,
                move |_, _, cx| {
                    let branch = delete_branch.clone();
                    run_modal_action_app(
                        root_delete.clone(),
                        GitModal::Branches,
                        move |root| kosmos_git::delete_branch(root, &branch),
                        cx,
                    );
                },
                cx,
            )))
        })
        .into_any_element()
}

fn create_branch_modal_body(cx: &mut App) -> AnyElement {
    let (branch_name, last_error) = {
        let state = cx.global::<GitUiState>();
        (
            state.branch_name.as_ref().unwrap().clone(),
            state.last_error.clone(),
        )
    };
    let theme = *cx.theme();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Branch Name", branch_name))
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
        .into_any_element()
}

fn create_branch_modal_footer(root: &Path, cx: &mut App) -> AnyElement {
    let branch_name = cx
        .global::<GitUiState>()
        .branch_name
        .as_ref()
        .unwrap()
        .clone();
    let root_cancel = root.to_path_buf();
    let root_create = root.to_path_buf();
    let cancel_input = branch_name.clone();
    let create_input = branch_name.clone();

    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(action_button(
            "git-cancel-create-branch",
            "Cancel",
            false,
            move |_, _, cx| {
                cancel_input.update(cx, |input, cx| input.set_value("", cx));
                open_modal_app(root_cancel.clone(), GitModal::Branches, cx);
            },
            cx,
        ))
        .child(action_button(
            "git-confirm-create-branch",
            "Create",
            false,
            move |_, _, cx| {
                let branch = create_input.read(cx).value().trim().to_string();
                if branch.is_empty() {
                    return;
                }
                let input = create_input.clone();
                run_modal_action_after_success_app(
                    root_create.clone(),
                    GitModal::Branches,
                    move |root| kosmos_git::create_branch(root, &branch),
                    move |cx| {
                        input.update(cx, |input, cx| input.set_value("", cx));
                        cx.update_global::<GitUiState, _>(|state, _| {
                            state.modal = Some(GitModal::Branches)
                        });
                    },
                    cx,
                );
            },
            cx,
        ))
        .into_any_element()
}

fn remotes_modal_body(root: &Path, cx: &mut App) -> AnyElement {
    let (name, url, remotes) = {
        let state = cx.global::<GitUiState>();
        (
            state.remote_name.as_ref().unwrap().clone(),
            state.remote_url.as_ref().unwrap().clone(),
            state.remotes.clone(),
        )
    };
    let root_add = root.to_path_buf();
    let theme = *cx.theme();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Remote Name", name.clone()))
        .child(input_row("Remote URL", url.clone()))
        .child(div().flex().justify_end().child(action_button(
            "git-add-remote",
            "Add Remote",
            false,
            move |_, _, cx| {
                let name_value = name.read(cx).value().to_string();
                let url_value = url.read(cx).value().to_string();
                if name_value.trim().is_empty() || url_value.trim().is_empty() {
                    return;
                }
                run_modal_action_app(
                    root_add.clone(),
                    GitModal::Remotes,
                    move |root| kosmos_git::add_remote(root, name_value.trim(), url_value.trim()),
                    cx,
                );
            },
            cx,
        )))
        .child(section_label("Existing Remotes", theme))
        .children(
            remotes
                .into_iter()
                .map(|remote| remote_row(root.to_path_buf(), remote, cx)),
        )
        .into_any_element()
}

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
        .p_2p5()
        .text_sm()
        .text_color(if is_current {
            theme.text_emphasis
        } else {
            theme.text
        })
        .when(is_current, |this| {
            this.bg(gpui::Hsla::from(theme.accent).opacity(if theme.is_dark {
                0.16
            } else {
                0.12
            }))
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
                .items_center()
                .gap_2()
                .child(
                    div().flex_none().flex().items_center().child(
                        Icon::new(IconName::SourceControl)
                            .size(14.0)
                            .color(theme.text_muted),
                    ),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(name),
                        )
                        .when(is_remote, |this| {
                            this.child(
                                div().flex_none().child(
                                    ComponentTag::secondary()
                                        .with_size(Size::Small)
                                        .border_0()
                                        .child("Remote"),
                                ),
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
    let branch_name = cx
        .global::<GitUiState>()
        .branch_name
        .as_ref()
        .unwrap()
        .clone();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Branch Name", branch_name))
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
            move |_, window, cx| {
                cancel_input.update(cx, |input, cx| input.set_value("", window, cx));
                open_modal_app(root_cancel.clone(), GitModal::Branches, cx);
            },
            cx,
        ))
        .child(
            Button::new("git-confirm-create-branch")
                .primary()
                .label("Create")
                .on_click(move |_, _, cx| {
                    let branch = create_input.read(cx).value().trim().to_string();
                    if branch.is_empty() {
                        return;
                    }
                    run_modal_action_after_success_app(
                        root_create.clone(),
                        GitModal::Branches,
                        move |root| kosmos_git::create_branch(root, &branch),
                        move |cx| {
                            cx.update_global::<GitUiState, _>(|state, _| {
                                state.modal = Some(GitModal::Branches)
                            });
                        },
                        cx,
                    );
                }),
        )
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

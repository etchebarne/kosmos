fn tags_modal_body(root: &Path, cx: &mut App) -> AnyElement {
    let (name, message, sha, tags) = {
        let state = cx.global::<GitUiState>();
        (
            state.tag_name.as_ref().unwrap().clone(),
            state.tag_message.as_ref().unwrap().clone(),
            state.tag_sha.as_ref().unwrap().clone(),
            state.tags.clone(),
        )
    };
    let root_add = root.to_path_buf();
    let theme = *cx.theme();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Tag Name", name.clone()))
        .child(input_row("Tag Message (optional)", message.clone()))
        .child(input_row("Commit SHA (optional)", sha.clone()))
        .child(div().flex().justify_end().child(action_button(
            "git-add-tag",
            "Add Tag",
            false,
            move |_, _, cx| {
                let name_value = name.read(cx).value().to_string();
                if name_value.trim().is_empty() {
                    return;
                }
                let message_value = message.read(cx).value().to_string();
                let sha_value = sha.read(cx).value().to_string();
                run_modal_action_app(
                    root_add.clone(),
                    GitModal::Tags,
                    move |root| {
                        kosmos_git::add_tag(
                            root,
                            name_value.trim(),
                            Some(message_value.trim()),
                            Some(sha_value.trim()),
                        )
                    },
                    cx,
                );
            },
            cx,
        )))
        .child(section_label("Existing Tags", theme))
        .children(tags.into_iter().map(|tag| tag_row(root.to_path_buf(), tag, cx)))
        .into_any_element()
}

fn stashes_modal_body(root: &Path, cx: &mut App) -> AnyElement {
    let (stashes, expanded) = {
        let state = cx.global::<GitUiState>();
        (state.stashes.clone(), state.expanded_stashes.clone())
    };
    let theme = *cx.theme();

    let mut body = div().flex().flex_col().gap_2();
    if stashes.is_empty() {
        body = body.child(
            div()
                .rounded(rems(0.375))
                .border_1()
                .border_color(theme.border_subtle)
                .p_3()
                .text_sm()
                .text_color(theme.text_subtle)
                .child("No stashes"),
        );
    } else {
        body = body.children(
            stashes
                .into_iter()
                .map(|stash| stash_row(root.to_path_buf(), stash, expanded.clone(), cx)),
        );
    }
    body.into_any_element()
}

fn stash_row(
    root: PathBuf,
    stash: Stash,
    expanded: std::collections::HashSet<String>,
    cx: &mut App,
) -> AnyElement {
    let theme = *cx.theme();
    let is_expanded = expanded.contains(&stash.id);
    let toggle_id = stash.id.clone();
    let apply_id = stash.id.clone();
    let delete_id = stash.id.clone();
    let root_apply = root.clone();
    let root_delete = root.clone();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_subtle)
        .p_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap_2()
                        .on_mouse_down(
                            MouseButton::Left,
                            move |_, _, cx| {
                                toggle_stash(&toggle_id, cx);
                            },
                        )
                        .child(
                            div().mt(rems(0.1875)).child(
                                Icon::new(if is_expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
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
                                        .text_sm()
                                        .text_color(theme.text)
                                        .child(stash.id.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.text_subtle)
                                        .child(stash.message.clone()),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(icon_action_button(
                            SharedString::from(format!("git-apply-stash:{}", stash.id)),
                            IconName::ArrowDown,
                            theme.text_muted,
                            move |_, _, cx| {
                                run_modal_action_app(
                                    root_apply.clone(),
                                    GitModal::Stashes,
                                    {
                                        let apply_id = apply_id.clone();
                                        move |root| kosmos_git::apply_stash(root, &apply_id)
                                    },
                                    cx,
                                );
                            },
                            cx,
                        ))
                        .child(delete_button(
                            SharedString::from(format!("git-delete-stash:{}", stash.id)),
                            move |_, _, cx| {
                                run_modal_action_app(
                                    root_delete.clone(),
                                    GitModal::Stashes,
                                    {
                                        let delete_id = delete_id.clone();
                                        move |root| kosmos_git::delete_stash(root, &delete_id)
                                    },
                                    cx,
                                );
                            },
                            cx,
                        )),
                ),
        )
        .when(is_expanded, |this| {
            this.child(
                div().ml_6().flex().flex_col().gap_1().children(
                    stash
                        .files
                        .into_iter()
                        .map(|file| div().text_xs().text_color(theme.text_subtle).child(file)),
                ),
            )
        })
        .into_any_element()
}

fn input_row(label: &'static str, input: Entity<TextInput>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(label))
        .child(input)
        .into_any_element()
}

fn section_label(label: &'static str, theme: theme::Theme) -> AnyElement {
    div()
        .pt_2()
        .text_xs()
        .text_color(theme.text_subtle)
        .child(label)
        .into_any_element()
}

fn remote_row(root: PathBuf, remote: Remote, cx: &mut App) -> AnyElement {
    let name = remote.name.clone();
    list_row(
        remote.name,
        remote.url,
        move |_, _, cx| {
            let name = name.clone();
            run_modal_action_app(
                root.clone(),
                GitModal::Remotes,
                move |root| kosmos_git::delete_remote(root, &name),
                cx,
            );
        },
        cx,
    )
}

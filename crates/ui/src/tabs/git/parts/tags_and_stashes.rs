fn tags_modal_body(root: &Path, cx: &mut App) -> AnyElement {
    let (tag_search, tags) = {
        let state = cx.global::<GitUiState>();
        (
            state.tag_search.as_ref().unwrap().clone(),
            state.tags.clone(),
        )
    };
    let theme = *cx.theme();
    let query = tag_search.read(cx).value().trim().to_lowercase();
    let has_tags = !tags.is_empty();
    let tags = if query.is_empty() {
        tags
    } else {
        tags
            .into_iter()
            .filter(|tag| {
                tag.name.to_lowercase().contains(&query)
                    || tag.message.to_lowercase().contains(&query)
            })
            .collect()
    };

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(tag_search_row(root.to_path_buf(), tag_search, theme, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .when(tags.is_empty(), |this| {
                    this.child(
                        div()
                            .rounded(rems(0.375))
                            .p_3()
                            .text_sm()
                            .text_color(theme.text_subtle)
                            .child(if has_tags {
                                "No tags match your search"
                            } else {
                                "No tags"
                            }),
                    )
                })
                .when(!tags.is_empty(), |this| {
                    this.children(tags.into_iter().map(|tag| tag_row(root.to_path_buf(), tag, cx)))
                }),
        )
        .into_any_element()
}

fn tag_search_row(
    root: PathBuf,
    input: Entity<InputState>,
    theme: theme::Theme,
    cx: &mut App,
) -> AnyElement {
    let (tag_name, tag_message, tag_sha) = {
        let state = cx.global::<GitUiState>();
        (
            state.tag_name.as_ref().unwrap().clone(),
            state.tag_message.as_ref().unwrap().clone(),
            state.tag_sha.as_ref().unwrap().clone(),
        )
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        .mt(rems(0.25))
        .child(div().flex_1().min_w_0().child(search_input(input, theme)))
        .child(
            Button::new("git-new-tag")
                .primary()
                .label("New")
                .on_click(move |_, window, cx| {
                    tag_name.update(cx, |input, cx| input.set_value("", window, cx));
                    tag_message.update(cx, |input, cx| input.set_value("", window, cx));
                    tag_sha.update(cx, |input, cx| input.set_value("", window, cx));
                    open_modal_app(root.clone(), GitModal::CreateTag, cx);
                }),
        )
        .into_any_element()
}

fn create_tag_modal_body(cx: &mut App) -> AnyElement {
    let (name, message, sha) = {
        let state = cx.global::<GitUiState>();
        (
            state.tag_name.as_ref().unwrap().clone(),
            state.tag_message.as_ref().unwrap().clone(),
            state.tag_sha.as_ref().unwrap().clone(),
        )
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(input_row("Tag Name", name))
        .child(input_row("Tag Message (optional)", message))
        .child(input_row("Commit SHA (optional)", sha))
        .into_any_element()
}

fn create_tag_modal_footer(root: &Path, cx: &mut App) -> AnyElement {
    let (name, message, sha) = {
        let state = cx.global::<GitUiState>();
        (
            state.tag_name.as_ref().unwrap().clone(),
            state.tag_message.as_ref().unwrap().clone(),
            state.tag_sha.as_ref().unwrap().clone(),
        )
    };
    let root_cancel = root.to_path_buf();
    let root_create = root.to_path_buf();
    let cancel_name = name.clone();
    let cancel_message = message.clone();
    let cancel_sha = sha.clone();
    let create_name = name.clone();
    let create_message = message.clone();
    let create_sha = sha.clone();

    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(action_button(
            "git-cancel-create-tag",
            "Cancel",
            false,
            move |_, window, cx| {
                cancel_name.update(cx, |input, cx| input.set_value("", window, cx));
                cancel_message.update(cx, |input, cx| input.set_value("", window, cx));
                cancel_sha.update(cx, |input, cx| input.set_value("", window, cx));
                open_modal_app(root_cancel.clone(), GitModal::Tags, cx);
            },
            cx,
        ))
        .child(
            Button::new("git-confirm-create-tag")
                .primary()
                .label("Create")
                .on_click(move |_, _, cx| {
                    let name_value = create_name.read(cx).value().trim().to_string();
                    if name_value.is_empty() {
                        return;
                    }
                    let message_value = create_message.read(cx).value().trim().to_string();
                    let sha_value = create_sha.read(cx).value().trim().to_string();
                    run_modal_action_after_success_app(
                        root_create.clone(),
                        GitModal::Tags,
                        move |root| {
                            kosmos_git::add_tag(
                                root,
                                &name_value,
                                Some(message_value.as_str()),
                                Some(sha_value.as_str()),
                            )
                        },
                        move |cx| {
                            cx.update_global::<GitUiState, _>(|state, _| {
                                state.modal = Some(GitModal::Tags)
                            });
                        },
                        cx,
                    );
                }),
        )
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
        .p_2()
        .hover(move |this| this.bg(theme.bg_hover))
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
                        .map(|file| stash_file_row(file, theme)),
                ),
            )
        })
        .into_any_element()
}

fn stash_file_row(file: kosmos_git::StashFile, theme: theme::Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .text_xs()
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(theme.text_subtle)
                .child(file.path),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .child(diff_stat_text(format!("+{}", file.insertions), theme.success))
                .child(diff_stat_text(format!("-{}", file.deletions), theme.danger)),
        )
        .into_any_element()
}

fn input_row(label: &'static str, input: Entity<InputState>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(label))
        .child(Input::new(&input).bordered(false))
        .into_any_element()
}

fn search_input(input: Entity<InputState>, theme: theme::Theme) -> AnyElement {
    Input::new(&input)
        .bordered(false)
        .cleanable(true)
        .prefix(
            component_icon(IconName::Search)
                .small()
                .text_color(gpui::Hsla::from(theme.text_muted)),
        )
        .w_full()
        .into_any_element()
}

fn remote_row(root: PathBuf, remote: Remote, cx: &mut App) -> AnyElement {
    let name = remote.name.clone();
    list_row(
        remote.name,
        remote.url,
        true,
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

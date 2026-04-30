use gpui::{AnyElement, Context, IntoElement, div, prelude::*, rems};

use file_tree::ActiveFileTree;
use icons::{Icon, IconName};
use kosmos_git::RepositorySummary;
use tabs::registry;
use theme::ActiveTheme;

use crate::delegate::{PaneDelegate, SettingsDelegate};

pub fn render<T: PaneDelegate + SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let theme = *cx.theme();
    let Some(root) = cx
        .file_tree()
        .and_then(|tree| tree.read(cx).root().map(|path| path.to_path_buf()))
    else {
        return empty_state("No workspace open", cx);
    };

    let summary = match RepositorySummary::discover(&root) {
        Ok(summary) => summary,
        Err(error) => return error_state(error.to_string(), cx),
    };

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(theme.bg_surface)
        .text_color(theme.text)
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(theme.border_subtle)
                .px_3()
                .py_2()
                .child(
                    Icon::new(registry::GIT.icon)
                        .size(16.0)
                        .color(theme.text_muted),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.text_emphasis)
                        .child(registry::GIT.name),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .child(summary_card(&summary, cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .child("Basic Git integration is wired through crates/git using gix."),
                ),
        )
        .into_any_element()
}

fn summary_card<T: PaneDelegate + SettingsDelegate>(
    summary: &RepositorySummary,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let branch = summary.branch.as_deref().unwrap_or("Detached HEAD");
    let status = if summary.is_clean() {
        "Working tree clean".to_string()
    } else {
        format!(
            "{} changed item{}",
            summary.changes,
            plural(summary.changes)
        )
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .rounded(rems(0.5))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_elevated)
        .p_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Icon::new(IconName::SourceControl)
                                .size(14.0)
                                .color(theme.text_muted),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.text_emphasis)
                                .child(branch.to_string()),
                        ),
                )
                .child(
                    div()
                        .rounded(rems(0.25))
                        .bg(if summary.is_clean() {
                            gpui::Hsla::from(theme.accent).opacity(0.14)
                        } else {
                            gpui::Hsla::from(theme.danger).opacity(0.14)
                        })
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(if summary.is_clean() {
                            theme.accent
                        } else {
                            theme.danger
                        })
                        .child(status),
                ),
        )
        .child(detail_row(
            "Repository",
            summary.work_dir.display().to_string(),
            cx,
        ))
        .child(detail_row(
            "Git directory",
            summary.git_dir.display().to_string(),
            cx,
        ))
        .into_any_element()
}

fn detail_row<T: PaneDelegate + SettingsDelegate>(
    label: &'static str,
    value: String,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(theme.text_subtle).child(label))
        .child(div().text_sm().text_color(theme.text).child(value))
        .into_any_element()
}

fn empty_state<T: PaneDelegate + SettingsDelegate>(
    message: &'static str,
    cx: &mut Context<T>,
) -> AnyElement {
    centered_state(registry::GIT.icon, message.to_string(), cx)
}

fn error_state<T: PaneDelegate + SettingsDelegate>(
    message: String,
    cx: &mut Context<T>,
) -> AnyElement {
    centered_state(registry::GIT.icon, message, cx)
}

fn centered_state<T: PaneDelegate + SettingsDelegate>(
    icon: IconName,
    message: String,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(theme.bg_surface)
        .text_color(theme.text_subtle)
        .child(Icon::new(icon).size(28.0).color(theme.text_muted))
        .child(div().text_sm().child(message))
        .into_any_element()
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

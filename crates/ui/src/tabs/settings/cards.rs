use gpui::{
    AnyElement, Context, ElementId, IntoElement, MouseButton, SharedString, div, prelude::*, rems,
};

use registry::{RegistryEntry, ToolKind};
use theme::ActiveTheme;

use crate::delegate::{ActiveSettingsUi, SettingsDelegate};

pub fn render_marketplace<T: SettingsDelegate>(kind: ToolKind, cx: &mut Context<T>) -> AnyElement {
    let cards: Vec<AnyElement> = registry::by_kind(kind)
        .map(|entry| render_tool_card(entry, cx))
        .collect();
    div()
        .flex()
        .flex_col()
        .gap_3()
        .min_w_0()
        .children(cards)
        .into_any_element()
}

fn render_tool_card<T: SettingsDelegate>(
    entry: &'static RegistryEntry,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let installed = installer::is_installed(entry);
    let installing = cx.settings_ui().installing.contains(entry.id);
    let error = cx.settings_ui().install_errors.get(entry.id).cloned();

    let names: Vec<&'static str> = entry
        .languages
        .iter()
        .filter_map(|id| language::info(id).map(|i| i.name))
        .collect();

    let kinds: SharedString = entry
        .kinds
        .iter()
        .map(|k| match k {
            ToolKind::Lsp => "LSP",
            ToolKind::Formatter => "Formatter",
            ToolKind::Linter => "Linter",
        })
        .collect::<Vec<_>>()
        .join(" · ")
        .into();

    let action = install_action(entry.id, installed, installing, theme, cx);

    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_lg()
                        .text_color(theme.text_emphasis)
                        .child(entry.id),
                )
                .child(div().text_xs().text_color(theme.text_subtle).child(kinds)),
        )
        .child(action);

    let supports_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_x_2()
        .gap_y_1()
        .min_w_0()
        .child(
            div()
                .text_xs()
                .text_color(theme.text_subtle)
                .child("Supports:"),
        )
        .children(names.into_iter().map(|name| {
            div()
                .px_2()
                .rounded(rems(0.25))
                .bg(theme.bg_elevated)
                .text_xs()
                .text_color(theme.text_muted)
                .child(name)
        }));

    let mut card = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(rems(0.5))
        .bg(theme.bg_surface)
        .border_1()
        .border_color(theme.border_subtle)
        .min_w_0()
        .w_full()
        .overflow_hidden()
        .child(header)
        .child(supports_row);

    if let Some(err) = error {
        card = card.child(div().text_xs().text_color(theme.danger).child(err));
    }

    card.into_any_element()
}

fn install_action<T: SettingsDelegate>(
    tool_id: &'static str,
    installed: bool,
    installing: bool,
    theme: theme::Theme,
    cx: &mut Context<T>,
) -> AnyElement {
    if installing {
        return div()
            .px_3()
            .py_1()
            .rounded(rems(0.375))
            .border_1()
            .border_color(theme.border_subtle)
            .text_xs()
            .text_color(theme.text_subtle)
            .child("Installing…")
            .into_any_element();
    }

    let entry = registry::get(tool_id).expect("tool id must exist in registry");
    let label = if installed { "Remove" } else { "Install" };
    let resting_text = if installed {
        theme.text_muted
    } else {
        theme.accent
    };

    let base = div()
        .id(ElementId::Name(format!("install-{tool_id}").into()))
        .px_3()
        .py_1()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg_elevated)
        .text_xs()
        .text_color(resting_text);

    let styled = if installed {
        base.hover(move |this| {
            this.bg(theme.bg_hover)
                .border_color(theme.danger)
                .text_color(theme.danger)
        })
    } else {
        base.hover(move |this| {
            this.bg(theme.bg_hover)
                .border_color(theme.accent)
                .text_color(theme.accent)
        })
    };

    styled
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| {
            if installed {
                this.uninstall_tool(entry, cx);
            } else {
                this.install_tool(entry, cx);
            }
        }))
        .child(label)
        .into_any_element()
}

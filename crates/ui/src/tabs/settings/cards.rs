use gpui::{
    AnyElement, Context, ElementId, IntoElement, MouseButton, SharedString, div, prelude::*, rems,
};

use registry::{RegistryEntry, ToolKind};
use settings::{ActiveSettings, SettingValue};
use theme::ActiveTheme;

use crate::components::{DropdownOption, MultiSelect};
use crate::delegate::{ActiveSettingsUi, SettingsDelegate};

pub fn render_languages<T: SettingsDelegate>(cx: &mut Context<T>) -> AnyElement {
    let cards: Vec<AnyElement> = language::ALL
        .iter()
        .filter(|info| has_any_tool(info.id))
        .map(|info| render_language_card(info, cx))
        .collect();
    div().flex().flex_col().gap_3().children(cards).into_any_element()
}

fn has_any_tool(language_id: &'static str) -> bool {
    registry::all().iter().any(|e| {
        (e.kinds.contains(&ToolKind::Formatter) || e.kinds.contains(&ToolKind::Linter))
            && e.languages.iter().any(|l| *l == language_id)
    })
}

pub fn render_marketplace<T: SettingsDelegate>(
    kind: ToolKind,
    cx: &mut Context<T>,
) -> AnyElement {
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

fn render_language_card<T: SettingsDelegate>(
    info: &'static language::LanguageInfo,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();

    let formatters: Vec<&'static RegistryEntry> =
        registry::for_language(info.id, ToolKind::Formatter).collect();
    let linters: Vec<&'static RegistryEntry> =
        registry::for_language(info.id, ToolKind::Linter).collect();

    let mut body: Vec<AnyElement> = Vec::new();

    if !formatters.is_empty() {
        body.push(section_header("Formatters", theme));
        body.push(render_picker_row(
            info.id,
            "formatters",
            ToolKind::Formatter,
            &formatters,
            true,
            cx,
        ));
    }
    if !linters.is_empty() {
        body.push(section_header("Linters", theme));
        body.push(render_picker_row(
            info.id,
            "linters",
            ToolKind::Linter,
            &linters,
            false,
            cx,
        ));
    }

    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(rems(0.5))
        .bg(theme.bg_surface)
        .border_1()
        .border_color(theme.border_subtle)
        .child(
            div()
                .text_lg()
                .text_color(theme.text_emphasis)
                .child(info.name),
        )
        .children(body)
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
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_subtle)
                        .child(kinds),
                ),
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
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.danger)
                .child(err),
        );
    }

    card.into_any_element()
}

fn render_picker_row<T: SettingsDelegate>(
    language_id: &'static str,
    suffix: &'static str,
    _kind: ToolKind,
    candidates: &[&'static RegistryEntry],
    ordered: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let setting_key = setting_key(language_id, suffix);

    let opts: Vec<DropdownOption> = candidates
        .iter()
        .copied()
        .filter(|e| installer::is_installed(e))
        .map(|e| DropdownOption::new(e.id, e.id))
        .collect();

    if opts.is_empty() {
        let theme = *cx.theme();
        return div()
            .text_xs()
            .text_color(theme.text_subtle)
            .child("None installed yet.")
            .into_any_element();
    }

    let current: Vec<SharedString> = cx
        .settings()
        .get(setting_key)
        .and_then(|v| v.as_list().map(|l| l.to_vec()))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            SettingValue::String(s) => Some(s),
            _ => None,
        })
        .collect();

    let open_dropdown = cx.settings_ui().open_dropdown;

    MultiSelect::new(
        SharedString::from(format!("picker-{setting_key}")),
        current,
        opts,
    )
    .ordered(ordered)
    .open(open_dropdown == Some(setting_key))
    .on_toggle(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
        this.toggle_settings_dropdown(setting_key, cx);
    }))
    .on_change(cx.listener(move |this, new_value: &Vec<SharedString>, _, cx| {
        let list = new_value
            .iter()
            .map(|s| SettingValue::String(s.clone()))
            .collect();
        this.set_setting_value(setting_key, SettingValue::List(list), cx);
    }))
    .into_any_element()
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
    let resting_text = if installed { theme.text_muted } else { theme.accent };

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

fn section_header(label: &'static str, theme: theme::Theme) -> AnyElement {
    div()
        .pt_1()
        .text_xs()
        .text_color(theme.text_subtle)
        .child(label)
        .into_any_element()
}

fn setting_key(language_id: &'static str, suffix: &'static str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(&'static str, &'static str), &'static str>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(s) = guard.get(&(language_id, suffix)) {
        return s;
    }
    let leaked: &'static str =
        Box::leak(format!("languages.{language_id}.{suffix}").into_boxed_str());
    guard.insert((language_id, suffix), leaked);
    leaked
}

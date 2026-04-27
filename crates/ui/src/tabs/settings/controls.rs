use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, rems};

use settings::{Setting, SettingControl, SettingValue};
use theme::ActiveTheme;

use crate::components::{Dropdown, DropdownOption, MultiSelect, NumericInput, Switch};
use crate::delegate::SettingsDelegate;
use crate::tabs::settings::state::ActiveSettingsInputs;

pub fn render<T: SettingsDelegate>(
    setting: &'static Setting,
    value: &SettingValue,
    open_dropdown: Option<&'static str>,
    cx: &mut Context<T>,
) -> AnyElement {
    let setting_id = setting.id;
    match &setting.control {
        SettingControl::Switch { .. } => Switch::new(
            make_id("setting-switch", setting_id),
            value.as_bool().unwrap_or(false),
        )
        .on_change(cx.listener(move |this, new_value: &bool, _, cx| {
            this.set_setting_value(setting_id, SettingValue::Bool(*new_value), cx);
        }))
        .into_any_element(),

        SettingControl::Number {
            min, max, step, ..
        } => {
            let mut input =
                NumericInput::new(format!("setting-num:{setting_id}"), value.as_int().unwrap_or(0))
                    .step(*step)
                    .on_change(cx.listener(move |this, new_value: &i64, _, cx| {
                        this.set_setting_value(setting_id, SettingValue::Int(*new_value), cx);
                    }));
            if let Some(min) = min {
                input = input.min(*min);
            }
            if let Some(max) = max {
                input = input.max(*max);
            }
            input.into_any_element()
        }

        SettingControl::Dropdown { options, .. } => {
            let opts: Vec<DropdownOption> = options
                .iter()
                .map(|o| DropdownOption::new(o.id, o.label))
                .collect();
            let current = value.as_str().unwrap_or("").to_string();
            Dropdown::new(format!("setting-dropdown:{setting_id}"), current, opts)
                .open(open_dropdown == Some(setting_id))
                .on_toggle(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.toggle_settings_dropdown(setting_id, cx);
                }))
                .on_select(cx.listener(move |this, value: &SharedString, _, cx| {
                    this.set_setting_value(
                        setting_id,
                        SettingValue::String(value.clone()),
                        cx,
                    );
                }))
                .into_any_element()
        }

        SettingControl::Input { placeholder, .. } => {
            let theme = *cx.theme();
            match cx.settings_inputs().get(setting_id) {
                Some(entity) => div()
                    .min_w(rems(13.75))
                    .child(entity)
                    .into_any_element(),
                None => div()
                    .h(rems(1.75))
                    .min_w(rems(13.75))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(rems(0.3125))
                    .bg(theme.bg_elevated)
                    .border_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.text_subtle)
                    .child(placeholder.unwrap_or(""))
                    .into_any_element(),
            }
        }

        SettingControl::MultiSelect {
            options, ordered, ..
        } => {
            let opts: Vec<DropdownOption> = options()
                .iter()
                .map(|o| DropdownOption::new(o.id, o.label))
                .collect();
            let current: Vec<SharedString> = value
                .as_list()
                .map(|l| {
                    l.iter()
                        .filter_map(|v| match v {
                            SettingValue::String(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            MultiSelect::new(format!("setting-multi:{setting_id}"), current, opts)
                .ordered(*ordered)
                .open(open_dropdown == Some(setting_id))
                .on_toggle(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.toggle_settings_dropdown(setting_id, cx);
                }))
                .on_change(cx.listener(move |this, new_value: &Vec<SharedString>, _, cx| {
                    let list = new_value
                        .iter()
                        .map(|s| SettingValue::String(s.clone()))
                        .collect();
                    this.set_setting_value(setting_id, SettingValue::List(list), cx);
                }))
                .into_any_element()
        }
    }
}

fn make_id(prefix: &str, id: &str) -> SharedString {
    SharedString::from(format!("{prefix}:{id}"))
}

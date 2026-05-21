use gpui::{Anchor, AnyElement, Context, IntoElement, SharedString, div, prelude::*, rems};
use gpui_component::{
    button::Button,
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
};

use settings::{Setting, SettingControl, SettingValue, Settings};
use theme::ActiveTheme;

use crate::components::{DropdownOption, MultiSelect, NumericInput, left_aligned_button_label};
use crate::delegate::SettingsDelegate;
use crate::tabs::settings::state::ActiveSettingsInputs;

const SETTING_DROPDOWN_WIDTH_REM: f32 = 11.25;

pub fn render<T: SettingsDelegate>(
    setting: &'static Setting,
    value: &SettingValue,
    open_dropdown: Option<&'static str>,
    cx: &mut Context<T>,
) -> AnyElement {
    let setting_id = setting.id;
    match &setting.control {
        SettingControl::Switch { .. } => Switch::new(make_id("setting-switch", setting_id))
            .checked(value.as_bool().unwrap_or(false))
            .on_click(cx.listener(move |this, new_value: &bool, _, cx| {
                this.set_setting_value(setting_id, SettingValue::Bool(*new_value), cx);
            }))
            .into_any_element(),

        SettingControl::Number { min, max, step, .. } => {
            let mut input = NumericInput::new(
                format!("setting-num:{setting_id}"),
                value.as_int().unwrap_or(0),
            )
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
            let current = SharedString::from(value.as_str().unwrap_or(""));
            let label = options
                .iter()
                .find(|option| option.id == current.as_ref())
                .map(|option| option.label)
                .unwrap_or(current.as_ref());

            Button::new(make_id("setting-dropdown", setting_id))
                .outline()
                .child(left_aligned_button_label(label))
                .dropdown_caret(true)
                .w(rems(SETTING_DROPDOWN_WIDTH_REM))
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, _| {
                    let menu_width = rems(SETTING_DROPDOWN_WIDTH_REM).to_pixels(window.rem_size());
                    options
                        .iter()
                        .fold(menu.min_w(menu_width).max_w(menu_width), |menu, option| {
                            let checked = option.id == current.as_ref();
                            menu.item(PopupMenuItem::new(option.label).checked(checked).on_click(
                                move |_, _, cx| {
                                    cx.update_global::<Settings, _>(|settings, _| {
                                        settings.set(
                                            setting_id,
                                            SettingValue::String(option.id.into()),
                                        );
                                    });
                                    cx.refresh_windows();
                                },
                            ))
                        })
                })
                .into_any_element()
        }

        SettingControl::Input { placeholder, .. } => {
            let theme = *cx.theme();
            match cx.settings_inputs().get(setting_id) {
                Some(entity) => div().min_w(rems(13.75)).child(entity).into_any_element(),
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
                .on_change(
                    cx.listener(move |this, new_value: &Vec<SharedString>, _, cx| {
                        let list = new_value
                            .iter()
                            .map(|s| SettingValue::String(s.clone()))
                            .collect();
                        this.set_setting_value(setting_id, SettingValue::List(list), cx);
                    }),
                )
                .into_any_element()
        }
    }
}

fn make_id(prefix: &str, id: &str) -> SharedString {
    SharedString::from(format!("{prefix}:{id}"))
}

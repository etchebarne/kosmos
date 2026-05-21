use gpui::{
    Anchor, AnyElement, Context, Entity, IntoElement, SharedString, Subscription, Window, div,
    prelude::*, rems,
};
use gpui_component::{
    button::Button,
    input::{InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
};

use settings::{Setting, SettingControl, SettingValue, Settings};
use theme::ActiveTheme;

use crate::components::{DropdownOption, MultiSelect, left_aligned_button_label};
use crate::delegate::SettingsDelegate;
use crate::tabs::settings::state::ActiveSettingsInputs;

const SETTING_DROPDOWN_WIDTH_REM: f32 = 11.25;
const SETTING_NUMBER_WIDTH_REM: f32 = 7.5;

struct NumberSettingState {
    input: Entity<InputState>,
    committed_value: i64,
    _subscriptions: Vec<Subscription>,
}

pub fn render<T: SettingsDelegate>(
    setting: &'static Setting,
    value: &SettingValue,
    open_dropdown: Option<&'static str>,
    window: &mut Window,
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
            let input = number_setting_input(
                setting_id,
                value.as_int().unwrap_or(0),
                *min,
                *max,
                *step,
                window,
                cx,
            );
            NumberInput::new(&input)
                .w(rems(SETTING_NUMBER_WIDTH_REM))
                .into_any_element()
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

fn number_setting_input<T: SettingsDelegate>(
    setting_id: &'static str,
    value: i64,
    min: Option<i64>,
    max: Option<i64>,
    step: i64,
    window: &mut Window,
    cx: &mut Context<T>,
) -> Entity<InputState> {
    let value = clamp_number_value(value, min, max);
    let step = step.max(1);
    let state = window.use_keyed_state(
        make_id("setting-number-state", setting_id),
        cx,
        |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()));
            let _subscriptions = vec![
                cx.subscribe_in(
                    &input,
                    window,
                    move |state: &mut NumberSettingState,
                          input,
                          event: &NumberInputEvent,
                          window,
                          cx| {
                        match event {
                            NumberInputEvent::Step(action) => {
                                let current_value = input
                                    .read(cx)
                                    .value()
                                    .parse::<i64>()
                                    .unwrap_or(state.committed_value);
                                let next_value = match *action {
                                    StepAction::Decrement => current_value.saturating_sub(step),
                                    StepAction::Increment => current_value.saturating_add(step),
                                };
                                state.commit(
                                    input,
                                    setting_id,
                                    clamp_number_value(next_value, min, max),
                                    window,
                                    cx,
                                );
                            }
                        }
                    },
                ),
                cx.subscribe_in(
                    &input,
                    window,
                    move |state: &mut NumberSettingState, input, event: &InputEvent, window, cx| {
                        match event {
                            InputEvent::Change => {
                                let Some(value) = input.read(cx).value().parse::<i64>().ok() else {
                                    return;
                                };
                                if number_value_in_range(value, min, max) {
                                    state.set_committed_value(setting_id, value, cx);
                                }
                            }
                            InputEvent::Blur | InputEvent::PressEnter { .. } => {
                                let value = input
                                    .read(cx)
                                    .value()
                                    .parse::<i64>()
                                    .map(|value| clamp_number_value(value, min, max))
                                    .unwrap_or(state.committed_value);
                                state.commit(input, setting_id, value, window, cx);
                            }
                            InputEvent::Focus => {}
                        }
                    },
                ),
            ];

            NumberSettingState {
                input,
                committed_value: value,
                _subscriptions,
            }
        },
    );

    sync_number_setting_input(&state, value, window, cx);
    state.read(cx).input.clone()
}

impl NumberSettingState {
    fn commit(
        &mut self,
        input: &Entity<InputState>,
        setting_id: &'static str,
        value: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_committed_value(setting_id, value, cx);
        set_input_value_if_needed(input, value, window, cx);
    }

    fn set_committed_value(
        &mut self,
        setting_id: &'static str,
        value: i64,
        cx: &mut Context<Self>,
    ) {
        if self.committed_value == value {
            return;
        }
        self.committed_value = value;
        cx.update_global::<Settings, _>(|settings, _| {
            settings.set(setting_id, SettingValue::Int(value));
        });
        cx.refresh_windows();
    }
}

fn sync_number_setting_input<T>(
    state: &Entity<NumberSettingState>,
    value: i64,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    state.update(cx, |state, cx| {
        if state.committed_value == value {
            return;
        }
        state.committed_value = value;
        set_input_value_if_needed(&state.input, value, window, cx);
    });
}

fn set_input_value_if_needed<T>(
    input: &Entity<InputState>,
    value: i64,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    let value = value.to_string();
    input.update(cx, |input, cx| {
        if input.value().as_ref() != value {
            input.set_value(value, window, cx);
        }
    });
}

fn clamp_number_value(value: i64, min: Option<i64>, max: Option<i64>) -> i64 {
    value.clamp(min.unwrap_or(i64::MIN), max.unwrap_or(i64::MAX))
}

fn number_value_in_range(value: i64, min: Option<i64>, max: Option<i64>) -> bool {
    min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max)
}

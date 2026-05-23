use std::collections::HashSet;

use gpui::{
    Anchor, AnyElement, App, Context, ElementId, Entity, IntoElement, MouseButton, SharedString,
    Subscription, Window, div, prelude::*, rems,
};
use gpui_component::{
    button::Button,
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    menu::{DropdownMenu, PopupMenuItem},
    popover::Popover,
    switch::Switch,
};

use settings::{DropdownOption, Setting, SettingControl, SettingValue, Settings};
use theme::ActiveTheme;

use crate::delegate::SettingsDelegate;
use crate::tabs::settings::state::ActiveSettingsInputs;

const SETTING_DROPDOWN_WIDTH_REM: f32 = 11.25;
const SETTING_MULTI_SELECT_WIDTH_REM: f32 = 13.75;
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
                .label(label)
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
                Some(entity) => div()
                    .min_w(rems(13.75))
                    .child(Input::new(&entity))
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
            render_multi_select(
                setting_id,
                current,
                options(),
                *ordered,
                open_dropdown == Some(setting_id),
                cx,
            )
        }
    }
}

fn make_id(prefix: &str, id: &str) -> SharedString {
    SharedString::from(format!("{prefix}:{id}"))
}

fn render_multi_select<T: SettingsDelegate>(
    setting_id: &'static str,
    selected: Vec<SharedString>,
    options: &'static [DropdownOption],
    ordered: bool,
    is_open: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let summary = multi_select_summary(&selected, options);
    let on_open_change = cx.listener(move |this, _: &bool, _, cx| {
        this.toggle_settings_dropdown(setting_id, cx);
    });
    let trigger = Button::new(make_id("setting-multi-select", setting_id))
        .outline()
        .w(rems(SETTING_MULTI_SELECT_WIDTH_REM))
        .label(summary)
        .dropdown_caret(true);

    Popover::new(make_id("setting-multi-select-popover", setting_id))
        .anchor(Anchor::TopLeft)
        .appearance(false)
        .open(is_open)
        .trigger(trigger)
        .on_open_change(move |is_open, window, cx| on_open_change(is_open, window, cx))
        .content(move |_, _, cx| {
            render_multi_select_menu(setting_id, selected.clone(), options, ordered, cx)
        })
        .into_any_element()
}

fn multi_select_summary(selected: &[SharedString], options: &[DropdownOption]) -> SharedString {
    if selected.is_empty() {
        return "None".into();
    }

    selected
        .iter()
        .map(|id| {
            options
                .iter()
                .find(|option| option.id == id.as_ref())
                .map(|option| option.label)
                .unwrap_or(id.as_ref())
        })
        .collect::<Vec<_>>()
        .join(", ")
        .into()
}

struct MultiSelectRow {
    option_id: &'static str,
    option_label: &'static str,
    is_selected: bool,
    selected_index: Option<usize>,
}

fn render_multi_select_menu<T>(
    setting_id: &'static str,
    selected: Vec<SharedString>,
    options: &'static [DropdownOption],
    ordered: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let rows = multi_select_rows(&selected, options);
    let last_selected_idx = selected.len().saturating_sub(1);
    let mut items: Vec<AnyElement> = Vec::with_capacity(rows.len());

    for (row_idx, row_data) in rows.into_iter().enumerate() {
        let item_id =
            ElementId::Name(format!("setting-multi-select-{setting_id}-{row_idx}").into());
        let selected_for_row = selected.clone();
        let option_id = row_data.option_id;

        let mut row = div()
            .id(item_id)
            .h(rems(1.75))
            .min_w_full()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(rems(0.25))
            .text_sm()
            .text_color(if row_data.is_selected {
                theme.text_emphasis
            } else {
                theme.text
            })
            .hover(move |this| this.bg(theme.bg_selected))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let mut next = selected_for_row.clone();
                if let Some(pos) = next.iter().position(|id| id.as_ref() == option_id) {
                    next.remove(pos);
                } else {
                    next.push(SharedString::from(option_id));
                }
                set_multi_select_value(setting_id, next, cx);
            })
            .child(div().flex_1().child(row_data.option_label));

        if ordered && row_data.is_selected {
            if let Some(selected_index) = row_data.selected_index {
                let is_first = selected_index == 0;
                let is_last = selected_index == last_selected_idx;

                if !is_first {
                    let selected_up = selected.clone();
                    row = row.child(
                        div()
                            .id(ElementId::Name(
                                format!("setting-multi-select-{setting_id}-up-{row_idx}").into(),
                            ))
                            .px_1()
                            .text_color(theme.text_subtle)
                            .hover(move |this| this.text_color(theme.text_emphasis))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let mut next = selected_up.clone();
                                next.swap(selected_index, selected_index - 1);
                                set_multi_select_value(setting_id, next, cx);
                            })
                            .child("↑"),
                    );
                }

                if !is_last {
                    let selected_down = selected.clone();
                    row = row.child(
                        div()
                            .id(ElementId::Name(
                                format!("setting-multi-select-{setting_id}-down-{row_idx}").into(),
                            ))
                            .px_1()
                            .text_color(theme.text_subtle)
                            .hover(move |this| this.text_color(theme.text_emphasis))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let mut next = selected_down.clone();
                                next.swap(selected_index, selected_index + 1);
                                set_multi_select_value(setting_id, next, cx);
                            })
                            .child("↓"),
                    );
                }
            }
        }

        if row_data.is_selected {
            row = row.child(div().text_color(theme.accent).child("✓"));
        }

        items.push(row.into_any_element());
    }

    div()
        .id(make_id("setting-multi-select-menu", setting_id))
        .mt(rems(1.75))
        .min_w(rems(SETTING_MULTI_SELECT_WIDTH_REM))
        .p_1()
        .flex()
        .flex_col()
        .gap_0p5()
        .rounded(rems(0.375))
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.bg_elevated)
        .shadow_lg()
        .block_mouse_except_scroll()
        .children(items)
        .into_any_element()
}

fn multi_select_rows(
    selected: &[SharedString],
    options: &'static [DropdownOption],
) -> Vec<MultiSelectRow> {
    let selected_set: HashSet<&str> = selected.iter().map(|id| id.as_ref()).collect();
    let mut rows = Vec::with_capacity(options.len());

    for (selected_index, selected_id) in selected.iter().enumerate() {
        if let Some(option) = options
            .iter()
            .find(|option| option.id == selected_id.as_ref())
        {
            rows.push(MultiSelectRow {
                option_id: option.id,
                option_label: option.label,
                is_selected: true,
                selected_index: Some(selected_index),
            });
        }
    }

    for option in options {
        if !selected_set.contains(option.id) {
            rows.push(MultiSelectRow {
                option_id: option.id,
                option_label: option.label,
                is_selected: false,
                selected_index: None,
            });
        }
    }

    rows
}

fn set_multi_select_value(setting_id: &'static str, selected: Vec<SharedString>, cx: &mut App) {
    let list = selected.into_iter().map(SettingValue::String).collect();
    cx.update_global::<Settings, _>(|settings, _| {
        settings.set(setting_id, SettingValue::List(list));
    });
    cx.refresh_windows();
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

use std::collections::HashMap;

use gpui::{App, AppContext, BorrowAppContext, Entity, Global};

use settings::{SettingControl, SettingValue, Settings};

use crate::components::{TextInput, ValueChanged};

#[derive(Default)]
pub struct SettingsInputs {
    entities: HashMap<&'static str, Entity<TextInput>>,
}

impl SettingsInputs {
    pub fn install(cx: &mut App) {
        let mut inputs = Self::default();
        for category in Settings::categories() {
            for setting in category.settings {
                let SettingControl::Input { placeholder, .. } = &setting.control else {
                    continue;
                };
                let placeholder = placeholder.unwrap_or("");
                let initial = cx
                    .global::<Settings>()
                    .value(setting)
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let setting_id = setting.id;
                let entity = cx.new(|cx| TextInput::new(initial, placeholder, cx));
                cx.subscribe(&entity, move |_, event: &ValueChanged, cx| {
                    let value = event.value.clone();
                    cx.update_global::<Settings, _>(|settings, _| {
                        settings.set(setting_id, SettingValue::String(value));
                    });
                })
                .detach();
                inputs.entities.insert(setting.id, entity);
            }
        }
        cx.set_global(inputs);
    }

    pub fn get(&self, setting_id: &str) -> Option<Entity<TextInput>> {
        self.entities.get(setting_id).cloned()
    }
}

impl Global for SettingsInputs {}

pub trait ActiveSettingsInputs {
    fn settings_inputs(&self) -> &SettingsInputs;
}

impl ActiveSettingsInputs for App {
    fn settings_inputs(&self) -> &SettingsInputs {
        self.global::<SettingsInputs>()
    }
}

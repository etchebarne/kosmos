use std::collections::HashMap;

use gpui::{App, AppContext, BorrowAppContext, Context, Entity, Global, Window};
use gpui_component::input::{InputEvent, InputState};

use settings::{SettingControl, SettingValue, Settings};

#[derive(Default)]
pub struct SettingsInputs {
    search: Option<Entity<InputState>>,
    configs: HashMap<&'static str, (String, &'static str)>,
    entities: HashMap<&'static str, Entity<InputState>>,
}

impl SettingsInputs {
    pub fn install<T: 'static>(window: &mut Window, cx: &mut Context<T>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();

        let mut configs = HashMap::new();
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
                configs.insert(setting.id, (initial, placeholder));
            }
        }

        let mut inputs = Self {
            search: Some(search),
            configs,
            entities: HashMap::new(),
        };

        for setting_id in inputs.configs.keys().copied().collect::<Vec<_>>() {
            let (initial, placeholder) = inputs.configs.get(setting_id).unwrap();
            let entity = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(*placeholder)
                    .default_value(initial.clone())
            });
            let entity_for_sub = entity.clone();
            cx.subscribe(&entity, move |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let value = entity_for_sub.read(cx).value();
                    cx.update_global::<Settings, _>(|settings, _| {
                        settings.set(setting_id, SettingValue::String(value));
                    });
                }
            })
            .detach();
            inputs.entities.insert(setting_id, entity);
        }

        cx.set_global(inputs);
    }

    pub fn get(&self, setting_id: &str) -> Option<Entity<InputState>> {
        self.entities.get(setting_id).cloned()
    }

    pub fn search(&self) -> Option<Entity<InputState>> {
        self.search.clone()
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

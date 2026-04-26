pub mod registry;
mod store;
mod value;

use std::collections::HashMap;

use gpui::{App, Global, SharedString};

pub use registry::{ALL, Category, DropdownOption, Setting, SettingControl};
pub use value::SettingValue;

pub struct Settings {
    values: HashMap<SharedString, SettingValue>,
}

impl Settings {
    pub fn load() -> Self {
        Self {
            values: store::load_all(),
        }
    }

    pub fn categories() -> &'static [&'static Category] {
        registry::ALL
    }

    pub fn value(&self, setting: &Setting) -> SettingValue {
        self.values
            .get(setting.id)
            .cloned()
            .unwrap_or_else(|| setting.default_value())
    }

    pub fn get(&self, key: &str) -> Option<&SettingValue> {
        self.values.get(key)
    }

    pub fn set(&mut self, key: impl Into<SharedString>, value: SettingValue) {
        let key: SharedString = key.into();
        store::save(key.as_ref(), &value);
        self.values.insert(key, value);
    }
}

impl Global for Settings {}

pub trait ActiveSettings {
    fn settings(&self) -> &Settings;
}

impl ActiveSettings for App {
    fn settings(&self) -> &Settings {
        self.global::<Settings>()
    }
}

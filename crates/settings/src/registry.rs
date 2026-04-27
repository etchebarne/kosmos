use icons::IconName;
pub use theme::DropdownOption;

use crate::value::SettingValue;

pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: IconName,
    pub settings: &'static [Setting],
}

pub struct Setting {
    pub id: &'static str,
    pub name: &'static str,
    pub description: Option<&'static str>,
    pub control: SettingControl,
}

impl Setting {
    pub fn default_value(&self) -> SettingValue {
        match &self.control {
            SettingControl::Switch { default } => SettingValue::Bool(*default),
            SettingControl::Input { default, .. } => SettingValue::String((*default).into()),
            SettingControl::Number { default, .. } => SettingValue::Int(*default),
            SettingControl::Dropdown { default, .. } => SettingValue::String((*default).into()),
        }
    }
}

pub enum SettingControl {
    Switch {
        default: bool,
    },
    Input {
        default: &'static str,
        placeholder: Option<&'static str>,
    },
    Number {
        default: i64,
        min: Option<i64>,
        max: Option<i64>,
        step: i64,
    },
    Dropdown {
        default: &'static str,
        options: &'static [DropdownOption],
    },
}

pub const APPEARANCE: Category = Category {
    id: "appearance",
    name: "Appearance",
    icon: IconName::SettingsGear,
    settings: &[
        Setting {
            id: "appearance.theme",
            name: "Theme",
            description: Some("Color theme used across the interface."),
            control: SettingControl::Dropdown {
                default: "dark",
                options: theme::REGISTRY,
            },
        },
        Setting {
            id: "appearance.zoom",
            name: "Zoom",
            description: Some("Interface zoom level, in percent."),
            control: SettingControl::Number {
                default: 100,
                min: Some(75),
                max: Some(125),
                step: 5,
            },
        },
    ],
};

pub const ALL: &[&Category] = &[&APPEARANCE];

pub fn category(id: &str) -> Option<&'static Category> {
    ALL.iter().copied().find(|c| c.id == id)
}

pub fn setting(id: &str) -> Option<&'static Setting> {
    ALL.iter()
        .flat_map(|c| c.settings.iter())
        .find(|s| s.id == id)
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateSettingParams {
    pub(crate) id: String,
    pub(crate) value: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsSnapshot {
    categories: Vec<SettingCategoryPayload>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingCategoryPayload {
    id: String,
    label: String,
    description: Option<String>,
    items: Vec<SettingItemPayload>,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum SettingItemPayload {
    Group {
        id: String,
        label: String,
        description: Option<String>,
        items: Vec<SettingItemPayload>,
    },
    Setting {
        id: String,
        label: String,
        description: Option<String>,
        control: SettingControlPayload,
        value: SettingValuePayload,
        default_value: SettingValuePayload,
    },
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum SettingControlPayload {
    Switch,
    Select {
        options: Vec<SettingOptionPayload>,
    },
    Input {
        input_type: SettingInputKindPayload,
        placeholder: Option<String>,
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingOptionPayload {
    value: String,
    label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum SettingInputKindPayload {
    Text,
    Number,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum SettingValuePayload {
    Boolean(bool),
    String(String),
    Number(f64),
}

impl SettingsSnapshot {
    pub(crate) fn from_settings(settings: &core::settings::Settings) -> Self {
        Self {
            categories: settings
                .categories()
                .iter()
                .map(SettingCategoryPayload::from_category)
                .collect(),
        }
    }
}

impl SettingCategoryPayload {
    fn from_category(category: &core::settings::SettingCategory) -> Self {
        Self {
            id: category.id().to_owned(),
            label: category.label().to_owned(),
            description: category.description().map(str::to_owned),
            items: category
                .items()
                .iter()
                .map(SettingItemPayload::from_item)
                .collect(),
        }
    }
}

impl SettingItemPayload {
    fn from_item(item: &core::settings::SettingItem) -> Self {
        match item {
            core::settings::SettingItem::Group(group) => Self::Group {
                id: group.id().to_owned(),
                label: group.label().to_owned(),
                description: group.description().map(str::to_owned),
                items: group.items().iter().map(Self::from_item).collect(),
            },
            core::settings::SettingItem::Setting(setting) => Self::Setting {
                id: setting.id().to_owned(),
                label: setting.label().to_owned(),
                description: setting.description().map(str::to_owned),
                control: SettingControlPayload::from_control(setting.control()),
                value: SettingValuePayload::from_value(setting.value()),
                default_value: SettingValuePayload::from_value(setting.default_value()),
            },
        }
    }
}

impl SettingControlPayload {
    fn from_control(control: &core::settings::SettingControl) -> Self {
        match control {
            core::settings::SettingControl::Switch => Self::Switch,
            core::settings::SettingControl::Select { options } => Self::Select {
                options: options
                    .iter()
                    .map(|option| SettingOptionPayload {
                        value: option.value().to_owned(),
                        label: option.label().to_owned(),
                    })
                    .collect(),
            },
            core::settings::SettingControl::Input(input) => Self::Input {
                input_type: match input.kind() {
                    core::settings::SettingInputKind::Text => SettingInputKindPayload::Text,
                    core::settings::SettingInputKind::Number => SettingInputKindPayload::Number,
                },
                placeholder: input.placeholder().map(str::to_owned),
                min: input.min(),
                max: input.max(),
                step: input.step(),
            },
        }
    }
}

impl SettingValuePayload {
    fn from_value(value: &core::settings::SettingValue) -> Self {
        match value {
            core::settings::SettingValue::Boolean(value) => Self::Boolean(*value),
            core::settings::SettingValue::String(value) => Self::String(value.clone()),
            core::settings::SettingValue::Number(value) => Self::Number(*value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_snapshot_uses_the_desktop_wire_shape() {
        let snapshot = serde_json::to_value(SettingsSnapshot::from_settings(
            &core::settings::Settings::default(),
        ))
        .expect("settings should serialize");

        assert_eq!(snapshot["categories"][0]["id"], "appearance");
        assert_eq!(snapshot["categories"][0]["items"][0]["type"], "setting");
        assert_eq!(
            snapshot["categories"][0]["items"][0]["control"]["type"],
            "input"
        );
        assert_eq!(snapshot["categories"][0]["items"][0]["value"], 100.0);
        assert_eq!(snapshot["categories"][0]["items"][0]["defaultValue"], 100.0);
        assert!(snapshot["categories"][0]["items"][0]["default_value"].is_null());
    }

    #[test]
    fn input_controls_serialize_camel_case_fields() {
        let control = serde_json::to_value(SettingControlPayload::Input {
            input_type: SettingInputKindPayload::Number,
            placeholder: None,
            min: Some(1.0),
            max: Some(10.0),
            step: Some(1.0),
        })
        .expect("control should serialize");

        assert_eq!(control["type"], "input");
        assert_eq!(control["inputType"], "number");
        assert!(control["input_type"].is_null());
    }
}

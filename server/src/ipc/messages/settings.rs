use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateSettingParams {
    pub(crate) id: String,
    pub(crate) value: SettingValueParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum SettingValueParam {
    Boolean(bool),
    String(String),
    Number(f64),
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsSnapshot {
    revision: u64,
    editor: ResolvedEditorSettingsPayload,
    appearance: ResolvedAppearanceSettingsPayload,
    categories: Vec<SettingCategoryPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedEditorSettingsPayload {
    soft_wrap: bool,
    minimap: bool,
    format_on_save: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedAppearanceSettingsPayload {
    zoom_setting_id: String,
    zoom_level: f64,
    default_zoom_level: f64,
    min_zoom_level: f64,
    max_zoom_level: f64,
    zoom_step: f64,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingCategoryPayload {
    id: String,
    label: String,
    description: Option<String>,
    items: Vec<SettingItemPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SettingItemPayload {
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
        #[schemars(rename = "defaultValue")]
        default_value: SettingValuePayload,
    },
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SettingControlPayload {
    Switch,
    Select {
        options: Vec<SettingOptionPayload>,
    },
    Input {
        #[schemars(rename = "inputType")]
        input_type: SettingInputKindPayload,
        placeholder: Option<String>,
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingOptionPayload {
    value: String,
    label: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SettingInputKindPayload {
    Text,
    Number,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(untagged)]
pub(crate) enum SettingValuePayload {
    Boolean(bool),
    String(String),
    Number(f64),
}

impl SettingsSnapshot {
    #[cfg(test)]
    pub(crate) fn from_settings(settings: &core::settings::Settings) -> Self {
        Self::from_resolved(core::settings::ResolvedSettings::new(0, settings), settings)
    }

    pub(crate) fn from_state(state: &core::State) -> Self {
        Self::from_resolved(state.resolved_settings(), state.settings())
    }

    pub(crate) fn from_state_with_revision(state: &core::State, revision: u64) -> Self {
        Self::from_resolved(
            core::settings::ResolvedSettings::new(revision, state.settings()),
            state.settings(),
        )
    }

    fn from_resolved(
        resolved: core::settings::ResolvedSettings,
        settings: &core::settings::Settings,
    ) -> Self {
        Self {
            revision: resolved.revision(),
            editor: ResolvedEditorSettingsPayload::from_core(resolved.editor()),
            appearance: ResolvedAppearanceSettingsPayload::from_core(resolved.appearance()),
            categories: settings
                .categories()
                .iter()
                .map(SettingCategoryPayload::from_category)
                .collect(),
        }
    }
}

impl ResolvedEditorSettingsPayload {
    fn from_core(settings: &core::settings::ResolvedEditorSettings) -> Self {
        Self {
            soft_wrap: settings.soft_wrap(),
            minimap: settings.minimap(),
            format_on_save: settings.format_on_save(),
        }
    }
}

impl ResolvedAppearanceSettingsPayload {
    fn from_core(settings: &core::settings::ResolvedAppearanceSettings) -> Self {
        Self {
            zoom_setting_id: settings.zoom_setting_id().to_owned(),
            zoom_level: settings.zoom_level(),
            default_zoom_level: settings.default_zoom_level(),
            min_zoom_level: settings.min_zoom_level(),
            max_zoom_level: settings.max_zoom_level(),
            zoom_step: settings.zoom_step(),
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
        assert_eq!(snapshot["revision"], 0);
        assert_eq!(snapshot["editor"]["softWrap"], false);
        assert_eq!(snapshot["appearance"]["zoomLevel"], 100.0);
        assert_eq!(snapshot["appearance"]["defaultZoomLevel"], 100.0);
        assert_eq!(snapshot["appearance"]["minZoomLevel"], 80.0);
        assert_eq!(snapshot["appearance"]["maxZoomLevel"], 140.0);
        assert_eq!(snapshot["appearance"]["zoomStep"], 10.0);
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

    #[test]
    fn settings_snapshot_maps_resolved_overrides_from_core() {
        let mut state = core::State::new();
        state
            .update_setting(
                core::settings::EDITOR_FORMAT_ON_SAVE,
                core::settings::SettingValue::Boolean(true),
            )
            .unwrap();
        state
            .update_setting(
                core::settings::APPEARANCE_ZOOM_LEVEL,
                core::settings::SettingValue::Number(120.0),
            )
            .unwrap();

        let snapshot = serde_json::to_value(SettingsSnapshot::from_state_with_revision(&state, 4))
            .expect("settings should serialize");
        assert_eq!(snapshot["revision"], 4);
        assert_eq!(snapshot["editor"]["formatOnSave"], true);
        assert_eq!(snapshot["appearance"]["zoomLevel"], 120.0);
        assert_eq!(snapshot["appearance"]["minZoomLevel"], 80.0);
        assert_eq!(snapshot["appearance"]["maxZoomLevel"], 140.0);
        assert_eq!(snapshot["appearance"]["zoomStep"], 10.0);
    }
}

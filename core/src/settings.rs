use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;

pub const APPEARANCE_ZOOM_LEVEL: &str = "appearance.zoomLevel";
pub const EDITOR_SOFT_WRAP: &str = "editor.softWrap";
pub const EDITOR_MINIMAP: &str = "editor.minimap";
pub const EDITOR_FORMAT_ON_SAVE: &str = "editor.formatOnSave";
pub const RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT: &str =
    "resources.developmentMemoryLimitPercent";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    overrides: BTreeMap<String, SettingValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSettings {
    revision: u64,
    editor: ResolvedEditorSettings,
    appearance: ResolvedAppearanceSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedEditorSettings {
    soft_wrap: bool,
    minimap: bool,
    format_on_save: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedAppearanceSettings {
    zoom_setting_id: &'static str,
    zoom_level: f64,
    default_zoom_level: f64,
    min_zoom_level: f64,
    max_zoom_level: f64,
    zoom_step: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingCategory {
    id: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    items: Vec<SettingItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingItem {
    Group(SettingGroup),
    Setting(SettingDefinition),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingGroup {
    id: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    items: Vec<SettingItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingDefinition {
    id: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    control: SettingControl,
    value: SettingValue,
    default_value: SettingValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingControl {
    Switch,
    Select { options: Vec<SettingOption> },
    Input(SettingInput),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingOption {
    value: &'static str,
    label: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingInput {
    kind: SettingInputKind,
    placeholder: Option<&'static str>,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingInputKind {
    Text,
    Number,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Boolean(bool),
    String(String),
    Number(f64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsError {
    UnknownSetting(String),
    InvalidValue { id: String, message: String },
}

impl Settings {
    pub fn categories(&self) -> Vec<SettingCategory> {
        vec![
            SettingCategory {
                id: "appearance",
                label: "Appearance",
                description: Some("Control how Kosmos looks on your screen."),
                items: vec![SettingItem::Setting(self.definition(APPEARANCE_ZOOM_LEVEL))],
            },
            SettingCategory {
                id: "editor",
                label: "Editor",
                description: Some("Control how files are displayed in the editor."),
                items: vec![
                    SettingItem::Setting(self.definition(EDITOR_SOFT_WRAP)),
                    SettingItem::Setting(self.definition(EDITOR_MINIMAP)),
                    SettingItem::Setting(self.definition(EDITOR_FORMAT_ON_SAVE)),
                ],
            },
            SettingCategory {
                id: "resources",
                label: "Resources",
                description: Some("Control resources used by development workloads."),
                items: vec![SettingItem::Setting(
                    self.definition(RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT),
                )],
            },
        ]
    }

    pub fn value(&self, id: &str) -> Option<SettingValue> {
        setting_definition(id).map(|definition| {
            self.overrides
                .get(id)
                .cloned()
                .unwrap_or(definition.default_value)
        })
    }

    pub fn boolean(&self, id: &str) -> Option<bool> {
        match self.value(id) {
            Some(SettingValue::Boolean(value)) => Some(value),
            _ => None,
        }
    }

    pub fn number(&self, id: &str) -> Option<f64> {
        match self.value(id) {
            Some(SettingValue::Number(value)) => Some(value),
            _ => None,
        }
    }

    pub fn resolved_editor_settings(&self) -> ResolvedEditorSettings {
        ResolvedEditorSettings {
            soft_wrap: self
                .boolean(EDITOR_SOFT_WRAP)
                .expect("editor soft wrap is boolean"),
            minimap: self
                .boolean(EDITOR_MINIMAP)
                .expect("editor minimap is boolean"),
            format_on_save: self
                .boolean(EDITOR_FORMAT_ON_SAVE)
                .expect("editor format on save is boolean"),
        }
    }

    pub fn resolved_appearance_settings(&self) -> ResolvedAppearanceSettings {
        let definition = self.definition(APPEARANCE_ZOOM_LEVEL);
        let SettingControl::Input(input) = definition.control else {
            unreachable!("appearance zoom is a numeric input")
        };
        let SettingValue::Number(zoom_level) = definition.value else {
            unreachable!("appearance zoom has a numeric value")
        };
        let SettingValue::Number(default_zoom_level) = definition.default_value else {
            unreachable!("appearance zoom has a numeric default")
        };

        ResolvedAppearanceSettings {
            zoom_setting_id: definition.id,
            zoom_level,
            default_zoom_level,
            min_zoom_level: input.min.expect("appearance zoom has a minimum"),
            max_zoom_level: input.max.expect("appearance zoom has a maximum"),
            zoom_step: input.step.expect("appearance zoom has a step"),
        }
    }

    pub fn update(&mut self, id: &str, value: SettingValue) -> Result<bool, SettingsError> {
        let definition =
            setting_definition(id).ok_or_else(|| SettingsError::UnknownSetting(id.to_owned()))?;
        validate_value(&definition, &value)?;

        let previous = self.value(id).expect("known settings always have a value");
        if value == definition.default_value {
            self.overrides.remove(id);
        } else {
            self.overrides.insert(id.to_owned(), value.clone());
        }

        Ok(previous != value)
    }

    pub fn overrides(&self) -> impl Iterator<Item = (&str, &SettingValue)> {
        self.overrides
            .iter()
            .map(|(id, value)| (id.as_str(), value))
    }

    fn definition(&self, id: &str) -> SettingDefinition {
        let mut definition = setting_definition(id).expect("catalog contains known settings");
        definition.value = self
            .overrides
            .get(id)
            .cloned()
            .unwrap_or_else(|| definition.default_value.clone());
        definition
    }
}

impl ResolvedSettings {
    pub fn new(revision: u64, settings: &Settings) -> Self {
        Self {
            revision,
            editor: settings.resolved_editor_settings(),
            appearance: settings.resolved_appearance_settings(),
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn editor(&self) -> &ResolvedEditorSettings {
        &self.editor
    }

    pub fn appearance(&self) -> &ResolvedAppearanceSettings {
        &self.appearance
    }
}

impl ResolvedEditorSettings {
    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    pub fn minimap(&self) -> bool {
        self.minimap
    }

    pub fn format_on_save(&self) -> bool {
        self.format_on_save
    }
}

impl ResolvedAppearanceSettings {
    pub fn zoom_setting_id(&self) -> &str {
        self.zoom_setting_id
    }

    pub fn zoom_level(&self) -> f64 {
        self.zoom_level
    }

    pub fn default_zoom_level(&self) -> f64 {
        self.default_zoom_level
    }

    pub fn min_zoom_level(&self) -> f64 {
        self.min_zoom_level
    }

    pub fn max_zoom_level(&self) -> f64 {
        self.max_zoom_level
    }

    pub fn zoom_step(&self) -> f64 {
        self.zoom_step
    }
}

impl SettingCategory {
    pub fn id(&self) -> &str {
        self.id
    }

    pub fn label(&self) -> &str {
        self.label
    }

    pub fn description(&self) -> Option<&str> {
        self.description
    }

    pub fn items(&self) -> &[SettingItem] {
        &self.items
    }
}

impl SettingGroup {
    pub fn id(&self) -> &str {
        self.id
    }

    pub fn label(&self) -> &str {
        self.label
    }

    pub fn description(&self) -> Option<&str> {
        self.description
    }

    pub fn items(&self) -> &[SettingItem] {
        &self.items
    }
}

impl SettingDefinition {
    pub fn id(&self) -> &str {
        self.id
    }

    pub fn label(&self) -> &str {
        self.label
    }

    pub fn description(&self) -> Option<&str> {
        self.description
    }

    pub fn control(&self) -> &SettingControl {
        &self.control
    }

    pub fn value(&self) -> &SettingValue {
        &self.value
    }

    pub fn default_value(&self) -> &SettingValue {
        &self.default_value
    }
}

impl SettingOption {
    pub fn value(&self) -> &str {
        self.value
    }

    pub fn label(&self) -> &str {
        self.label
    }
}

impl SettingInput {
    pub fn kind(&self) -> SettingInputKind {
        self.kind
    }

    pub fn placeholder(&self) -> Option<&str> {
        self.placeholder
    }

    pub fn min(&self) -> Option<f64> {
        self.min
    }

    pub fn max(&self) -> Option<f64> {
        self.max
    }

    pub fn step(&self) -> Option<f64> {
        self.step
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSetting(id) => write!(formatter, "unknown setting `{id}`"),
            Self::InvalidValue { id, message } => {
                write!(formatter, "invalid value for setting `{id}`: {message}")
            }
        }
    }
}

impl StdError for SettingsError {}

fn setting_definition(id: &str) -> Option<SettingDefinition> {
    if id == APPEARANCE_ZOOM_LEVEL {
        return Some(SettingDefinition {
            id: APPEARANCE_ZOOM_LEVEL,
            label: "Zoom level (%)",
            description: Some("Scale the entire interface. You can also use Ctrl/Cmd + and -."),
            control: SettingControl::Input(SettingInput {
                kind: SettingInputKind::Number,
                placeholder: None,
                min: Some(80.0),
                max: Some(140.0),
                step: Some(10.0),
            }),
            value: SettingValue::Number(100.0),
            default_value: SettingValue::Number(100.0),
        });
    }

    if id == RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT {
        return Some(SettingDefinition {
            id: RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT,
            label: "Development memory limit (%)",
            description: Some(
                "Limit memory across all processes launched by Kosmos terminals. Lower limits can stop running commands, while protecting the editor from system-wide out-of-memory failures. Changes apply when a terminal is next opened or restarted.",
            ),
            control: SettingControl::Input(SettingInput {
                kind: SettingInputKind::Number,
                placeholder: None,
                min: Some(10.0),
                max: Some(75.0),
                step: Some(5.0),
            }),
            value: SettingValue::Number(25.0),
            default_value: SettingValue::Number(25.0),
        });
    }

    let (label, description) = match id {
        EDITOR_SOFT_WRAP => (
            "Soft wrap",
            "Wrap long lines to the editor width instead of scrolling horizontally.",
        ),
        EDITOR_MINIMAP => (
            "Minimap",
            "Show a compact overview of the file along the right edge of the editor.",
        ),
        EDITOR_FORMAT_ON_SAVE => (
            "Format on save",
            "Format the active document before saving when a formatter is available.",
        ),
        _ => return None,
    };

    Some(SettingDefinition {
        id: match id {
            EDITOR_SOFT_WRAP => EDITOR_SOFT_WRAP,
            EDITOR_MINIMAP => EDITOR_MINIMAP,
            EDITOR_FORMAT_ON_SAVE => EDITOR_FORMAT_ON_SAVE,
            _ => unreachable!(),
        },
        label,
        description: Some(description),
        control: SettingControl::Switch,
        value: SettingValue::Boolean(false),
        default_value: SettingValue::Boolean(false),
    })
}

fn validate_value(
    definition: &SettingDefinition,
    value: &SettingValue,
) -> Result<(), SettingsError> {
    let is_valid = match (&definition.control, value) {
        (SettingControl::Switch, SettingValue::Boolean(_)) => true,
        (SettingControl::Select { options }, SettingValue::String(value)) => {
            options.iter().any(|option| option.value == value)
        }
        (SettingControl::Input(input), SettingValue::String(_)) => {
            input.kind == SettingInputKind::Text
        }
        (SettingControl::Input(input), SettingValue::Number(value)) => {
            input.kind == SettingInputKind::Number
                && value.is_finite()
                && input.min.is_none_or(|min| *value >= min)
                && input.max.is_none_or(|max| *value <= max)
        }
        _ => false,
    };

    if is_valid {
        Ok(())
    } else {
        Err(SettingsError::InvalidValue {
            id: definition.id.to_owned(),
            message: "the value does not match the setting control".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_defaults_in_stable_order() {
        let settings = Settings::default();
        let categories = settings.categories();

        assert_eq!(categories.len(), 3);
        assert_eq!(categories[0].id(), "appearance");
        assert_eq!(categories[0].items().len(), 1);
        assert_eq!(categories[1].id(), "editor");
        assert_eq!(categories[1].items().len(), 3);
        assert_eq!(categories[2].id(), "resources");
        assert_eq!(categories[2].items().len(), 1);
        assert_eq!(
            settings.value(APPEARANCE_ZOOM_LEVEL),
            Some(SettingValue::Number(100.0))
        );
        assert_eq!(settings.boolean(EDITOR_SOFT_WRAP), Some(false));
        assert_eq!(settings.boolean(EDITOR_MINIMAP), Some(false));
        assert_eq!(settings.boolean(EDITOR_FORMAT_ON_SAVE), Some(false));
        assert_eq!(
            settings.number(RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT),
            Some(25.0)
        );
    }

    #[test]
    fn updates_known_settings_and_removes_default_overrides() {
        let mut settings = Settings::default();

        assert!(
            settings
                .update(EDITOR_SOFT_WRAP, SettingValue::Boolean(true))
                .expect("setting should update")
        );
        assert_eq!(settings.boolean(EDITOR_SOFT_WRAP), Some(true));
        assert_eq!(settings.overrides().count(), 1);

        assert!(
            settings
                .update(EDITOR_SOFT_WRAP, SettingValue::Boolean(false))
                .expect("setting should reset")
        );
        assert_eq!(settings.overrides().count(), 0);
    }

    #[test]
    fn invalid_updates_do_not_mutate_settings() {
        let mut settings = Settings::default();

        assert!(matches!(
            settings.update(EDITOR_SOFT_WRAP, SettingValue::String("true".to_owned())),
            Err(SettingsError::InvalidValue { .. })
        ));
        assert!(matches!(
            settings.update("missing", SettingValue::Boolean(true)),
            Err(SettingsError::UnknownSetting(_))
        ));
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn development_memory_limit_updates_and_resets() {
        let mut settings = Settings::default();

        assert!(
            settings
                .update(
                    RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT,
                    SettingValue::Number(50.0),
                )
                .unwrap()
        );
        assert_eq!(
            settings.number(RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT),
            Some(50.0)
        );
        assert!(
            settings
                .update(
                    RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT,
                    SettingValue::Number(25.0),
                )
                .unwrap()
        );
        assert_eq!(settings.overrides().count(), 0);
    }

    #[test]
    fn development_memory_limit_rejects_out_of_range_values() {
        let mut settings = Settings::default();

        for value in [9.0, 76.0, f64::NAN] {
            assert!(matches!(
                settings.update(
                    RESOURCES_DEVELOPMENT_MEMORY_LIMIT_PERCENT,
                    SettingValue::Number(value),
                ),
                Err(SettingsError::InvalidValue { .. })
            ));
        }
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn resolved_defaults_match_the_catalog() {
        let settings = Settings::default();
        let editor = settings.resolved_editor_settings();
        let appearance = settings.resolved_appearance_settings();

        assert!(!editor.soft_wrap());
        assert!(!editor.minimap());
        assert!(!editor.format_on_save());
        assert_eq!(appearance.zoom_level(), 100.0);
        assert_eq!(appearance.default_zoom_level(), 100.0);
        assert_eq!(appearance.min_zoom_level(), 80.0);
        assert_eq!(appearance.max_zoom_level(), 140.0);
        assert_eq!(appearance.zoom_step(), 10.0);
    }

    #[test]
    fn resolved_overrides_match_the_catalog() {
        let mut settings = Settings::default();
        settings
            .update(EDITOR_SOFT_WRAP, SettingValue::Boolean(true))
            .unwrap();
        settings
            .update(EDITOR_MINIMAP, SettingValue::Boolean(true))
            .unwrap();
        settings
            .update(EDITOR_FORMAT_ON_SAVE, SettingValue::Boolean(true))
            .unwrap();
        settings
            .update(APPEARANCE_ZOOM_LEVEL, SettingValue::Number(120.0))
            .unwrap();

        let editor = settings.resolved_editor_settings();
        let appearance = settings.resolved_appearance_settings();
        assert!(editor.soft_wrap());
        assert!(editor.minimap());
        assert!(editor.format_on_save());
        assert_eq!(appearance.zoom_setting_id(), APPEARANCE_ZOOM_LEVEL);
        assert_eq!(appearance.zoom_level(), 120.0);
        assert_eq!(appearance.min_zoom_level(), 80.0);
        assert_eq!(appearance.max_zoom_level(), 140.0);
        assert_eq!(appearance.zoom_step(), 10.0);
    }
}

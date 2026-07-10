use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;

pub const EDITOR_SOFT_WRAP: &str = "editor.softWrap";
pub const EDITOR_MINIMAP: &str = "editor.minimap";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    overrides: BTreeMap<String, SettingValue>,
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
        vec![SettingCategory {
            id: "editor",
            label: "Editor",
            description: Some("Control how files are displayed in the editor."),
            items: vec![
                SettingItem::Setting(self.definition(EDITOR_SOFT_WRAP)),
                SettingItem::Setting(self.definition(EDITOR_MINIMAP)),
            ],
        }]
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
    let (label, description) = match id {
        EDITOR_SOFT_WRAP => (
            "Soft wrap",
            "Wrap long lines to the editor width instead of scrolling horizontally.",
        ),
        EDITOR_MINIMAP => (
            "Minimap",
            "Show a compact overview of the file along the right edge of the editor.",
        ),
        _ => return None,
    };

    Some(SettingDefinition {
        id: match id {
            EDITOR_SOFT_WRAP => EDITOR_SOFT_WRAP,
            EDITOR_MINIMAP => EDITOR_MINIMAP,
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
    fn catalog_contains_editor_defaults_in_stable_order() {
        let settings = Settings::default();
        let categories = settings.categories();

        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].id(), "editor");
        assert_eq!(categories[0].items().len(), 2);
        assert_eq!(settings.boolean(EDITOR_SOFT_WRAP), Some(false));
        assert_eq!(settings.boolean(EDITOR_MINIMAP), Some(false));
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
}

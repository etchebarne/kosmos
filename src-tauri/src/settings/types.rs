use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSchema {
    pub sections: Vec<SettingsSection>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSection {
    pub id: String,
    pub label: String,
    pub groups: Vec<SettingsGroup>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsGroup {
    pub title: String,
    pub settings: Vec<SettingEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingEntry {
    pub key: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub control: SettingControl,
    pub default_value: serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub show_when: Vec<ShowWhen>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowWhen {
    pub key: String,
    pub equals: serde_json::Value,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SettingControl {
    #[serde(rename_all = "camelCase")]
    Dropdown { options: Vec<DropdownOption> },
    Switch,
    #[serde(rename_all = "camelCase")]
    Number { min: f64, max: f64, step: f64 },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropdownOption {
    pub value: String,
    pub label: String,
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

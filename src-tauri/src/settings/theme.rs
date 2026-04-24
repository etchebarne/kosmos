use super::types::*;

pub fn section() -> SettingsSection {
    SettingsSection {
        id: "theme".into(),
        label: "Theme".into(),
        groups: vec![SettingsGroup {
            title: "Color Theme".into(),
            settings: vec![
                SettingEntry {
                    key: "theme.colorTheme".into(),
                    label: "Color theme".into(),
                    description: Some("Specifies the color theme".into()),
                    control: SettingControl::Dropdown {
                        options: vec![
                            DropdownOption {
                                value: "kosmos-dark".into(),
                                label: "Kosmos Dark".into(),
                                disabled: false,
                            },
                            DropdownOption {
                                value: "kosmos-light".into(),
                                label: "Kosmos Light".into(),
                                disabled: false,
                            },
                            DropdownOption {
                                value: "kosmos-ember".into(),
                                label: "Kosmos Ember".into(),
                                disabled: false,
                            },
                        ],
                    },
                    default_value: serde_json::json!("kosmos-dark"),
                    show_when: vec![],
                },
                SettingEntry {
                    key: "theme.solidMode".into(),
                    label: "Solid mode".into(),
                    description: Some(
                        "Flatten depth gradients on pill surfaces".into(),
                    ),
                    control: SettingControl::Switch,
                    default_value: serde_json::json!(false),
                    show_when: vec![],
                },
            ],
        }],
    }
}

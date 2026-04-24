use super::types::*;

pub fn section() -> SettingsSection {
    SettingsSection {
        id: "theme".into(),
        label: "Theme".into(),
        groups: vec![
            SettingsGroup {
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
            },
            SettingsGroup {
                title: "Display".into(),
                settings: vec![SettingEntry {
                    key: "theme.uiZoom".into(),
                    label: "UI zoom".into(),
                    description: Some(
                        "Scale the entire UI, as a percentage (80–125).".into(),
                    ),
                    control: SettingControl::Number {
                        min: 80.0,
                        max: 125.0,
                        step: 5.0,
                    },
                    default_value: serde_json::json!(100),
                    show_when: vec![],
                }],
            },
        ],
    }
}

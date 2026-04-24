use super::types::*;

pub fn section() -> SettingsSection {
    SettingsSection {
        id: "ai".into(),
        label: "AI".into(),
        groups: vec![SettingsGroup {
            title: "Code Generation".into(),
            settings: vec![
                SettingEntry {
                    key: "ai.enableCompletion".into(),
                    label: "Function completion".into(),
                    description: Some(
                        "Show a button on function definition lines to generate the function body using AI, based on its name, parameters, return type, comments, and surrounding context."
                            .into(),
                    ),
                    control: SettingControl::Switch,
                    default_value: serde_json::json!(false),
                    show_when: None,
                },
                SettingEntry {
                    key: "ai.agent".into(),
                    label: "Agent".into(),
                    description: Some(
                        "The AI agent used to generate function bodies.".into(),
                    ),
                    control: SettingControl::Dropdown {
                        options: vec![
                            DropdownOption {
                                value: "claude-code".into(),
                                label: "Claude Code".into(),
                            },
                            DropdownOption {
                                value: "codex".into(),
                                label: "Codex".into(),
                            },
                        ],
                    },
                    default_value: serde_json::json!("claude-code"),
                    show_when: Some(ShowWhen {
                        key: "ai.enableCompletion".into(),
                        equals: serde_json::json!(true),
                    }),
                },
            ],
        }],
    }
}

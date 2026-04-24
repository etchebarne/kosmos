use super::types::*;
use crate::ai;

pub fn section() -> SettingsSection {
    let claude_installed = ai::is_agent_installed("claude-code");
    let codex_installed = ai::is_agent_installed("codex");

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
                    show_when: vec![],
                },
                SettingEntry {
                    key: "ai.agent".into(),
                    label: "Agent".into(),
                    description: Some(
                        "The AI agent used to generate function bodies.".into(),
                    ),
                    control: SettingControl::Dropdown {
                        options: vec![
                            agent_option("claude-code", "Claude Code", claude_installed),
                            agent_option("codex", "Codex", codex_installed),
                        ],
                    },
                    default_value: serde_json::json!("claude-code"),
                    show_when: vec![ShowWhen {
                        key: "ai.enableCompletion".into(),
                        equals: serde_json::json!(true),
                    }],
                },
                SettingEntry {
                    key: "ai.claudeCode.model".into(),
                    label: "Model".into(),
                    description: Some(
                        "The Claude model used to generate function bodies.".into(),
                    ),
                    control: SettingControl::Dropdown {
                        options: vec![
                            DropdownOption {
                                value: "haiku".into(),
                                label: "Haiku".into(),
                                disabled: false,
                            },
                            DropdownOption {
                                value: "sonnet".into(),
                                label: "Sonnet".into(),
                                disabled: false,
                            },
                            DropdownOption {
                                value: "opus".into(),
                                label: "Opus".into(),
                                disabled: false,
                            },
                        ],
                    },
                    default_value: serde_json::json!("sonnet"),
                    show_when: vec![
                        ShowWhen {
                            key: "ai.enableCompletion".into(),
                            equals: serde_json::json!(true),
                        },
                        ShowWhen {
                            key: "ai.agent".into(),
                            equals: serde_json::json!("claude-code"),
                        },
                    ],
                },
            ],
        }],
    }
}

fn agent_option(value: &str, label: &str, installed: bool) -> DropdownOption {
    DropdownOption {
        value: value.into(),
        label: label.into(),
        disabled: !installed,
    }
}


use serde::{Deserialize, Serialize};

use super::ids::{TabIdParam, WorkspaceIdParam};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerParams {
    pub(crate) server_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenLanguageServerDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) tab_id: TabIdParam,
    pub(crate) language_id: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeLanguageServerDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) changes: Vec<LanguageServerChangePayload>,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseLanguageServerDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveLanguageServerDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerHoverParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) position: LanguageServerPositionPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerDiagnosticsParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerCompletionParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) position: LanguageServerPositionPayload,
    pub(crate) trigger_kind: u32,
    pub(crate) trigger_character: Option<String>,
    pub(crate) filter: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveLanguageServerCompletionParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) server_id: String,
    pub(crate) raw: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerColorPresentationParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) server_id: String,
    pub(crate) range: LanguageServerRangePayload,
    pub(crate) color: LanguageServerColorPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerFormattingParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) language_id: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) text: String,
    pub(crate) tab_size: u32,
    pub(crate) insert_spaces: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerColorPayload {
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrustLanguageServerWorkspaceParams {
    pub(crate) workspace_id: WorkspaceIdParam,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerChangePayload {
    pub(crate) range: LanguageServerRangePayload,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerPositionPayload {
    pub(crate) line: u32,
    pub(crate) character: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerRangePayload {
    pub(crate) start: LanguageServerPositionPayload,
    pub(crate) end: LanguageServerPositionPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerListSnapshot {
    servers: Vec<LanguageServerSnapshot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerSnapshot {
    id: String,
    name: String,
    description: String,
    languages: Vec<String>,
    language_ids: Vec<String>,
    catalog_version: String,
    selected_version: Option<String>,
    installed_version: Option<String>,
    installation_state: InstallationStatePayload,
    last_error: Option<LanguageServerFailurePayload>,
    runtime_state: RuntimeStatePayload,
    session_count: usize,
    workspace_count: usize,
    runtime_error: Option<LanguageServerFailurePayload>,
    supported: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum InstallationStatePayload {
    NotInstalled,
    Installing,
    Installed,
    Uninstalling,
    Failed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum RuntimeStatePayload {
    Inactive,
    Running,
    Degraded,
    Crashed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerFailurePayload {
    code: String,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerHoverPayload {
    contents: Vec<LanguageServerHoverContentPayload>,
    range: Option<LanguageServerRangePayload>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerDiagnosticPayload {
    range: LanguageServerRangePayload,
    severity: Option<LanguageServerDiagnosticSeverityPayload>,
    message: String,
    source: Option<String>,
    code: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerCompletionListPayload {
    items: Vec<LanguageServerCompletionItemPayload>,
    is_incomplete: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerCompletionItemPayload {
    server_id: String,
    label: String,
    label_detail: Option<String>,
    label_description: Option<String>,
    kind: Option<u32>,
    detail: Option<String>,
    documentation: Option<LanguageServerHoverContentPayload>,
    sort_text: Option<String>,
    filter_text: Option<String>,
    insert_text: String,
    insert_text_is_snippet: bool,
    text_edit: Option<LanguageServerCompletionTextEditPayload>,
    additional_text_edits: Vec<LanguageServerCompletionTextEditPayload>,
    commit_characters: Vec<String>,
    preselect: bool,
    deprecated: bool,
    raw: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerCompletionTextEditPayload {
    insert: LanguageServerRangePayload,
    replace: LanguageServerRangePayload,
    new_text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerColorInformationPayload {
    server_id: String,
    range: LanguageServerRangePayload,
    color: LanguageServerColorPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerColorPresentationPayload {
    label: String,
    text_edit: Option<LanguageServerCompletionTextEditPayload>,
    additional_text_edits: Vec<LanguageServerCompletionTextEditPayload>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerTextEditPayload {
    range: LanguageServerRangePayload,
    new_text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageServerDiagnosticSeverityPayload {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerHoverContentPayload {
    kind: LanguageServerMarkupKindPayload,
    value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageServerMarkupKindPayload {
    PlainText,
    Markdown,
}

impl LanguageServerListSnapshot {
    pub(crate) fn new(statuses: Vec<core::language_servers::LanguageServerStatus>) -> Self {
        Self {
            servers: statuses
                .into_iter()
                .map(LanguageServerSnapshot::from_status)
                .collect(),
        }
    }
}

impl LanguageServerSnapshot {
    pub(crate) fn from_status(status: core::language_servers::LanguageServerStatus) -> Self {
        Self {
            id: status.id,
            name: status.name,
            description: status.description,
            languages: status.languages,
            language_ids: status.language_ids,
            catalog_version: status.catalog_version,
            selected_version: status.selected_version,
            installed_version: status.installed_version,
            installation_state: match status.installation_state {
                core::language_servers::LanguageServerInstallationState::NotInstalled => {
                    InstallationStatePayload::NotInstalled
                }
                core::language_servers::LanguageServerInstallationState::Installing => {
                    InstallationStatePayload::Installing
                }
                core::language_servers::LanguageServerInstallationState::Installed => {
                    InstallationStatePayload::Installed
                }
                core::language_servers::LanguageServerInstallationState::Uninstalling => {
                    InstallationStatePayload::Uninstalling
                }
                core::language_servers::LanguageServerInstallationState::Failed => {
                    InstallationStatePayload::Failed
                }
            },
            last_error: status.last_error.map(|error| LanguageServerFailurePayload {
                code: error.code,
                message: error.message,
            }),
            runtime_state: match status.runtime_state {
                core::language_servers::LanguageServerRuntimeState::Inactive => {
                    RuntimeStatePayload::Inactive
                }
                core::language_servers::LanguageServerRuntimeState::Running => {
                    RuntimeStatePayload::Running
                }
                core::language_servers::LanguageServerRuntimeState::Degraded => {
                    RuntimeStatePayload::Degraded
                }
                core::language_servers::LanguageServerRuntimeState::Crashed => {
                    RuntimeStatePayload::Crashed
                }
            },
            session_count: status.session_count,
            workspace_count: status.workspace_count,
            runtime_error: status
                .runtime_error
                .map(|error| LanguageServerFailurePayload {
                    code: error.code,
                    message: error.message,
                }),
            supported: status.supported,
        }
    }
}

impl LanguageServerChangePayload {
    pub(crate) fn into_core(self) -> core::language_servers::LanguageServerChange {
        core::language_servers::LanguageServerChange {
            range: self.range.into_core(),
            text: self.text,
        }
    }
}

impl LanguageServerPositionPayload {
    pub(crate) fn into_core(self) -> core::language_servers::LanguageServerPosition {
        core::language_servers::LanguageServerPosition {
            line: self.line,
            character: self.character,
        }
    }

    fn from_core(position: core::language_servers::LanguageServerPosition) -> Self {
        Self {
            line: position.line,
            character: position.character,
        }
    }
}

impl LanguageServerRangePayload {
    pub(crate) fn into_core(self) -> core::language_servers::LanguageServerRange {
        core::language_servers::LanguageServerRange {
            start: self.start.into_core(),
            end: self.end.into_core(),
        }
    }

    fn from_core(range: core::language_servers::LanguageServerRange) -> Self {
        Self {
            start: LanguageServerPositionPayload::from_core(range.start),
            end: LanguageServerPositionPayload::from_core(range.end),
        }
    }
}

impl LanguageServerHoverPayload {
    pub(crate) fn from_core(hover: core::language_servers::LanguageServerHover) -> Self {
        Self {
            contents: hover
                .contents
                .into_iter()
                .map(|content| LanguageServerHoverContentPayload {
                    kind: match content.kind {
                        core::language_servers::LanguageServerMarkupKind::PlainText => {
                            LanguageServerMarkupKindPayload::PlainText
                        }
                        core::language_servers::LanguageServerMarkupKind::Markdown => {
                            LanguageServerMarkupKindPayload::Markdown
                        }
                    },
                    value: content.value,
                })
                .collect(),
            range: hover.range.map(LanguageServerRangePayload::from_core),
        }
    }
}

impl LanguageServerDiagnosticPayload {
    pub(crate) fn from_core(diagnostic: core::language_servers::LanguageServerDiagnostic) -> Self {
        Self {
            range: LanguageServerRangePayload::from_core(diagnostic.range),
            severity: diagnostic.severity.map(|severity| match severity {
                core::language_servers::LanguageServerDiagnosticSeverity::Error => {
                    LanguageServerDiagnosticSeverityPayload::Error
                }
                core::language_servers::LanguageServerDiagnosticSeverity::Warning => {
                    LanguageServerDiagnosticSeverityPayload::Warning
                }
                core::language_servers::LanguageServerDiagnosticSeverity::Information => {
                    LanguageServerDiagnosticSeverityPayload::Information
                }
                core::language_servers::LanguageServerDiagnosticSeverity::Hint => {
                    LanguageServerDiagnosticSeverityPayload::Hint
                }
            }),
            message: diagnostic.message,
            source: diagnostic.source,
            code: diagnostic.code,
        }
    }
}

impl LanguageServerCompletionListPayload {
    pub(crate) fn from_core(
        completion: core::language_servers::LanguageServerCompletionList,
    ) -> Self {
        Self {
            items: completion
                .items
                .into_iter()
                .map(LanguageServerCompletionItemPayload::from_core)
                .collect(),
            is_incomplete: completion.is_incomplete,
        }
    }
}

impl LanguageServerCompletionItemPayload {
    pub(crate) fn from_core(item: core::language_servers::LanguageServerCompletionItem) -> Self {
        Self {
            server_id: item.server_id,
            label: item.label,
            label_detail: item.label_detail,
            label_description: item.label_description,
            kind: item.kind,
            detail: item.detail,
            documentation: item.documentation.map(|documentation| {
                LanguageServerHoverContentPayload {
                    kind: match documentation.kind {
                        core::language_servers::LanguageServerMarkupKind::PlainText => {
                            LanguageServerMarkupKindPayload::PlainText
                        }
                        core::language_servers::LanguageServerMarkupKind::Markdown => {
                            LanguageServerMarkupKindPayload::Markdown
                        }
                    },
                    value: documentation.value,
                }
            }),
            sort_text: item.sort_text,
            filter_text: item.filter_text,
            insert_text: item.insert_text,
            insert_text_is_snippet: item.insert_text_is_snippet,
            text_edit: item
                .text_edit
                .map(LanguageServerCompletionTextEditPayload::from_core),
            additional_text_edits: item
                .additional_text_edits
                .into_iter()
                .map(LanguageServerCompletionTextEditPayload::from_core)
                .collect(),
            commit_characters: item.commit_characters,
            preselect: item.preselect,
            deprecated: item.deprecated,
            raw: item.raw,
        }
    }
}

impl LanguageServerCompletionTextEditPayload {
    fn from_core(edit: core::language_servers::LanguageServerCompletionTextEdit) -> Self {
        Self {
            insert: LanguageServerRangePayload::from_core(edit.insert),
            replace: LanguageServerRangePayload::from_core(edit.replace),
            new_text: edit.new_text,
        }
    }
}

impl LanguageServerTextEditPayload {
    pub(crate) fn from_core(edit: core::language_servers::LanguageServerTextEdit) -> Self {
        Self {
            range: LanguageServerRangePayload::from_core(edit.range),
            new_text: edit.new_text,
        }
    }
}

impl LanguageServerColorPayload {
    pub(crate) fn into_core(self) -> core::language_servers::LanguageServerColor {
        core::language_servers::LanguageServerColor {
            red: self.red,
            green: self.green,
            blue: self.blue,
            alpha: self.alpha,
        }
    }

    fn from_core(color: core::language_servers::LanguageServerColor) -> Self {
        Self {
            red: color.red,
            green: color.green,
            blue: color.blue,
            alpha: color.alpha,
        }
    }
}

impl LanguageServerColorInformationPayload {
    pub(crate) fn from_core(color: core::language_servers::LanguageServerColorInformation) -> Self {
        Self {
            server_id: color.server_id,
            range: LanguageServerRangePayload::from_core(color.range),
            color: LanguageServerColorPayload::from_core(color.color),
        }
    }
}

impl LanguageServerColorPresentationPayload {
    pub(crate) fn from_core(
        presentation: core::language_servers::LanguageServerColorPresentation,
    ) -> Self {
        Self {
            label: presentation.label,
            text_edit: presentation
                .text_edit
                .map(LanguageServerCompletionTextEditPayload::from_core),
            additional_text_edits: presentation
                .additional_text_edits
                .into_iter()
                .map(LanguageServerCompletionTextEditPayload::from_core)
                .collect(),
        }
    }
}

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AnyJson;
use super::ids::{TabIdParam, WorkspaceIdParam};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerParams {
    pub(crate) server_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedToolingCapabilitiesParams {
    pub(crate) documents: Vec<ResolvedToolingDocumentParams>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedToolingDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) language_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenLanguageServerDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) tab_id: TabIdParam,
    pub(crate) language_id: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeLanguageServerDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) changes: Vec<LanguageServerChangePayload>,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloseLanguageServerDocumentParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerHoverParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) position: LanguageServerPositionPayload,
}

pub(crate) type LanguageServerPositionParams = LanguageServerHoverParams;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerReferencesParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) position: LanguageServerPositionPayload,
    pub(crate) include_declaration: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerWorkspaceSymbolsParams {
    pub(crate) query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveLanguageServerWorkspaceSymbolParams {
    pub(crate) server_id: String,
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) raw: AnyJson,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerDiagnosticsParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveLanguageServerCompletionParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) server_id: String,
    pub(crate) raw: AnyJson,
}

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerRenameParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) position: LanguageServerPositionPayload,
    pub(crate) new_name: String,
    pub(crate) server_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerCodeActionsParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) range: LanguageServerRangePayload,
    pub(crate) context: AnyJson,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveLanguageServerCodeActionParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) action_id: u64,
    pub(crate) server_id: String,
    pub(crate) raw: AnyJson,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StageLanguageServerCodeActionParams {
    pub(crate) action: LanguageServerCodeActionPayload,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecuteLanguageServerCommandParams {
    pub(crate) workspace_id: WorkspaceIdParam,
    pub(crate) path: String,
    pub(crate) generation: u64,
    pub(crate) version: i64,
    pub(crate) server_id: String,
    pub(crate) authorization: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceEditTransactionParams {
    pub(crate) transaction_id: u64,
    pub(crate) authorization: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveWorkspaceEditRecoveryParams {
    pub(crate) transaction_id: u64,
    pub(crate) authorization: String,
    pub(crate) intent: WorkspaceEditRecoveryIntentPayload,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkspaceEditRecoveryIntentPayload {
    RetryRollback,
    Finalize,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceEditTransactionStatusPayload {
    transaction_id: u64,
    phase: WorkspaceEditTransactionPhasePayload,
    retry_rollback: bool,
    can_finalize: bool,
    requires_acknowledgement: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceEditRecoveryPayload {
    authorization: String,
    #[serde(flatten)]
    status: WorkspaceEditTransactionStatusPayload,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
enum WorkspaceEditTransactionPhasePayload {
    Staged,
    Committed,
    FinishingCommitted,
    CommittedCleanupRequired,
    RolledBack,
    RecoveryRequired,
    FinishedCommitted,
    FinishedRolledBack,
    FinishedUncommitted,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerColorPayload {
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrustLanguageServerWorkspaceParams {
    pub(crate) workspace_id: WorkspaceIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerChangePayload {
    pub(crate) range: LanguageServerRangePayload,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerPositionPayload {
    pub(crate) line: u32,
    pub(crate) character: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerRangePayload {
    pub(crate) start: LanguageServerPositionPayload,
    pub(crate) end: LanguageServerPositionPayload,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerListSnapshot {
    servers: Vec<LanguageServerSnapshot>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedToolingSnapshotPayload {
    revision: u64,
    documents: Vec<ResolvedToolingDocumentPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedToolingDocumentPayload {
    workspace_id: WorkspaceIdParam,
    path: String,
    language_id: String,
    supported: bool,
    external_available: bool,
    features: Vec<ResolvedToolingFeaturePayload>,
    formatter_id: Option<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedToolingFeaturePayload {
    feature: LanguageToolFeaturePayload,
    owners: Vec<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageToolFeaturePayload {
    Completion,
    Hover,
    SignatureHelp,
    Navigation,
    References,
    Symbols,
    Diagnostics,
    Colors,
    Formatting,
    Rename,
    CodeActions,
}

#[derive(Debug, JsonSchema, Serialize)]
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
    logs: Vec<LanguageServerLogPayload>,
    supported: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
enum InstallationStatePayload {
    NotInstalled,
    Installing,
    Installed,
    Uninstalling,
    Failed,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
enum RuntimeStatePayload {
    Inactive,
    Restarting,
    Running,
    Degraded,
    Crashed,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerFailurePayload {
    code: String,
    message: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerLogPayload {
    kind: LanguageServerLogKindPayload,
    message: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageServerLogKindPayload {
    Stderr,
    Runtime,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerHoverPayload {
    contents: Vec<LanguageServerHoverContentPayload>,
    range: Option<LanguageServerRangePayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerDiagnosticPayload {
    range: LanguageServerRangePayload,
    severity: Option<LanguageServerDiagnosticSeverityPayload>,
    message: String,
    source: Option<String>,
    code: Option<String>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerDiagnosticSnapshotPayload {
    server_id: String,
    diagnostics: Vec<LanguageServerDiagnosticPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerCompletionListPayload {
    items: Vec<LanguageServerCompletionItemPayload>,
    is_incomplete: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
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
    raw: AnyJson,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerCompletionTextEditPayload {
    insert: LanguageServerRangePayload,
    replace: LanguageServerRangePayload,
    new_text: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerColorInformationPayload {
    server_id: String,
    range: LanguageServerRangePayload,
    color: LanguageServerColorPayload,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerColorPresentationPayload {
    label: String,
    text_edit: Option<LanguageServerCompletionTextEditPayload>,
    additional_text_edits: Vec<LanguageServerCompletionTextEditPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerTextEditPayload {
    range: LanguageServerRangePayload,
    new_text: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerPrepareRenamePayload {
    server_id: String,
    range: Option<LanguageServerRangePayload>,
    placeholder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerCodeActionPayload {
    action_id: u64,
    server_id: String,
    title: String,
    kind: Option<String>,
    is_preferred: bool,
    disabled_reason: Option<String>,
    resolve_supported: bool,
    command_authorization: Option<String>,
    raw: AnyJson,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagedWorkspaceEditPayload {
    transaction_id: u64,
    authorization: String,
    documents: Vec<StagedWorkspaceEditDocumentPayload>,
    operations: Vec<StagedWorkspaceEditOperationPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub(crate) directive: Option<Box<WorkspaceEditDirectivePayload>>,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum WorkspaceEditDirectivePayload {
    #[serde(rename = "applyOpenModels")]
    Apply {
        transaction_id: u64,
        models: Vec<WorkspaceEditModelDirectivePayload>,
    },
    #[serde(rename = "undoOpenModels")]
    Undo {
        transaction_id: u64,
        models: Vec<WorkspaceEditModelDirectivePayload>,
    },
    #[serde(rename = "reconcileCommittedModels")]
    ReconcileCommitted { transaction_id: u64 },
    #[serde(rename = "reconcileRolledBackModels")]
    ReconcileRolledBack { transaction_id: u64 },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceEditModelDirectivePayload {
    workspace_id: WorkspaceIdParam,
    original_path: String,
    path: Option<String>,
    generation: u64,
    version: i64,
    original_text: String,
    text: String,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum StagedWorkspaceEditOperationPayload {
    TextDocument {
        document: usize,
    },
    CreateFile {
        #[schemars(rename = "workspaceId")]
        workspace_id: WorkspaceIdParam,
        path: String,
    },
    RenameFile {
        #[schemars(rename = "workspaceId")]
        workspace_id: WorkspaceIdParam,
        #[schemars(rename = "oldPath")]
        old_path: String,
        #[schemars(rename = "newPath")]
        new_path: String,
    },
    DeleteFile {
        #[schemars(rename = "workspaceId")]
        workspace_id: WorkspaceIdParam,
        path: String,
        recursive: bool,
    },
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct StagedWorkspaceEditDocumentPayload {
    workspace_id: WorkspaceIdParam,
    path: String,
    original_path: String,
    original_text: String,
    new_text: String,
    generation: Option<u64>,
    version: Option<i64>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerSignatureHelpPayload {
    signatures: Vec<LanguageServerSignatureInformationPayload>,
    active_signature: Option<u32>,
    active_parameter: Option<u32>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerSignatureInformationPayload {
    label: String,
    documentation: Option<LanguageServerHoverContentPayload>,
    parameters: Vec<LanguageServerParameterInformationPayload>,
    active_parameter: Option<u32>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerParameterInformationPayload {
    label: LanguageServerParameterLabelPayload,
    documentation: Option<LanguageServerHoverContentPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(untagged)]
enum LanguageServerParameterLabelPayload {
    Text(String),
    Utf16Offsets([u32; 2]),
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerLocationPayload {
    workspace_id: WorkspaceIdParam,
    path: String,
    range: LanguageServerRangePayload,
    selection_range: LanguageServerRangePayload,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerDocumentSymbolPayload {
    name: String,
    detail: Option<String>,
    kind: u32,
    deprecated: bool,
    range: LanguageServerRangePayload,
    selection_range: LanguageServerRangePayload,
    children: Vec<LanguageServerDocumentSymbolPayload>,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanguageServerWorkspaceSymbolPayload {
    server_id: String,
    workspace_id: WorkspaceIdParam,
    name: String,
    kind: u32,
    container_name: Option<String>,
    deprecated: bool,
    location: Option<LanguageServerLocationPayload>,
    raw: AnyJson,
    resolve_supported: bool,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageServerDiagnosticSeverityPayload {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServerHoverContentPayload {
    kind: LanguageServerMarkupKindPayload,
    value: String,
}

#[derive(Debug, JsonSchema, Serialize)]
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

impl ResolvedToolingCapabilitiesParams {
    pub(crate) fn into_core(self) -> Vec<core::language_servers::ResolvedToolingDocumentRequest> {
        self.documents
            .into_iter()
            .map(
                |document| core::language_servers::ResolvedToolingDocumentRequest {
                    workspace_id: document.workspace_id.into(),
                    path: document.path,
                    language_id: document.language_id,
                },
            )
            .collect()
    }
}

impl ResolvedToolingSnapshotPayload {
    pub(crate) fn from_core(snapshot: core::language_servers::ResolvedToolingSnapshot) -> Self {
        Self {
            revision: snapshot.revision,
            documents: snapshot
                .documents
                .into_iter()
                .map(ResolvedToolingDocumentPayload::from_core)
                .collect(),
        }
    }
}

impl ResolvedToolingDocumentPayload {
    fn from_core(document: core::language_servers::ResolvedToolingDocument) -> Self {
        Self {
            workspace_id: document.workspace_id.into(),
            path: document.path,
            language_id: document.language_id,
            supported: document.supported,
            external_available: document.external_available,
            features: document
                .features
                .into_iter()
                .map(ResolvedToolingFeaturePayload::from_core)
                .collect(),
            formatter_id: document.formatter_id,
        }
    }
}

impl ResolvedToolingFeaturePayload {
    fn from_core(feature: core::language_servers::ResolvedToolingFeature) -> Self {
        Self {
            feature: match feature.feature {
                core::language_servers::LanguageToolFeature::Completion => {
                    LanguageToolFeaturePayload::Completion
                }
                core::language_servers::LanguageToolFeature::Hover => {
                    LanguageToolFeaturePayload::Hover
                }
                core::language_servers::LanguageToolFeature::SignatureHelp => {
                    LanguageToolFeaturePayload::SignatureHelp
                }
                core::language_servers::LanguageToolFeature::Navigation => {
                    LanguageToolFeaturePayload::Navigation
                }
                core::language_servers::LanguageToolFeature::References => {
                    LanguageToolFeaturePayload::References
                }
                core::language_servers::LanguageToolFeature::Symbols => {
                    LanguageToolFeaturePayload::Symbols
                }
                core::language_servers::LanguageToolFeature::Diagnostics => {
                    LanguageToolFeaturePayload::Diagnostics
                }
                core::language_servers::LanguageToolFeature::Colors => {
                    LanguageToolFeaturePayload::Colors
                }
                core::language_servers::LanguageToolFeature::Formatting => {
                    LanguageToolFeaturePayload::Formatting
                }
                core::language_servers::LanguageToolFeature::Rename => {
                    LanguageToolFeaturePayload::Rename
                }
                core::language_servers::LanguageToolFeature::CodeActions => {
                    LanguageToolFeaturePayload::CodeActions
                }
            },
            owners: feature.owners,
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
                core::language_servers::LanguageServerRuntimeState::Restarting => {
                    RuntimeStatePayload::Restarting
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
            logs: status
                .logs
                .into_iter()
                .map(|log| LanguageServerLogPayload {
                    kind: match log.kind {
                        core::language_servers::LanguageServerLogKind::Stderr => {
                            LanguageServerLogKindPayload::Stderr
                        }
                        core::language_servers::LanguageServerLogKind::Runtime => {
                            LanguageServerLogKindPayload::Runtime
                        }
                    },
                    message: log.message,
                })
                .collect(),
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

impl LanguageServerDiagnosticSnapshotPayload {
    pub(crate) fn from_core(
        snapshot: core::language_servers::LanguageServerDiagnosticSnapshot,
    ) -> Self {
        Self {
            server_id: snapshot.server_id,
            diagnostics: snapshot
                .diagnostics
                .into_iter()
                .map(LanguageServerDiagnosticPayload::from_core)
                .collect(),
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
            raw: item.raw.into(),
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

impl LanguageServerPrepareRenamePayload {
    pub(crate) fn from_core(rename: core::language_servers::LanguageServerPrepareRename) -> Self {
        Self {
            server_id: rename.server_id,
            range: rename.range.map(LanguageServerRangePayload::from_core),
            placeholder: rename.placeholder,
        }
    }
}

impl LanguageServerCodeActionPayload {
    pub(crate) fn from_core(action: core::language_servers::LanguageServerCodeAction) -> Self {
        Self {
            action_id: action.action_id,
            server_id: action.server_id,
            title: action.title,
            kind: action.kind,
            is_preferred: action.is_preferred,
            disabled_reason: action.disabled_reason,
            resolve_supported: action.resolve_supported,
            command_authorization: action.command_authorization,
            raw: action.raw.into(),
        }
    }

    pub(crate) fn into_core(self) -> core::language_servers::LanguageServerCodeAction {
        core::language_servers::LanguageServerCodeAction {
            action_id: self.action_id,
            server_id: self.server_id,
            title: self.title,
            kind: self.kind,
            is_preferred: self.is_preferred,
            disabled_reason: self.disabled_reason,
            resolve_supported: self.resolve_supported,
            command_authorization: self.command_authorization,
            raw: self.raw.into_inner(),
        }
    }
}

impl StagedWorkspaceEditPayload {
    pub(crate) fn from_core(edit: core::language_servers::StagedWorkspaceEdit) -> Self {
        Self {
            transaction_id: edit.transaction_id,
            authorization: edit.authorization,
            documents: edit
                .documents
                .into_iter()
                .map(|document| StagedWorkspaceEditDocumentPayload {
                    workspace_id: document.workspace_id.into(),
                    path: document.path,
                    original_path: document.original_path,
                    original_text: document.original_text,
                    new_text: document.new_text,
                    generation: document.generation,
                    version: document.version,
                })
                .collect(),
            operations: edit
                .operations
                .into_iter()
                .map(|operation| match operation {
                    core::language_servers::StagedWorkspaceEditOperation::TextDocument {
                        document,
                    } => StagedWorkspaceEditOperationPayload::TextDocument { document },
                    core::language_servers::StagedWorkspaceEditOperation::CreateFile {
                        workspace_id,
                        path,
                    } => StagedWorkspaceEditOperationPayload::CreateFile {
                        workspace_id: workspace_id.into(),
                        path,
                    },
                    core::language_servers::StagedWorkspaceEditOperation::RenameFile {
                        workspace_id,
                        old_path,
                        new_path,
                    } => StagedWorkspaceEditOperationPayload::RenameFile {
                        workspace_id: workspace_id.into(),
                        old_path,
                        new_path,
                    },
                    core::language_servers::StagedWorkspaceEditOperation::DeleteFile {
                        workspace_id,
                        path,
                        recursive,
                    } => StagedWorkspaceEditOperationPayload::DeleteFile {
                        workspace_id: workspace_id.into(),
                        path,
                        recursive,
                    },
                })
                .collect(),
            directive: None,
        }
    }

    pub(crate) fn from_core_with_directive(
        edit: core::language_servers::StagedWorkspaceEdit,
        directive: core::language_servers::WorkspaceEditDirective,
    ) -> Self {
        let mut payload = Self::from_core(edit);
        payload.directive = Some(Box::new(WorkspaceEditDirectivePayload::from_core(
            directive,
        )));
        payload
    }
}

impl WorkspaceEditDirectivePayload {
    fn from_core(directive: core::language_servers::WorkspaceEditDirective) -> Self {
        match directive {
            core::language_servers::WorkspaceEditDirective::ApplyOpenModels {
                transaction_id,
                models,
            } => Self::Apply {
                transaction_id,
                models: models
                    .into_iter()
                    .map(WorkspaceEditModelDirectivePayload::from_core)
                    .collect(),
            },
            core::language_servers::WorkspaceEditDirective::UndoOpenModels {
                transaction_id,
                models,
            } => Self::Undo {
                transaction_id,
                models: models
                    .into_iter()
                    .map(WorkspaceEditModelDirectivePayload::from_core)
                    .collect(),
            },
            core::language_servers::WorkspaceEditDirective::ReconcileCommittedModels {
                transaction_id,
            } => Self::ReconcileCommitted { transaction_id },
            core::language_servers::WorkspaceEditDirective::ReconcileRolledBackModels {
                transaction_id,
            } => Self::ReconcileRolledBack { transaction_id },
        }
    }
}

impl WorkspaceEditModelDirectivePayload {
    fn from_core(model: core::language_servers::WorkspaceEditModelDirective) -> Self {
        Self {
            workspace_id: model.workspace_id.into(),
            original_path: model.original_path,
            path: model.path,
            generation: model.generation,
            version: model.version,
            original_text: model.original_text,
            text: model.text,
        }
    }
}

impl WorkspaceEditTransactionStatusPayload {
    pub(crate) fn from_core(
        status: core::language_servers::WorkspaceEditTransactionStatus,
    ) -> Self {
        use core::language_servers::WorkspaceEditTransactionPhase as Phase;

        Self {
            transaction_id: status.transaction_id,
            phase: match status.phase {
                Phase::Staged => WorkspaceEditTransactionPhasePayload::Staged,
                Phase::Committed => WorkspaceEditTransactionPhasePayload::Committed,
                Phase::FinishingCommitted => {
                    WorkspaceEditTransactionPhasePayload::FinishingCommitted
                }
                Phase::CommittedCleanupRequired => {
                    WorkspaceEditTransactionPhasePayload::CommittedCleanupRequired
                }
                Phase::RolledBack => WorkspaceEditTransactionPhasePayload::RolledBack,
                Phase::RecoveryRequired => WorkspaceEditTransactionPhasePayload::RecoveryRequired,
                Phase::FinishedCommitted => WorkspaceEditTransactionPhasePayload::FinishedCommitted,
                Phase::FinishedRolledBack => {
                    WorkspaceEditTransactionPhasePayload::FinishedRolledBack
                }
                Phase::FinishedUncommitted => {
                    WorkspaceEditTransactionPhasePayload::FinishedUncommitted
                }
            },
            retry_rollback: status.retry_rollback,
            can_finalize: status.can_finalize,
            requires_acknowledgement: status.requires_acknowledgement,
        }
    }
}

impl WorkspaceEditRecoveryPayload {
    pub(crate) fn from_core(recovery: core::language_servers::WorkspaceEditRecovery) -> Self {
        Self {
            authorization: recovery.authorization,
            status: WorkspaceEditTransactionStatusPayload::from_core(recovery.status),
        }
    }
}

impl LanguageServerSignatureHelpPayload {
    pub(crate) fn from_core(help: core::language_servers::LanguageServerSignatureHelp) -> Self {
        Self {
            signatures: help
                .signatures
                .into_iter()
                .map(|signature| LanguageServerSignatureInformationPayload {
                    label: signature.label,
                    documentation: signature.documentation.map(LanguageServerHoverContentPayload::from_core),
                    parameters: signature
                        .parameters
                        .into_iter()
                        .map(|parameter| LanguageServerParameterInformationPayload {
                            label: match parameter.label {
                                core::language_servers::LanguageServerParameterLabel::Text(label) => LanguageServerParameterLabelPayload::Text(label),
                                core::language_servers::LanguageServerParameterLabel::Utf16Offsets(start, end) => LanguageServerParameterLabelPayload::Utf16Offsets([start, end]),
                            },
                            documentation: parameter.documentation.map(LanguageServerHoverContentPayload::from_core),
                        })
                        .collect(),
                    active_parameter: signature.active_parameter,
                })
                .collect(),
            active_signature: help.active_signature,
            active_parameter: help.active_parameter,
        }
    }
}

impl LanguageServerHoverContentPayload {
    fn from_core(content: core::language_servers::LanguageServerHoverContent) -> Self {
        Self {
            kind: match content.kind {
                core::language_servers::LanguageServerMarkupKind::PlainText => {
                    LanguageServerMarkupKindPayload::PlainText
                }
                core::language_servers::LanguageServerMarkupKind::Markdown => {
                    LanguageServerMarkupKindPayload::Markdown
                }
            },
            value: content.value,
        }
    }
}

impl LanguageServerLocationPayload {
    pub(crate) fn from_core(location: core::language_servers::LanguageServerLocation) -> Self {
        Self {
            workspace_id: location.workspace_id.into(),
            path: location.path,
            range: LanguageServerRangePayload::from_core(location.range),
            selection_range: LanguageServerRangePayload::from_core(location.selection_range),
        }
    }
}

impl LanguageServerDocumentSymbolPayload {
    pub(crate) fn from_core(symbol: core::language_servers::LanguageServerDocumentSymbol) -> Self {
        Self {
            name: symbol.name,
            detail: symbol.detail,
            kind: symbol.kind,
            deprecated: symbol.deprecated,
            range: LanguageServerRangePayload::from_core(symbol.range),
            selection_range: LanguageServerRangePayload::from_core(symbol.selection_range),
            children: symbol.children.into_iter().map(Self::from_core).collect(),
        }
    }
}

impl LanguageServerWorkspaceSymbolPayload {
    pub(crate) fn from_core(symbol: core::language_servers::LanguageServerWorkspaceSymbol) -> Self {
        Self {
            server_id: symbol.server_id,
            workspace_id: symbol.workspace_id.into(),
            name: symbol.name,
            kind: symbol.kind,
            container_name: symbol.container_name,
            deprecated: symbol.deprecated,
            location: symbol
                .location
                .map(LanguageServerLocationPayload::from_core),
            raw: symbol.raw.into(),
            resolve_supported: symbol.resolve_supported,
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

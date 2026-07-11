mod catalog;
mod edits;
mod installation;
mod manager;
mod runtime;

use std::error::Error as StdError;
use std::fmt;
use std::path::Path;

use crate::tree::WorkspaceId;

pub use catalog::{LanguageServerDefinition, language_server_catalog};
pub use installation::LanguageServerPaths;
pub use manager::LanguageServerManager;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageServerInstallationState {
    NotInstalled,
    Installing,
    Installed,
    Uninstalling,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageServerRuntimeState {
    Inactive,
    Running,
    Degraded,
    Crashed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LanguageServerRuntimeStatus {
    pub state: LanguageServerRuntimeState,
    pub session_count: usize,
    pub workspace_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerFailure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerStatus {
    pub id: String,
    pub name: String,
    pub description: String,
    pub languages: Vec<String>,
    pub language_ids: Vec<String>,
    pub catalog_version: String,
    pub selected_version: Option<String>,
    pub installed_version: Option<String>,
    pub installation_state: LanguageServerInstallationState,
    pub last_error: Option<LanguageServerFailure>,
    pub runtime_state: LanguageServerRuntimeState,
    pub session_count: usize,
    pub workspace_count: usize,
    pub runtime_error: Option<LanguageServerFailure>,
    pub supported: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageServerPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageServerRange {
    pub start: LanguageServerPosition,
    pub end: LanguageServerPosition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerChange {
    pub range: LanguageServerRange,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageServerMarkupKind {
    PlainText,
    Markdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerHoverContent {
    pub kind: LanguageServerMarkupKind,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerHover {
    pub contents: Vec<LanguageServerHoverContent>,
    pub range: Option<LanguageServerRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageServerDiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerDiagnostic {
    pub range: LanguageServerRange,
    pub severity: Option<LanguageServerDiagnosticSeverity>,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerCompletionList {
    pub items: Vec<LanguageServerCompletionItem>,
    pub is_incomplete: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerCompletionItem {
    pub server_id: String,
    pub label: String,
    pub label_detail: Option<String>,
    pub label_description: Option<String>,
    pub kind: Option<u32>,
    pub detail: Option<String>,
    pub documentation: Option<LanguageServerHoverContent>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
    pub insert_text: String,
    pub insert_text_is_snippet: bool,
    pub text_edit: Option<LanguageServerCompletionTextEdit>,
    pub additional_text_edits: Vec<LanguageServerCompletionTextEdit>,
    pub commit_characters: Vec<String>,
    pub preselect: bool,
    pub deprecated: bool,
    pub raw: serde_json::Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerCompletionTextEdit {
    pub insert: LanguageServerRange,
    pub replace: LanguageServerRange,
    pub new_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerTextEdit {
    pub range: LanguageServerRange,
    pub new_text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageServerFormattingOptions {
    pub tab_size: u32,
    pub insert_spaces: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerCompletionRequest {
    pub position: LanguageServerPosition,
    pub trigger_kind: u32,
    pub trigger_character: Option<String>,
    pub filter: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LanguageServerColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerColorInformation {
    pub server_id: String,
    pub range: LanguageServerRange,
    pub color: LanguageServerColor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerColorPresentation {
    pub label: String,
    pub text_edit: Option<LanguageServerCompletionTextEdit>,
    pub additional_text_edits: Vec<LanguageServerCompletionTextEdit>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LanguageServerColorPresentationRequest {
    pub server_id: String,
    pub range: LanguageServerRange,
    pub color: LanguageServerColor,
}

#[derive(Clone, Copy)]
pub struct LanguageServerDocumentOpen<'a> {
    pub workspace_id: WorkspaceId,
    pub workspace_root: &'a Path,
    pub absolute_path: &'a Path,
    pub relative_path: &'a str,
    pub language_id: &'a str,
    pub generation: u64,
    pub version: i64,
    pub text: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LanguageServerError {
    ManagerUnavailable,
    UnknownServer(String),
    UnsupportedPlatform,
    OperationInProgress,
    WorkerBusy,
    WorkerUnavailable(String),
    Download(String),
    DownloadTooLarge,
    ChecksumMismatch,
    InvalidArtifact(String),
    InvalidManifest(String),
    Persistence(String),
    Io(String),
    LanguageNotSupported(String),
    ServerNotInstalled(String),
    DocumentNotOpen,
    StaleDocument,
    ServerStart(String),
    ServerExited,
    RequestTimeout,
    Protocol(String),
    InvalidDocument(String),
    ContentModified,
    FeatureNotSupported(String),
    WorkspaceNotTrusted,
    WorkspaceClosed,
    RuntimeUnavailable(String),
    PackageInstall(String),
}

impl LanguageServerError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ManagerUnavailable => "language_servers.unavailable",
            Self::UnknownServer(_) => "language_servers.unknown_server",
            Self::UnsupportedPlatform => "language_servers.unsupported_platform",
            Self::OperationInProgress => "language_servers.operation_in_progress",
            Self::WorkerBusy => "language_servers.worker_busy",
            Self::WorkerUnavailable(_) => "language_servers.worker_unavailable",
            Self::Download(_) => "language_servers.download_failed",
            Self::DownloadTooLarge => "language_servers.download_too_large",
            Self::ChecksumMismatch => "language_servers.checksum_mismatch",
            Self::InvalidArtifact(_) => "language_servers.invalid_artifact",
            Self::InvalidManifest(_) => "language_servers.invalid_manifest",
            Self::Persistence(_) => "language_servers.persistence_failed",
            Self::Io(_) => "language_servers.io_failed",
            Self::LanguageNotSupported(_) => "language_servers.language_not_supported",
            Self::ServerNotInstalled(_) => "language_servers.server_not_installed",
            Self::DocumentNotOpen => "language_servers.document_not_open",
            Self::StaleDocument => "language_servers.stale_document",
            Self::ServerStart(_) => "language_servers.server_start_failed",
            Self::ServerExited => "language_servers.server_exited",
            Self::RequestTimeout => "language_servers.request_timeout",
            Self::Protocol(_) => "language_servers.protocol_failed",
            Self::InvalidDocument(_) => "language_servers.invalid_document",
            Self::ContentModified => "language_servers.content_modified",
            Self::FeatureNotSupported(_) => "language_servers.feature_not_supported",
            Self::WorkspaceNotTrusted => "language_servers.workspace_not_trusted",
            Self::WorkspaceClosed => "language_servers.workspace_closed",
            Self::RuntimeUnavailable(_) => "language_servers.runtime_unavailable",
            Self::PackageInstall(_) => "language_servers.package_install_failed",
        }
    }

    pub(crate) fn io(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl fmt::Display for LanguageServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManagerUnavailable => {
                formatter.write_str("language server manager is unavailable")
            }
            Self::UnknownServer(id) => write!(formatter, "unknown language server `{id}`"),
            Self::UnsupportedPlatform => formatter
                .write_str("this language server is not available for the current platform"),
            Self::OperationInProgress => {
                formatter.write_str("a language server operation is already in progress")
            }
            Self::WorkerBusy => formatter.write_str("the language server installer is busy"),
            Self::WorkerUnavailable(message) => write!(
                formatter,
                "language server installer is unavailable: {message}"
            ),
            Self::Download(message) => {
                write!(formatter, "language server download failed: {message}")
            }
            Self::DownloadTooLarge => {
                formatter.write_str("language server download exceeded the size limit")
            }
            Self::ChecksumMismatch => {
                formatter.write_str("language server download checksum did not match the catalog")
            }
            Self::InvalidArtifact(message) => {
                write!(formatter, "language server artifact is invalid: {message}")
            }
            Self::InvalidManifest(message) => write!(
                formatter,
                "language server installation manifest is invalid: {message}"
            ),
            Self::Persistence(message) => write!(
                formatter,
                "language server configuration could not be saved: {message}"
            ),
            Self::Io(message) => write!(
                formatter,
                "language server filesystem operation failed: {message}"
            ),
            Self::LanguageNotSupported(language) => {
                write!(formatter, "no language server supports `{language}`")
            }
            Self::ServerNotInstalled(server) => {
                write!(formatter, "language server `{server}` is not installed")
            }
            Self::DocumentNotOpen => formatter.write_str("language server document is not open"),
            Self::StaleDocument => formatter.write_str("language server document version is stale"),
            Self::ServerStart(message) => {
                write!(formatter, "language server could not start: {message}")
            }
            Self::ServerExited => formatter.write_str("language server exited"),
            Self::RequestTimeout => formatter.write_str("language server request timed out"),
            Self::Protocol(message) => {
                write!(formatter, "language server protocol failed: {message}")
            }
            Self::InvalidDocument(message) => {
                write!(formatter, "language server document is invalid: {message}")
            }
            Self::ContentModified => {
                formatter.write_str("language server content changed during the request")
            }
            Self::FeatureNotSupported(feature) => {
                write!(formatter, "no active language server supports {feature}")
            }
            Self::WorkspaceNotTrusted => {
                formatter.write_str("workspace must be trusted before starting language servers")
            }
            Self::WorkspaceClosed => formatter.write_str("language server workspace is closed"),
            Self::RuntimeUnavailable(message) => {
                write!(
                    formatter,
                    "language server runtime is unavailable: {message}"
                )
            }
            Self::PackageInstall(message) => {
                write!(
                    formatter,
                    "language server package installation failed: {message}"
                )
            }
        }
    }
}

impl StdError for LanguageServerError {}

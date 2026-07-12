mod catalog;
mod installation;
mod manager;
mod process;

use std::error::Error as StdError;
use std::fmt;

pub use catalog::{FormatterDefinition, formatter_catalog};
pub use installation::FormatterPaths;
pub use manager::FormatterManager;

pub struct DocumentFormattingRequest<'a> {
    pub workspace_id: crate::tree::WorkspaceId,
    pub path: &'a str,
    pub language_id: &'a str,
    pub generation: u64,
    pub version: i64,
    pub text: &'a str,
    pub options: crate::language_servers::LanguageServerFormattingOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatterInstallationState {
    NotInstalled,
    Installing,
    Installed,
    Uninstalling,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatterFailure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatterStatus {
    pub id: String,
    pub name: String,
    pub description: String,
    pub languages: Vec<String>,
    pub language_ids: Vec<String>,
    pub extensions: Vec<String>,
    pub filenames: Vec<String>,
    pub priority: usize,
    pub catalog_version: String,
    pub installed_version: Option<String>,
    pub installation_state: FormatterInstallationState,
    pub last_error: Option<FormatterFailure>,
    pub supported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatterError {
    ManagerUnavailable,
    UnknownFormatter(String),
    UnsupportedPlatform,
    OperationInProgress,
    InvalidPreferences(String),
    WorkerBusy,
    WorkerUnavailable(String),
    Install(String),
    InvalidInstallation(String),
    ChecksumMismatch,
    InvalidDocument(String),
    Execution(String),
    Timeout,
    OutputTooLarge,
    InvalidOutput(String),
    Io(String),
}

impl FormatterError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ManagerUnavailable => "formatters.unavailable",
            Self::UnknownFormatter(_) => "formatters.unknown_formatter",
            Self::UnsupportedPlatform => "formatters.unsupported_platform",
            Self::OperationInProgress => "formatters.operation_in_progress",
            Self::InvalidPreferences(_) => "formatters.invalid_preferences",
            Self::WorkerBusy => "formatters.worker_busy",
            Self::WorkerUnavailable(_) => "formatters.worker_unavailable",
            Self::Install(_) => "formatters.install_failed",
            Self::InvalidInstallation(_) => "formatters.invalid_installation",
            Self::ChecksumMismatch => "formatters.checksum_mismatch",
            Self::InvalidDocument(_) => "formatters.invalid_document",
            Self::Execution(_) => "formatters.execution_failed",
            Self::Timeout => "formatters.timeout",
            Self::OutputTooLarge => "formatters.output_too_large",
            Self::InvalidOutput(_) => "formatters.invalid_output",
            Self::Io(_) => "formatters.io_failed",
        }
    }

    pub(crate) fn io(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl fmt::Display for FormatterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManagerUnavailable => formatter.write_str("formatter manager is unavailable"),
            Self::UnknownFormatter(id) => write!(formatter, "unknown formatter `{id}`"),
            Self::UnsupportedPlatform => {
                formatter.write_str("formatter is not available for this platform or runtime")
            }
            Self::OperationInProgress => {
                formatter.write_str("a formatter operation is already in progress")
            }
            Self::InvalidPreferences(message) => {
                write!(formatter, "formatter preferences are invalid: {message}")
            }
            Self::WorkerBusy => formatter.write_str("formatter installer is busy"),
            Self::WorkerUnavailable(message) => {
                write!(formatter, "formatter installer is unavailable: {message}")
            }
            Self::Install(message) => write!(formatter, "formatter installation failed: {message}"),
            Self::InvalidInstallation(message) => {
                write!(formatter, "formatter installation is invalid: {message}")
            }
            Self::ChecksumMismatch => {
                formatter.write_str("formatter package integrity did not match the catalog")
            }
            Self::InvalidDocument(message) => {
                write!(formatter, "formatter document is invalid: {message}")
            }
            Self::Execution(message) => write!(formatter, "formatter failed: {message}"),
            Self::Timeout => formatter.write_str("formatter timed out"),
            Self::OutputTooLarge => formatter.write_str("formatter output exceeded the size limit"),
            Self::InvalidOutput(message) => {
                write!(formatter, "formatter returned invalid output: {message}")
            }
            Self::Io(message) => write!(
                formatter,
                "formatter filesystem operation failed: {message}"
            ),
        }
    }
}

impl StdError for FormatterError {}

#[derive(Debug)]
pub enum FormattingError {
    Formatter(FormatterError),
    LanguageServer(crate::language_servers::LanguageServerError),
}

impl FormattingError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Formatter(error) => error.code(),
            Self::LanguageServer(error) => error.code(),
        }
    }
}

impl fmt::Display for FormattingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Formatter(error) => error.fmt(formatter),
            Self::LanguageServer(error) => error.fmt(formatter),
        }
    }
}

impl StdError for FormattingError {}

impl From<FormatterError> for FormattingError {
    fn from(error: FormatterError) -> Self {
        Self::Formatter(error)
    }
}

impl From<crate::language_servers::LanguageServerError> for FormattingError {
    fn from(error: crate::language_servers::LanguageServerError) -> Self {
        Self::LanguageServer(error)
    }
}

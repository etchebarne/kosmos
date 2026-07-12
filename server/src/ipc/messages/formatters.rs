use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormatterParams {
    pub(crate) formatter_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormatterPrioritiesParams {
    pub(crate) formatter_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormatterListSnapshot {
    formatters: Vec<FormatterSnapshot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FormatterSnapshot {
    id: String,
    name: String,
    description: String,
    languages: Vec<String>,
    language_ids: Vec<String>,
    extensions: Vec<String>,
    filenames: Vec<String>,
    priority: usize,
    catalog_version: String,
    installed_version: Option<String>,
    installation_state: InstallationStatePayload,
    last_error: Option<FormatterFailurePayload>,
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
struct FormatterFailurePayload {
    code: String,
    message: String,
}

impl FormatterListSnapshot {
    pub(crate) fn new(statuses: Vec<core::formatters::FormatterStatus>) -> Self {
        Self {
            formatters: statuses
                .into_iter()
                .map(FormatterSnapshot::from_status)
                .collect(),
        }
    }
}

impl FormatterSnapshot {
    pub(crate) fn from_status(status: core::formatters::FormatterStatus) -> Self {
        Self {
            id: status.id,
            name: status.name,
            description: status.description,
            languages: status.languages,
            language_ids: status.language_ids,
            extensions: status.extensions,
            filenames: status.filenames,
            priority: status.priority,
            catalog_version: status.catalog_version,
            installed_version: status.installed_version,
            installation_state: match status.installation_state {
                core::formatters::FormatterInstallationState::NotInstalled => {
                    InstallationStatePayload::NotInstalled
                }
                core::formatters::FormatterInstallationState::Installing => {
                    InstallationStatePayload::Installing
                }
                core::formatters::FormatterInstallationState::Installed => {
                    InstallationStatePayload::Installed
                }
                core::formatters::FormatterInstallationState::Uninstalling => {
                    InstallationStatePayload::Uninstalling
                }
                core::formatters::FormatterInstallationState::Failed => {
                    InstallationStatePayload::Failed
                }
            },
            last_error: status.last_error.map(|error| FormatterFailurePayload {
                code: error.code,
                message: error.message,
            }),
            supported: status.supported,
        }
    }
}

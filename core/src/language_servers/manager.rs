use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::persistence::StateStore;

use super::catalog::{
    language_server_catalog, language_server_definition, language_servers_for_language,
};
use super::installation::{
    LanguageServerPaths, clean_stale_versions, clean_temporary_directories, finish_uninstall,
    install, installation_supported, installed_executable, installed_version, restore_uninstall,
    uninstall,
};
use super::runtime::LanguageServerRuntime;
use super::{
    LanguageServerChange, LanguageServerColorInformation, LanguageServerColorPresentation,
    LanguageServerColorPresentationRequest, LanguageServerCompletionItem,
    LanguageServerCompletionList, LanguageServerCompletionRequest, LanguageServerDiagnostic,
    LanguageServerDocumentOpen, LanguageServerError, LanguageServerFailure,
    LanguageServerFormattingOptions, LanguageServerHover, LanguageServerInstallationState,
    LanguageServerPosition, LanguageServerRuntimeState, LanguageServerStatus,
    LanguageServerTextEdit,
};
use crate::tree::WorkspaceId;

const COMMAND_QUEUE_CAPACITY: usize = 8;

#[derive(Clone)]
pub struct LanguageServerManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    paths: LanguageServerPaths,
    store: StateStore,
    entries: Arc<Mutex<BTreeMap<String, ManagerEntry>>>,
    sender: SyncSender<ManagerCommand>,
    runtime: LanguageServerRuntime,
    trusted_workspaces: Mutex<std::collections::HashSet<std::path::PathBuf>>,
}

struct WorkerContext {
    paths: LanguageServerPaths,
    store: StateStore,
    entries: Arc<Mutex<BTreeMap<String, ManagerEntry>>>,
}

#[derive(Debug)]
struct ManagerEntry {
    selected_version: Option<String>,
    installed_version: Option<String>,
    operation: ManagerOperation,
    operation_epoch: u64,
    runtime_failure: Option<LanguageServerFailure>,
}

#[derive(Debug)]
enum ManagerOperation {
    Idle,
    Installing,
    Uninstalling,
    Failed(LanguageServerFailure),
}

enum ManagerCommand {
    Install(&'static str),
    Uninstall(&'static str, String),
}

impl LanguageServerManager {
    pub fn open(
        paths: LanguageServerPaths,
        store: StateStore,
    ) -> Result<Self, LanguageServerError> {
        let selections = store
            .language_server_selections()
            .map_err(|error| LanguageServerError::Persistence(error.to_string()))?;
        let trusted_workspaces = store
            .trusted_language_server_workspaces()
            .map_err(|error| LanguageServerError::Persistence(error.to_string()))?
            .into_iter()
            .collect();
        clean_temporary_directories(&paths);
        let mut entries = BTreeMap::new();
        for definition in language_server_catalog() {
            let mut selected_version = selections.get(definition.id).cloned();
            let installed_version =
                installed_version(&paths, definition, selected_version.as_deref());
            clean_stale_versions(&paths, definition, selected_version.as_deref());
            if selected_version.is_some() && installed_version.is_none() {
                store
                    .clear_language_server_selection(definition.id)
                    .map_err(|error| LanguageServerError::Persistence(error.to_string()))?;
                selected_version = None;
            }
            entries.insert(
                definition.id.to_owned(),
                ManagerEntry {
                    selected_version,
                    installed_version,
                    operation: ManagerOperation::Idle,
                    operation_epoch: 0,
                    runtime_failure: None,
                },
            );
        }
        let (sender, receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let entries = Arc::new(Mutex::new(entries));
        let manager_store = store.clone();
        let worker = WorkerContext {
            paths: paths.clone(),
            store,
            entries: Arc::clone(&entries),
        };
        let manager = Self {
            inner: Arc::new(ManagerInner {
                paths,
                store: manager_store,
                entries,
                sender,
                runtime: LanguageServerRuntime::default(),
                trusted_workspaces: Mutex::new(trusted_workspaces),
            }),
        };
        thread::Builder::new()
            .name("kosmos-language-server-installer".to_owned())
            .spawn(move || worker.run(receiver))
            .map_err(|error| LanguageServerError::WorkerUnavailable(error.to_string()))?;

        Ok(manager)
    }

    pub fn list(&self) -> Vec<LanguageServerStatus> {
        language_server_catalog()
            .iter()
            .filter_map(|definition| self.status(definition.id).ok())
            .collect()
    }

    pub fn status(&self, server_id: &str) -> Result<LanguageServerStatus, LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        let entries = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = entries
            .get(server_id)
            .expect("catalog entries are initialized with the manager");

        let runtime = self.inner.runtime.server_status(server_id);
        Ok(LanguageServerStatus {
            id: definition.id.to_owned(),
            name: definition.name.to_owned(),
            description: definition.description.to_owned(),
            languages: definition
                .languages
                .iter()
                .map(|language| (*language).to_owned())
                .collect(),
            language_ids: definition
                .language_ids
                .iter()
                .map(|language_id| (*language_id).to_owned())
                .collect(),
            catalog_version: definition.version.to_owned(),
            selected_version: entry.selected_version.clone(),
            installed_version: entry.installed_version.clone(),
            installation_state: match entry.operation {
                ManagerOperation::Idle if entry.installed_version.is_some() => {
                    LanguageServerInstallationState::Installed
                }
                ManagerOperation::Idle => LanguageServerInstallationState::NotInstalled,
                ManagerOperation::Installing => LanguageServerInstallationState::Installing,
                ManagerOperation::Uninstalling => LanguageServerInstallationState::Uninstalling,
                ManagerOperation::Failed(_) => LanguageServerInstallationState::Failed,
            },
            last_error: match &entry.operation {
                ManagerOperation::Failed(error) => Some(error.clone()),
                _ => None,
            },
            runtime_state: runtime.state,
            session_count: runtime.session_count,
            workspace_count: runtime.workspace_count,
            runtime_error: entry.runtime_failure.clone().or_else(|| {
                matches!(runtime.state, LanguageServerRuntimeState::Crashed).then(|| {
                    LanguageServerFailure {
                        code: "language_servers.server_exited".to_owned(),
                        message: "Language server process exited unexpectedly.".to_owned(),
                    }
                })
            }),
            supported: installation_supported(definition),
        })
    }

    pub fn install(&self, server_id: &str) -> Result<LanguageServerStatus, LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        if !installation_supported(definition) {
            return Err(LanguageServerError::UnsupportedPlatform);
        }

        {
            let mut entries = self
                .inner
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let entry = entries
                .get_mut(server_id)
                .expect("catalog entries are initialized with the manager");
            if matches!(
                entry.operation,
                ManagerOperation::Installing | ManagerOperation::Uninstalling
            ) {
                return Err(LanguageServerError::OperationInProgress);
            }
            let installation_is_valid = installed_executable(
                &self.inner.paths,
                definition,
                entry.selected_version.as_deref(),
            )
            .is_some();
            if entry.installed_version.as_deref() == Some(definition.version)
                && installation_is_valid
            {
                entry.operation = ManagerOperation::Idle;
                drop(entries);
                return self.status(server_id);
            }
            if !installation_is_valid {
                entry.installed_version = None;
            }
            entry.operation = ManagerOperation::Installing;
            entry.operation_epoch = entry.operation_epoch.wrapping_add(1);
            entry.runtime_failure = None;
        }
        self.inner.runtime.close_server(definition.id);

        if let Err(error) = self.try_send(ManagerCommand::Install(definition.id)) {
            fail_entry(&self.inner.entries, definition.id, &error);
            return Err(error);
        }
        self.status(server_id)
    }

    pub fn uninstall(&self, server_id: &str) -> Result<LanguageServerStatus, LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        let version = {
            let mut entries = self
                .inner
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let entry = entries
                .get_mut(server_id)
                .expect("catalog entries are initialized with the manager");
            if matches!(
                entry.operation,
                ManagerOperation::Installing | ManagerOperation::Uninstalling
            ) {
                return Err(LanguageServerError::OperationInProgress);
            }
            let Some(version) = entry.selected_version.clone() else {
                entry.operation = ManagerOperation::Idle;
                drop(entries);
                return self.status(server_id);
            };
            entry.operation = ManagerOperation::Uninstalling;
            entry.operation_epoch = entry.operation_epoch.wrapping_add(1);
            entry.runtime_failure = None;
            version
        };
        self.inner.runtime.close_server(definition.id);

        if let Err(error) = self.try_send(ManagerCommand::Uninstall(definition.id, version)) {
            fail_entry(&self.inner.entries, definition.id, &error);
            return Err(error);
        }
        self.status(server_id)
    }

    pub fn open_document(
        &self,
        document: LanguageServerDocumentOpen<'_>,
    ) -> Result<bool, LanguageServerError> {
        let definitions = language_servers_for_language(document.language_id).collect::<Vec<_>>();
        if definitions.is_empty() {
            return Err(LanguageServerError::LanguageNotSupported(
                document.language_id.to_owned(),
            ));
        }
        if !self
            .inner
            .trusted_workspaces
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(document.workspace_root)
        {
            return Err(LanguageServerError::WorkspaceNotTrusted);
        }
        let launches = {
            let entries = self
                .inner
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            definitions
                .iter()
                .filter_map(|definition| {
                    let entry = entries
                        .get(definition.id)
                        .expect("catalog entries are initialized with the manager");
                    if !matches!(entry.operation, ManagerOperation::Idle) {
                        return None;
                    }
                    installed_executable(
                        &self.inner.paths,
                        definition,
                        entry.selected_version.as_deref(),
                    )
                    .map(|executable| {
                        (
                            *definition,
                            executable,
                            entry.operation_epoch,
                            entry.selected_version.clone(),
                        )
                    })
                })
                .collect::<Vec<_>>()
        };
        if launches.is_empty() {
            return Err(LanguageServerError::ServerNotInstalled(
                definitions
                    .iter()
                    .map(|definition| definition.id)
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }

        let launch_count = launches.len();
        let results = thread::scope(|scope| {
            launches
                .into_iter()
                .map(|launch| {
                    scope.spawn(move || {
                        let (definition, executable, operation_epoch, selected_version) = launch;
                        let result =
                            self.inner
                                .runtime
                                .open_document(definition, &executable, document);
                        (definition, operation_epoch, selected_version, result)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|worker| {
                    worker
                        .join()
                        .expect("language server startup worker panicked")
                })
                .collect::<Vec<_>>()
        });
        let mut opened = 0;
        let mut first_error = None;
        for (definition, operation_epoch, selected_version, result) in results {
            match result {
                Ok(()) => {
                    let mut entries = self
                        .inner
                        .entries
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    let operation_is_current = entries.get(definition.id).is_some_and(|entry| {
                        matches!(entry.operation, ManagerOperation::Idle)
                            && entry.operation_epoch == operation_epoch
                            && entry.selected_version == selected_version
                    });
                    if operation_is_current {
                        if let Some(entry) = entries.get_mut(definition.id) {
                            entry.runtime_failure = None;
                        }
                        opened += 1;
                    } else {
                        drop(entries);
                        self.inner.runtime.close_server(definition.id);
                    }
                }
                Err(error) => {
                    set_runtime_failure(&self.inner.entries, definition.id, &error);
                    first_error.get_or_insert(error);
                }
            }
        }
        if opened > 0 {
            Ok(opened == launch_count)
        } else {
            Err(first_error.unwrap_or(LanguageServerError::OperationInProgress))
        }
    }

    pub fn change_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        changes: &[LanguageServerChange],
        text: &str,
    ) -> Result<(), LanguageServerError> {
        self.inner
            .runtime
            .change_document(workspace_id, path, generation, version, changes, text)
    }

    pub fn close_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
    ) -> Result<(), LanguageServerError> {
        self.inner
            .runtime
            .close_document(workspace_id, path, generation)
    }

    pub fn save_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        text: &str,
    ) -> Result<(), LanguageServerError> {
        self.inner
            .runtime
            .save_document(workspace_id, path, generation, version, text)
    }

    pub fn hover(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
    ) -> Result<Option<LanguageServerHover>, LanguageServerError> {
        self.inner
            .runtime
            .hover(workspace_id, path, generation, version, position)
    }

    pub fn diagnostics(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Option<Vec<LanguageServerDiagnostic>>, LanguageServerError> {
        self.inner
            .runtime
            .diagnostics(workspace_id, path, generation, version)
    }

    pub fn completion(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerCompletionRequest,
    ) -> Result<LanguageServerCompletionList, LanguageServerError> {
        self.inner
            .runtime
            .completion(workspace_id, path, generation, version, request)
    }

    pub fn resolve_completion(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        server_id: &str,
        raw: serde_json::Value,
    ) -> Result<LanguageServerCompletionItem, LanguageServerError> {
        self.inner.runtime.resolve_completion(
            workspace_id,
            path,
            generation,
            version,
            server_id,
            raw,
        )
    }

    pub fn document_colors(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Vec<LanguageServerColorInformation>, LanguageServerError> {
        self.inner
            .runtime
            .document_colors(workspace_id, path, generation, version)
    }

    pub fn color_presentations(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerColorPresentationRequest,
    ) -> Result<Vec<LanguageServerColorPresentation>, LanguageServerError> {
        self.inner
            .runtime
            .color_presentations(workspace_id, path, generation, version, request)
    }

    pub fn formatting(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        options: LanguageServerFormattingOptions,
    ) -> Result<Vec<LanguageServerTextEdit>, LanguageServerError> {
        self.inner
            .runtime
            .formatting(workspace_id, path, generation, version, options)
    }

    pub fn retain_workspaces(&self, workspace_ids: &std::collections::HashSet<WorkspaceId>) {
        self.inner.runtime.retain_workspaces(workspace_ids);
    }

    pub fn restart(&self, server_id: &str) -> Result<LanguageServerStatus, LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        let mut entries = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = entries
            .get_mut(server_id)
            .expect("catalog entries are initialized with the manager");
        if entry.installed_version.is_none() {
            return Err(LanguageServerError::ServerNotInstalled(
                server_id.to_owned(),
            ));
        }
        entry.runtime_failure = None;
        drop(entries);
        self.inner.runtime.close_server(definition.id);
        self.status(server_id)
    }

    pub fn trust_workspace(&self, workspace_root: &Path) -> Result<(), LanguageServerError> {
        self.inner
            .store
            .trust_language_server_workspace(workspace_root)
            .map_err(|error| LanguageServerError::Persistence(error.to_string()))?;
        self.inner
            .trusted_workspaces
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(workspace_root.to_path_buf());
        Ok(())
    }

    fn try_send(&self, command: ManagerCommand) -> Result<(), LanguageServerError> {
        self.inner
            .sender
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => LanguageServerError::WorkerBusy,
                TrySendError::Disconnected(_) => {
                    LanguageServerError::WorkerUnavailable("installer worker stopped".to_owned())
                }
            })
    }
}

impl WorkerContext {
    fn run(&self, receiver: Receiver<ManagerCommand>) {
        while let Ok(command) = receiver.recv() {
            let (server_id, result) = match command {
                ManagerCommand::Install(server_id) => (server_id, self.install_now(server_id)),
                ManagerCommand::Uninstall(server_id, version) => {
                    (server_id, self.uninstall_now(server_id, &version))
                }
            };
            match result {
                Ok(()) => finish_entry(&self.entries, server_id),
                Err(error) => fail_entry(&self.entries, server_id, &error),
            }
        }
    }

    fn install_now(&self, server_id: &str) -> Result<(), LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        let previous_version = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(server_id)
            .and_then(|entry| entry.selected_version.clone());
        install(&self.paths, definition)?;
        self.store
            .select_language_server_version(server_id, definition.version)
            .map_err(|error| LanguageServerError::Persistence(error.to_string()))?;

        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = entries
            .get_mut(server_id)
            .expect("catalog entries are initialized with the manager");
        entry.selected_version = Some(definition.version.to_owned());
        entry.installed_version = Some(definition.version.to_owned());
        drop(entries);

        if let Some(previous_version) = previous_version
            && previous_version != definition.version
            && let Ok(trash) = uninstall(&self.paths, definition, &previous_version)
        {
            finish_uninstall(trash);
        }
        Ok(())
    }

    fn uninstall_now(&self, server_id: &str, version: &str) -> Result<(), LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        let trash = uninstall(&self.paths, definition, version)?;
        if let Err(error) = self.store.clear_language_server_selection(server_id) {
            if let Some(trash) = trash.as_deref() {
                restore_uninstall(&self.paths, definition, version, trash)?;
            }
            return Err(LanguageServerError::Persistence(error.to_string()));
        }

        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = entries
            .get_mut(server_id)
            .expect("catalog entries are initialized with the manager");
        entry.selected_version = None;
        entry.installed_version = None;
        drop(entries);
        finish_uninstall(trash);
        Ok(())
    }
}

fn finish_entry(entries: &Mutex<BTreeMap<String, ManagerEntry>>, server_id: &str) {
    let mut entries = entries.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = entries.get_mut(server_id) {
        entry.operation = ManagerOperation::Idle;
    }
}

fn fail_entry(
    entries: &Mutex<BTreeMap<String, ManagerEntry>>,
    server_id: &str,
    error: &LanguageServerError,
) {
    let mut entries = entries.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = entries.get_mut(server_id) {
        entry.operation = ManagerOperation::Failed(LanguageServerFailure {
            code: error.code().to_owned(),
            message: error.to_string(),
        });
    }
}

fn set_runtime_failure(
    entries: &Mutex<BTreeMap<String, ManagerEntry>>,
    server_id: &str,
    error: &LanguageServerError,
) {
    let mut entries = entries.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = entries.get_mut(server_id) {
        entry.runtime_failure = Some(LanguageServerFailure {
            code: error.code().to_owned(),
            message: error.to_string(),
        });
    }
}

impl fmt::Debug for LanguageServerManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LanguageServerManager")
            .field("paths", &self.inner.paths)
            .finish_non_exhaustive()
    }
}

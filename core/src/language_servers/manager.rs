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
use super::edits::WorkspaceEditTransactions;
use super::installation::{
    LanguageServerPaths, clean_stale_versions, clean_temporary_directories, finish_uninstall,
    install, installation_supported, installed_executable, installed_version, restore_uninstall,
    uninstall,
};
use super::runtime::LanguageServerRuntime;
use super::{
    LanguageServerChange, LanguageServerCodeAction, LanguageServerCodeActionRequest,
    LanguageServerCodeActionResolveRequest, LanguageServerColorInformation,
    LanguageServerColorPresentation, LanguageServerColorPresentationRequest,
    LanguageServerCompletionItem, LanguageServerCompletionList, LanguageServerCompletionRequest,
    LanguageServerCompletionResolveRequest, LanguageServerDiagnosticSnapshot,
    LanguageServerDocumentOpen, LanguageServerDocumentSymbol, LanguageServerError,
    LanguageServerExecuteCommandRequest, LanguageServerFailure, LanguageServerFormattingOptions,
    LanguageServerHover, LanguageServerInstallationState, LanguageServerLocation,
    LanguageServerPosition, LanguageServerPrepareRename, LanguageServerRequestCancellation,
    LanguageServerRuntimeState, LanguageServerSignatureHelp, LanguageServerStatus,
    LanguageServerTextEdit, LanguageServerWorkspaceSymbol,
    LanguageServerWorkspaceSymbolResolveRequest, StagedWorkspaceEdit, WorkspaceEditError,
    WorkspaceEditOpenDocument, WorkspaceEditRoot,
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
    runtime: Arc<LanguageServerRuntime>,
    workspace_edits: Arc<WorkspaceEditTransactions>,
    trusted_workspaces: Mutex<std::collections::HashSet<std::path::PathBuf>>,
}

struct WorkerContext {
    paths: LanguageServerPaths,
    store: StateStore,
    entries: Arc<Mutex<BTreeMap<String, ManagerEntry>>>,
    runtime: Arc<LanguageServerRuntime>,
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
        let workspace_edits = Arc::new(WorkspaceEditTransactions::new());
        let runtime = LanguageServerRuntime::new(Arc::clone(&workspace_edits));
        let worker = WorkerContext {
            paths: paths.clone(),
            store,
            entries: Arc::clone(&entries),
            runtime: Arc::clone(&runtime),
        };
        let manager = Self {
            inner: Arc::new(ManagerInner {
                paths,
                store: manager_store,
                entries,
                sender,
                runtime,
                workspace_edits,
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

    pub fn set_event_sink(&self, sink: Arc<dyn crate::events::CoreEventSink>) {
        self.inner.runtime.set_event_sink(sink);
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
            logs: self.inner.runtime.logs(server_id),
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
        if let Err(error) = self.try_send(ManagerCommand::Install(definition.id)) {
            fail_entry(&self.inner.entries, definition.id, &error);
            self.inner.runtime.emit_status(definition.id);
            return Err(error);
        }
        self.inner.runtime.emit_status(definition.id);
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
        if let Err(error) = self.try_send(ManagerCommand::Uninstall(definition.id, version)) {
            fail_entry(&self.inner.entries, definition.id, &error);
            self.inner.runtime.emit_status(definition.id);
            return Err(error);
        }
        self.inner.runtime.emit_status(definition.id);
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
                        entry.operation_epoch == operation_epoch
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
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerHover>, LanguageServerError> {
        self.inner.runtime.hover(
            workspace_id,
            path,
            generation,
            version,
            position,
            cancellation,
        )
    }

    pub fn signature_help(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerSignatureHelp>, LanguageServerError> {
        self.inner.runtime.signature_help(
            workspace_id,
            path,
            generation,
            version,
            position,
            cancellation,
        )
    }

    pub fn definition(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.inner.runtime.definition(
            workspace_id,
            path,
            generation,
            version,
            position,
            cancellation,
        )
    }

    pub fn declaration(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.inner.runtime.declaration(
            workspace_id,
            path,
            generation,
            version,
            position,
            cancellation,
        )
    }

    pub fn type_definition(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.inner.runtime.type_definition(
            workspace_id,
            path,
            generation,
            version,
            position,
            cancellation,
        )
    }

    pub fn implementation(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.inner.runtime.implementation(
            workspace_id,
            path,
            generation,
            version,
            position,
            cancellation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn references(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        include_declaration: bool,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.inner.runtime.references(
            workspace_id,
            path,
            generation,
            version,
            position,
            include_declaration,
            cancellation,
        )
    }

    pub fn document_symbols(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerDocumentSymbol>, LanguageServerError> {
        self.inner
            .runtime
            .document_symbols(workspace_id, path, generation, version, cancellation)
    }

    pub fn workspace_symbols(
        &self,
        query: &str,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerWorkspaceSymbol>, LanguageServerError> {
        self.inner.runtime.workspace_symbols(query, cancellation)
    }

    pub fn resolve_workspace_symbol(
        &self,
        request: LanguageServerWorkspaceSymbolResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerWorkspaceSymbol, LanguageServerError> {
        self.inner
            .runtime
            .resolve_workspace_symbol(request, cancellation)
    }

    pub fn diagnostics(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Option<Vec<LanguageServerDiagnosticSnapshot>>, LanguageServerError> {
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
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionList, LanguageServerError> {
        self.inner.runtime.completion(
            workspace_id,
            path,
            generation,
            version,
            request,
            cancellation,
        )
    }

    pub fn resolve_completion(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: LanguageServerCompletionResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionItem, LanguageServerError> {
        self.inner.runtime.resolve_completion(
            workspace_id,
            path,
            generation,
            version,
            request,
            cancellation,
        )
    }

    pub fn document_colors(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorInformation>, LanguageServerError> {
        self.inner
            .runtime
            .document_colors(workspace_id, path, generation, version, cancellation)
    }

    pub fn color_presentations(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerColorPresentationRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorPresentation>, LanguageServerError> {
        self.inner.runtime.color_presentations(
            workspace_id,
            path,
            generation,
            version,
            request,
            cancellation,
        )
    }

    pub fn formatting(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        options: LanguageServerFormattingOptions,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerTextEdit>, LanguageServerError> {
        self.inner.runtime.formatting(
            workspace_id,
            path,
            generation,
            version,
            options,
            cancellation,
        )
    }

    pub fn prepare_rename(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerPrepareRename>, LanguageServerError> {
        self.inner.runtime.prepare_rename(
            workspace_id,
            path,
            generation,
            version,
            position,
            cancellation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rename(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        new_name: &str,
        server_id: Option<&str>,
        roots: &[WorkspaceEditRoot],
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<StagedWorkspaceEdit, LanguageServerError> {
        let edit = self.inner.runtime.rename(
            workspace_id,
            path,
            generation,
            version,
            position,
            new_name,
            server_id,
            cancellation,
        )?;
        self.stage_workspace_edit(&edit, roots)
            .map_err(workspace_edit_language_error)
    }

    pub fn code_actions(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerCodeActionRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerCodeAction>, LanguageServerError> {
        self.inner.runtime.code_actions(
            workspace_id,
            path,
            generation,
            version,
            request,
            cancellation,
        )
    }

    pub fn resolve_code_action(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: LanguageServerCodeActionResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCodeAction, LanguageServerError> {
        self.inner.runtime.resolve_code_action(
            workspace_id,
            path,
            generation,
            version,
            request,
            cancellation,
        )
    }

    pub fn stage_code_action_edit(
        &self,
        action: &LanguageServerCodeAction,
        roots: &[WorkspaceEditRoot],
    ) -> Result<Option<StagedWorkspaceEdit>, WorkspaceEditError> {
        let workspace_id = self
            .inner
            .runtime
            .validate_code_action(action)
            .map_err(|error| WorkspaceEditError::Invalid(error.to_string()))?;
        let root = roots
            .iter()
            .find(|root| root.workspace_id == workspace_id)
            .cloned()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid(
                    "code action workspace root is no longer available".to_owned(),
                )
            })?;
        let staged = action
            .raw
            .get("edit")
            .map(|edit| self.stage_workspace_edit(edit, &[root]))
            .transpose()?;
        self.inner
            .runtime
            .bind_code_action_command_to_staged_edit(action.action_id, staged.as_ref())
            .map_err(|error| WorkspaceEditError::Invalid(error.to_string()))?;
        Ok(staged)
    }

    pub fn execute_command(
        &self,
        request: LanguageServerExecuteCommandRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<serde_json::Value, LanguageServerError> {
        self.inner.runtime.execute_command(request, cancellation)
    }

    pub fn stage_workspace_edit(
        &self,
        edit: &serde_json::Value,
        roots: &[WorkspaceEditRoot],
    ) -> Result<StagedWorkspaceEdit, WorkspaceEditError> {
        self.inner
            .workspace_edits
            .stage(edit, roots, &self.open_documents())
    }

    pub fn commit_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        self.inner.workspace_edits.commit_closed(
            transaction_id,
            authorization,
            &self.open_documents(),
        )
    }

    pub fn rollback_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        self.inner
            .workspace_edits
            .rollback(transaction_id, authorization)
    }

    pub fn finish_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        self.inner
            .workspace_edits
            .finish(transaction_id, authorization)
    }

    pub fn finalize_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<super::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.inner
            .workspace_edits
            .finalize(transaction_id, authorization)
    }

    pub fn workspace_edit_status(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<super::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.inner
            .workspace_edits
            .status(transaction_id, authorization)
    }

    pub fn claim_workspace_edit_owner(
        &self,
        transaction_id: u64,
        authorization: &str,
        owner: u64,
    ) -> Result<(), WorkspaceEditError> {
        self.inner
            .workspace_edits
            .claim_owner(transaction_id, authorization, owner)
    }

    pub fn cancel_owned_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
        owner: u64,
    ) -> Result<super::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.inner
            .workspace_edits
            .cancel_owned(transaction_id, authorization, owner)
    }

    pub fn disconnect_workspace_edit_owner(
        &self,
        owner: u64,
    ) -> Vec<super::WorkspaceEditTransactionStatus> {
        self.inner.workspace_edits.disconnect_owner(owner)
    }

    fn open_documents(&self) -> Vec<WorkspaceEditOpenDocument> {
        self.inner.runtime.open_documents()
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
        let selected_version = entry.selected_version.clone();
        let installation_is_valid =
            installed_executable(&self.inner.paths, definition, selected_version.as_deref())
                .is_some();
        if !installation_is_valid {
            entry.installed_version = None;
            return Err(LanguageServerError::ServerNotInstalled(
                server_id.to_owned(),
            ));
        }
        entry.installed_version = selected_version;
        entry.operation = ManagerOperation::Idle;
        entry.operation_epoch = entry.operation_epoch.wrapping_add(1);
        entry.runtime_failure = None;
        drop(entries);
        self.inner.runtime.restart_server(definition.id);
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

fn workspace_edit_language_error(error: WorkspaceEditError) -> LanguageServerError {
    LanguageServerError::InvalidDocument(error.to_string())
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
            self.runtime.emit_status(server_id);
        }
    }

    fn install_now(&self, server_id: &str) -> Result<(), LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        self.install_now_with(server_id, || install(&self.paths, definition))
    }

    fn install_now_with(
        &self,
        server_id: &str,
        installer: impl FnOnce() -> Result<(), LanguageServerError>,
    ) -> Result<(), LanguageServerError> {
        let definition = language_server_definition(server_id)
            .ok_or_else(|| LanguageServerError::UnknownServer(server_id.to_owned()))?;
        let previous_version = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(server_id)
            .and_then(|entry| entry.selected_version.clone());
        installer()?;
        if let Err(error) = self
            .store
            .select_language_server_version(server_id, definition.version)
        {
            if previous_version.as_deref() != Some(definition.version)
                && let Ok(trash) = uninstall(&self.paths, definition, definition.version)
            {
                finish_uninstall(trash);
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
        entry.selected_version = Some(definition.version.to_owned());
        entry.installed_version = Some(definition.version.to_owned());
        self.runtime.close_server(server_id);
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
        self.runtime.close_server(server_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn failed_update_keeps_the_previous_installation_selected_and_usable() {
        let fixture = TestFixture::new("failed-update");
        let result = fixture.worker.install_now_with(fixture.definition.id, || {
            Err(LanguageServerError::ChecksumMismatch)
        });
        let error = result.expect_err("the update should fail");
        fail_entry(&fixture.worker.entries, fixture.definition.id, &error);

        let entries = fixture.worker.entries.lock().unwrap();
        let entry = entries.get(fixture.definition.id).unwrap();
        assert_eq!(entry.selected_version.as_deref(), Some("previous"));
        assert_eq!(entry.installed_version.as_deref(), Some("previous"));
        assert!(matches!(entry.operation, ManagerOperation::Failed(_)));
        assert!(
            installed_executable(&fixture.paths, fixture.definition, Some("previous")).is_some()
        );
        drop(entries);

        let (sender, _receiver) = mpsc::sync_channel(1);
        let manager = LanguageServerManager {
            inner: Arc::new(ManagerInner {
                paths: fixture.paths.clone(),
                store: fixture.store.clone(),
                entries: Arc::clone(&fixture.worker.entries),
                sender,
                runtime: Arc::clone(&fixture.worker.runtime),
                workspace_edits: Arc::new(WorkspaceEditTransactions::new()),
                trusted_workspaces: Mutex::new(std::collections::HashSet::new()),
            }),
        };
        let recovered = manager.restart(fixture.definition.id).unwrap();
        assert_eq!(
            recovered.installation_state,
            LanguageServerInstallationState::Installed
        );
        assert_eq!(recovered.selected_version.as_deref(), Some("previous"));
    }

    #[test]
    fn persistence_failure_rolls_back_the_candidate_and_keeps_the_previous_selection() {
        let fixture = TestFixture::new("persistence-rollback");
        let database_backup = fixture.root.join("state-backup.sqlite");
        fs::rename(fixture.store.path(), &database_backup).unwrap();
        fs::create_dir(fixture.store.path()).unwrap();

        let result = fixture.worker.install_now_with(fixture.definition.id, || {
            write_test_installation(
                &fixture.paths,
                fixture.definition,
                fixture.definition.version,
            );
            Ok(())
        });

        assert!(matches!(result, Err(LanguageServerError::Persistence(_))));
        assert!(
            installed_executable(
                &fixture.paths,
                fixture.definition,
                Some(fixture.definition.version)
            )
            .is_none()
        );
        assert!(
            installed_executable(&fixture.paths, fixture.definition, Some("previous")).is_some()
        );

        fs::remove_dir(fixture.store.path()).unwrap();
        fs::rename(database_backup, fixture.store.path()).unwrap();
        assert_eq!(
            fixture
                .store
                .language_server_selections()
                .unwrap()
                .get(fixture.definition.id)
                .map(String::as_str),
            Some("previous")
        );
    }

    struct TestFixture {
        root: PathBuf,
        paths: LanguageServerPaths,
        store: StateStore,
        definition: &'static super::super::catalog::LanguageServerDefinition,
        worker: WorkerContext,
    }

    impl TestFixture {
        fn new(name: &str) -> Self {
            let root = test_directory(name);
            let paths = LanguageServerPaths::new(root.join("data"), root.join("cache"));
            paths.prepare().unwrap();
            let store = StateStore::open(root.join("state.sqlite")).unwrap();
            let definition = language_server_definition("typescript-language-server").unwrap();
            write_test_installation(&paths, definition, "previous");
            store
                .select_language_server_version(definition.id, "previous")
                .unwrap();
            let entries = Arc::new(Mutex::new(BTreeMap::from([(
                definition.id.to_owned(),
                ManagerEntry {
                    selected_version: Some("previous".to_owned()),
                    installed_version: Some("previous".to_owned()),
                    operation: ManagerOperation::Installing,
                    operation_epoch: 1,
                    runtime_failure: None,
                },
            )])));
            let worker = WorkerContext {
                paths: paths.clone(),
                store: store.clone(),
                entries,
                runtime: LanguageServerRuntime::new(Arc::new(WorkspaceEditTransactions::new())),
            };
            Self {
                root,
                paths,
                store,
                definition,
                worker,
            }
        }
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write_test_installation(
        paths: &LanguageServerPaths,
        definition: &super::super::catalog::LanguageServerDefinition,
        version: &str,
    ) {
        let directory = paths.data_directory().join(definition.id).join(version);
        let executable = directory.join(definition.executable);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let source = if version == definition.version {
            format!(
                "npm:{}",
                definition
                    .npm_packages
                    .iter()
                    .map(|package| package.spec)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            "npm:test".to_owned()
        };
        fs::write(
            directory.join("installation.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "serverId": definition.id,
                "version": version,
                "operatingSystem": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
                "sourceUrl": source,
                "sha256": "npm-package-lock",
                "executable": definition.executable,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kosmos-language-server-manager-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}

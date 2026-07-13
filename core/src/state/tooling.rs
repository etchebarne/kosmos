use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::formatters::{
    DocumentFormattingRequest, FormatterError, FormatterManager, FormatterStatus, FormattingError,
};
use crate::language_servers::{
    LanguageServerChange, LanguageServerCodeAction, LanguageServerCodeActionRequest,
    LanguageServerCodeActionResolveRequest, LanguageServerColorInformation,
    LanguageServerColorPresentation, LanguageServerColorPresentationRequest,
    LanguageServerCompletionItem, LanguageServerCompletionList, LanguageServerCompletionRequest,
    LanguageServerCompletionResolveRequest, LanguageServerDiagnosticSnapshot,
    LanguageServerDocumentOpen, LanguageServerDocumentSymbol, LanguageServerError,
    LanguageServerExecuteCommandRequest, LanguageServerHover, LanguageServerLocation,
    LanguageServerManager, LanguageServerPosition, LanguageServerPrepareRename,
    LanguageServerRequestCancellation, LanguageServerSignatureHelp, LanguageServerStatus,
    LanguageServerTextEdit, LanguageServerWorkspaceSymbol,
    LanguageServerWorkspaceSymbolResolveRequest, LanguageToolFeature, ResolvedToolingDocument,
    ResolvedToolingDocumentRequest, ResolvedToolingFeature, ResolvedToolingSnapshot,
    StagedWorkspaceEdit, WorkspaceEditError,
};
use crate::tabs::editor::normalize_path as normalize_editor_path;
use crate::tree::{TabId, Workspace, WorkspaceId};

use super::{State, full_document_edit};

impl State {
    pub fn attach_language_server_manager(&mut self, manager: LanguageServerManager) {
        let workspace_ids = self
            .workspaces
            .workspaces()
            .iter()
            .map(Workspace::id)
            .collect::<HashSet<_>>();
        manager.retain_workspaces(&workspace_ids);
        manager.set_tooling_capabilities(self.tooling_capabilities.clone());
        self.language_server_manager = Some(manager);
    }

    pub fn set_event_sink(&self, sink: Arc<dyn crate::events::CoreEventSink>) {
        self.tooling_capabilities.set_event_sink(Arc::clone(&sink));
        if let Some(manager) = &self.language_server_manager {
            manager.set_event_sink(sink);
        }
    }

    pub fn attach_formatter_manager(&mut self, manager: FormatterManager) {
        manager.set_tooling_capabilities(self.tooling_capabilities.clone());
        self.formatter_manager = Some(manager);
    }

    pub fn formatters(&self) -> Result<Vec<FormatterStatus>, FormatterError> {
        self.formatter_manager
            .as_ref()
            .map(FormatterManager::list)
            .ok_or(FormatterError::ManagerUnavailable)
    }

    pub fn formatter_status(&self, formatter_id: &str) -> Result<FormatterStatus, FormatterError> {
        self.formatter_manager
            .as_ref()
            .ok_or(FormatterError::ManagerUnavailable)?
            .status(formatter_id)
    }

    pub fn set_formatter_priorities(
        &self,
        formatter_ids: Vec<String>,
    ) -> Result<Vec<FormatterStatus>, FormatterError> {
        self.formatter_manager
            .as_ref()
            .ok_or(FormatterError::ManagerUnavailable)?
            .set_priorities(formatter_ids)
    }

    pub fn install_formatter(&self, formatter_id: &str) -> Result<FormatterStatus, FormatterError> {
        self.formatter_manager
            .as_ref()
            .ok_or(FormatterError::ManagerUnavailable)?
            .install(formatter_id)
    }

    pub fn uninstall_formatter(
        &self,
        formatter_id: &str,
    ) -> Result<FormatterStatus, FormatterError> {
        self.formatter_manager
            .as_ref()
            .ok_or(FormatterError::ManagerUnavailable)?
            .uninstall(formatter_id)
    }

    pub fn language_servers(&self) -> Result<Vec<LanguageServerStatus>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .map(LanguageServerManager::list)
            .ok_or(LanguageServerError::ManagerUnavailable)
    }

    pub fn resolved_tooling_capabilities(
        &self,
        documents: &[ResolvedToolingDocumentRequest],
    ) -> Result<ResolvedToolingSnapshot, LanguageServerError> {
        const MAX_DOCUMENTS: usize = 256;
        if documents.len() > MAX_DOCUMENTS {
            return Err(LanguageServerError::InvalidDocument(format!(
                "tooling capability snapshots support at most {MAX_DOCUMENTS} documents"
            )));
        }
        let documents = documents
            .iter()
            .map(|document| self.resolved_tooling_document(document))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ResolvedToolingSnapshot {
            revision: self.tooling_capabilities.revision(),
            documents,
        })
    }

    fn resolved_tooling_document(
        &self,
        request: &ResolvedToolingDocumentRequest,
    ) -> Result<ResolvedToolingDocument, LanguageServerError> {
        if self.workspaces.workspace(request.workspace_id).is_none() {
            return Err(LanguageServerError::InvalidDocument(
                "workspace does not exist".to_owned(),
            ));
        }
        let path = normalize_editor_path(&request.path)
            .map_err(|error| LanguageServerError::InvalidDocument(error.to_string()))?;
        let mut document = self.language_server_manager.as_ref().map_or_else(
            || ResolvedToolingDocument {
                workspace_id: request.workspace_id,
                path: path.clone(),
                language_id: request.language_id.clone(),
                supported: false,
                external_available: false,
                features: Vec::new(),
                formatter_id: None,
            },
            |manager| manager.resolved_document(request.workspace_id, &path, &request.language_id),
        );
        let formatter_id = self.formatter_manager.as_ref().and_then(|manager| {
            manager.applicable_formatter(&request.language_id, Path::new(&path))
        });
        if let Some(formatter_id) = formatter_id {
            document.supported = true;
            document
                .features
                .retain(|feature| feature.feature != LanguageToolFeature::Formatting);
            document.features.push(ResolvedToolingFeature {
                feature: LanguageToolFeature::Formatting,
                owners: vec![formatter_id.clone()],
            });
            document.formatter_id = Some(formatter_id);
        }
        Ok(document)
    }

    pub fn language_server_status(
        &self,
        server_id: &str,
    ) -> Result<LanguageServerStatus, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .status(server_id)
    }

    pub fn install_language_server(
        &self,
        server_id: &str,
    ) -> Result<LanguageServerStatus, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .install(server_id)
    }

    pub fn uninstall_language_server(
        &self,
        server_id: &str,
    ) -> Result<LanguageServerStatus, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .uninstall(server_id)
    }

    pub fn restart_language_server(
        &self,
        server_id: &str,
    ) -> Result<LanguageServerStatus, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .restart(server_id)
    }

    pub fn open_language_server_document(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        language_id: &str,
        generation: u64,
        version: i64,
        text: &str,
    ) -> Result<bool, LanguageServerError> {
        let location = self
            .editor_location(workspace_id, tab_id)
            .map_err(|error| LanguageServerError::InvalidDocument(error.to_string()))?;
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .open_document(LanguageServerDocumentOpen {
                workspace_id,
                workspace_root: location.workspace_root(),
                absolute_path: location.absolute_path(),
                relative_path: location.relative_path(),
                language_id,
                generation,
                version,
                text,
            })
    }

    pub fn change_language_server_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        changes: &[LanguageServerChange],
        text: &str,
    ) -> Result<(), LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .change_document(workspace_id, path, generation, version, changes, text)
    }

    pub fn close_language_server_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
    ) -> Result<(), LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .close_document(workspace_id, path, generation)
    }

    pub fn language_server_hover(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerHover>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .hover(
                workspace_id,
                path,
                generation,
                version,
                position,
                cancellation,
            )
    }

    pub fn language_server_signature_help(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerSignatureHelp>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .signature_help(
                workspace_id,
                path,
                generation,
                version,
                position,
                cancellation,
            )
    }

    pub fn language_server_definition(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .definition(
                workspace_id,
                path,
                generation,
                version,
                position,
                cancellation,
            )
    }

    pub fn language_server_declaration(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .declaration(
                workspace_id,
                path,
                generation,
                version,
                position,
                cancellation,
            )
    }

    pub fn language_server_type_definition(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .type_definition(
                workspace_id,
                path,
                generation,
                version,
                position,
                cancellation,
            )
    }

    pub fn language_server_implementation(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .implementation(
                workspace_id,
                path,
                generation,
                version,
                position,
                cancellation,
            )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn language_server_references(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        include_declaration: bool,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerLocation>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .references(
                workspace_id,
                path,
                generation,
                version,
                position,
                include_declaration,
                cancellation,
            )
    }

    pub fn language_server_document_symbols(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerDocumentSymbol>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .document_symbols(workspace_id, path, generation, version, cancellation)
    }

    pub fn language_server_workspace_symbols(
        &self,
        query: &str,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerWorkspaceSymbol>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .workspace_symbols(query, cancellation)
    }

    pub fn resolve_language_server_workspace_symbol(
        &self,
        request: LanguageServerWorkspaceSymbolResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerWorkspaceSymbol, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .resolve_workspace_symbol(request, cancellation)
    }

    pub fn language_server_diagnostics(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
    ) -> Result<Option<Vec<LanguageServerDiagnosticSnapshot>>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .diagnostics(workspace_id, path, generation, version)
    }

    pub fn language_server_completion(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerCompletionRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionList, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .completion(
                workspace_id,
                path,
                generation,
                version,
                request,
                cancellation,
            )
    }

    pub fn resolve_language_server_completion(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: LanguageServerCompletionResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCompletionItem, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .resolve_completion(
                workspace_id,
                path,
                generation,
                version,
                request,
                cancellation,
            )
    }

    pub fn language_server_document_colors(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorInformation>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .document_colors(workspace_id, path, generation, version, cancellation)
    }

    pub fn language_server_color_presentations(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerColorPresentationRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerColorPresentation>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .color_presentations(
                workspace_id,
                path,
                generation,
                version,
                request,
                cancellation,
            )
    }

    pub fn format_document(
        &self,
        request: DocumentFormattingRequest<'_>,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerTextEdit>, FormattingError> {
        if let Some(edits) = self.format_with_standalone_formatter(&request, cancellation)? {
            return Ok(edits);
        }
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .formatting(
                request.workspace_id,
                request.path,
                request.generation,
                request.version,
                request.options,
                cancellation,
            )
            .map_err(FormattingError::from)
    }

    pub(crate) fn format_editor_session_content(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        revision: u64,
        text: &str,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<String, FormattingError> {
        let request = DocumentFormattingRequest {
            workspace_id,
            path,
            // Standalone formatters also match by path. Editor sessions do not own Monaco's
            // language identifier, so that path is the durable input to the save policy.
            language_id: "",
            generation: revision,
            version: i64::try_from(revision).unwrap_or(i64::MAX),
            text,
            options: crate::language_servers::LanguageServerFormattingOptions {
                tab_size: 4,
                insert_spaces: true,
            },
        };
        let edits =
            if let Some(edits) = self.format_with_standalone_formatter(&request, cancellation)? {
                edits
            } else {
                self.language_server_manager
                    .as_ref()
                    .ok_or(LanguageServerError::ManagerUnavailable)?
                    .formatting_current_document(
                        workspace_id,
                        path,
                        text,
                        request.options,
                        cancellation,
                    )
                    .map_err(FormattingError::from)?
            };
        crate::language_servers::apply_document_text_edits(text, &edits).map_err(|error| {
            FormattingError::from(LanguageServerError::InvalidDocument(error.to_string()))
        })
    }

    pub(crate) fn notify_editor_session_saved(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        text: &str,
    ) -> Result<(), LanguageServerError> {
        match &self.language_server_manager {
            Some(manager) => manager.save_current_document(workspace_id, path, text),
            None => Ok(()),
        }
    }

    fn format_with_standalone_formatter(
        &self,
        request: &DocumentFormattingRequest<'_>,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<Vec<LanguageServerTextEdit>>, FormattingError> {
        if cancellation.is_cancelled() {
            return Err(LanguageServerError::RequestCancelled.into());
        }
        if request.options.tab_size == 0 {
            return Err(FormatterError::InvalidOptions(
                "tab size must be greater than zero".to_owned(),
            )
            .into());
        }
        if request.text.len() > crate::tabs::editor::MAX_EDITOR_FILE_BYTES {
            return Err(FormatterError::InvalidDocument(
                "document exceeds the editor size limit".to_owned(),
            )
            .into());
        }
        let workspace = self
            .workspaces
            .workspace(request.workspace_id)
            .ok_or_else(|| {
                FormatterError::InvalidDocument("workspace does not exist".to_owned())
            })?;
        let relative_path = normalize_editor_path(request.path)
            .map_err(|error| FormatterError::InvalidDocument(error.to_string()))?;
        let workspace_root =
            std::fs::canonicalize(workspace.directory()).map_err(FormatterError::io)?;
        let unresolved_path = workspace_root.join(&relative_path);
        if std::fs::symlink_metadata(&unresolved_path)
            .map_err(FormatterError::io)?
            .file_type()
            .is_symlink()
        {
            return Err(FormatterError::InvalidDocument(
                "formatter path must not be a symlink".to_owned(),
            )
            .into());
        }
        let absolute_path = std::fs::canonicalize(unresolved_path).map_err(FormatterError::io)?;
        if !absolute_path.starts_with(&workspace_root) {
            return Err(FormatterError::InvalidDocument(
                "formatter path is outside the workspace".to_owned(),
            )
            .into());
        }
        if let Some(manager) = &self.formatter_manager
            && let Some(formatted) = manager.format(
                request.language_id,
                Path::new(&relative_path),
                &workspace_root,
                &absolute_path,
                request.text,
            )?
        {
            if cancellation.is_cancelled() {
                return Err(LanguageServerError::RequestCancelled.into());
            }
            if formatted.len() > crate::tabs::editor::MAX_EDITOR_FILE_BYTES {
                return Err(FormatterError::OutputTooLarge.into());
            }
            return Ok(Some(full_document_edit(request.text, formatted)));
        }
        Ok(None)
    }

    pub fn language_server_prepare_rename(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Option<LanguageServerPrepareRename>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .prepare_rename(
                workspace_id,
                path,
                generation,
                version,
                position,
                cancellation,
            )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn language_server_rename(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        position: LanguageServerPosition,
        new_name: &str,
        server_id: Option<&str>,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<StagedWorkspaceEdit, LanguageServerError> {
        let roots = vec![self.workspace_edit_root(workspace_id)?];
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .rename(
                workspace_id,
                path,
                generation,
                version,
                position,
                new_name,
                server_id,
                &roots,
                cancellation,
            )
    }

    pub fn language_server_code_actions(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: &LanguageServerCodeActionRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<Vec<LanguageServerCodeAction>, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .code_actions(
                workspace_id,
                path,
                generation,
                version,
                request,
                cancellation,
            )
    }

    pub fn resolve_language_server_code_action(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        request: LanguageServerCodeActionResolveRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<LanguageServerCodeAction, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .resolve_code_action(
                workspace_id,
                path,
                generation,
                version,
                request,
                cancellation,
            )
    }

    pub fn stage_language_server_code_action(
        &self,
        action: &LanguageServerCodeAction,
    ) -> Result<Option<StagedWorkspaceEdit>, WorkspaceEditError> {
        let roots = self
            .workspace_edit_roots()
            .map_err(|error| WorkspaceEditError::Invalid(error.to_string()))?;
        self.language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .stage_code_action_edit(action, &roots)
    }

    pub fn execute_language_server_command(
        &self,
        request: LanguageServerExecuteCommandRequest,
        cancellation: &LanguageServerRequestCancellation,
    ) -> Result<serde_json::Value, LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .execute_command(request, cancellation)
    }

    pub fn trust_language_server_workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(), LanguageServerError> {
        let workspace = self.workspaces.workspace(workspace_id).ok_or_else(|| {
            LanguageServerError::InvalidDocument("workspace does not exist".to_owned())
        })?;
        let workspace_root = std::fs::canonicalize(workspace.directory())
            .map_err(|error| LanguageServerError::InvalidDocument(error.to_string()))?;
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .trust_workspace(&workspace_root)
    }
}

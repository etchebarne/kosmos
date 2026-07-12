use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
    StagedWorkspaceEdit, StagedWorkspaceEditOperation, WorkspaceEditError, WorkspaceEditRoot,
};
use crate::settings::{SettingValue, Settings, SettingsError};
use crate::tabs::editor::{
    EditorDocument, EditorError, EditorLocation, EditorViewState,
    normalize_path as normalize_editor_path, save_document,
};
use crate::tabs::file_tree::{
    FileTree, FileTreeDirectory, FileTreeEntryKind, FileTreeError, FileTreeViewState,
};
use crate::tabs::git::{
    GitDiff, GitDiffViewState, GitError, GitLineHunk, GitRemote, GitRepository,
    GitRepositorySnapshot, GitStash, GitTag,
};
use crate::tabs::search::{SearchError, SearchMode, WorkspaceSearch, WorkspaceSearchResults};
use crate::tabs::terminal::{
    TerminalError, TerminalOutput, TerminalSessions, TerminalSize, available_shells,
};
use crate::tree::{
    Pane, PaneId, PaneNode, SplitAxis, SplitPaneId, Tab, TabId, TabKind, Workspace, WorkspaceId,
    WorkspaceList,
};
use crate::window::WindowState;

#[derive(Debug)]
pub struct State {
    settings: Settings,
    window_state: Option<WindowState>,
    workspaces: WorkspaceList,
    file_tree_view_states: Vec<FileTreeViewState>,
    git_diff_view_states: Vec<GitDiffViewState>,
    editor_view_states: Vec<EditorViewState>,
    workspace_edit_editor_recovery: HashMap<u64, Vec<WorkspaceEditEditorRecovery>>,
    terminal_sessions: TerminalSessions,
    language_server_manager: Option<LanguageServerManager>,
    formatter_manager: Option<FormatterManager>,
    next_workspace_id: u64,
    next_pane_id: u64,
    next_split_id: u64,
    next_tab_id: u64,
    instance_id: u64,
    persistent_revision: u64,
    persistence_scope: PersistenceScope,
    tooling_capabilities: crate::events::ToolingCapabilities,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceEditEditorRecovery {
    original: EditorViewState,
    original_title: String,
    virtual_path: String,
    present: bool,
}

impl WorkspaceEditEditorRecovery {
    pub(crate) fn original(&self) -> &EditorViewState {
        &self.original
    }

    pub(crate) fn original_title(&self) -> &str {
        &self.original_title
    }

    pub(crate) fn applied_path(&self) -> Option<&str> {
        self.present.then_some(self.virtual_path.as_str())
    }
}

#[derive(Debug)]
pub(crate) struct PersistentStateCandidate {
    state: State,
    source_instance_id: u64,
    source_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PersistenceScope {
    Clean,
    ActiveWorkspace,
    Settings,
    Window,
    Full,
}

static NEXT_STATE_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

fn full_document_edit(text: &str, formatted: String) -> Vec<LanguageServerTextEdit> {
    if formatted == text {
        return Vec::new();
    }
    let line =
        u32::try_from(text.bytes().filter(|byte| *byte == b'\n').count()).unwrap_or(u32::MAX);
    let last_line = text.rsplit('\n').next().unwrap_or_default();
    let character = u32::try_from(last_line.encode_utf16().count()).unwrap_or(u32::MAX);
    vec![LanguageServerTextEdit {
        range: crate::language_servers::LanguageServerRange {
            start: LanguageServerPosition {
                line: 0,
                character: 0,
            },
            end: LanguageServerPosition { line, character },
        },
        new_text: formatted,
    }]
}

fn remap_workspace_path(path: &str, source: &str, destination: &str) -> Option<String> {
    if path == source {
        return Some(destination.to_owned());
    }
    let suffix = path.strip_prefix(source)?.strip_prefix('/')?;
    Some(format!("{destination}/{suffix}"))
}

fn path_is_at_or_below(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

impl PersistentStateCandidate {
    pub(crate) fn state(&self) -> &State {
        &self.state
    }

    pub(crate) fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    pub(crate) fn into_state(self) -> State {
        self.state
    }

    pub(crate) fn persistence_scope(&self) -> PersistenceScope {
        self.state.persistence_scope
    }
}

impl PersistenceScope {
    pub(crate) fn save(
        self,
        store: &crate::persistence::StateStore,
        state: &State,
    ) -> crate::persistence::Result<()> {
        match self {
            Self::Clean => store.save(state),
            Self::ActiveWorkspace => store.save_active_workspace(state),
            Self::Settings => store.save_settings(state),
            Self::Window => store.save_window_state(state),
            Self::Full => store.save(state),
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_workspaces(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
    ) -> Option<Self> {
        Self::from_workspaces_with_file_tree_view_states(
            workspaces,
            active_workspace_id,
            Vec::new(),
        )
    }

    pub fn from_workspaces_with_file_tree_view_states(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
        file_tree_view_states: Vec<FileTreeViewState>,
    ) -> Option<Self> {
        Self::from_workspaces_with_view_states(
            workspaces,
            active_workspace_id,
            file_tree_view_states,
            Vec::new(),
        )
    }

    pub fn from_workspaces_with_view_states(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
        file_tree_view_states: Vec<FileTreeViewState>,
        git_diff_view_states: Vec<GitDiffViewState>,
    ) -> Option<Self> {
        Self::from_workspaces_with_all_view_states(
            workspaces,
            active_workspace_id,
            file_tree_view_states,
            git_diff_view_states,
            Vec::new(),
        )
    }

    pub fn from_workspaces_with_all_view_states(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
        file_tree_view_states: Vec<FileTreeViewState>,
        git_diff_view_states: Vec<GitDiffViewState>,
        editor_view_states: Vec<EditorViewState>,
    ) -> Option<Self> {
        let mut workspace_list = WorkspaceList::new();

        for workspace in workspaces {
            if !workspace_list.add_workspace(workspace) {
                return None;
            }
        }

        match active_workspace_id {
            Some(active_workspace_id) if workspace_list.activate_workspace(active_workspace_id) => {
            }
            None if workspace_list.is_empty() => {}
            _ => return None,
        }

        Self::from_workspace_list(
            workspace_list,
            file_tree_view_states,
            git_diff_view_states,
            editor_view_states,
        )
    }

    pub fn workspaces(&self) -> &WorkspaceList {
        &self.workspaces
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn window_state(&self) -> Option<WindowState> {
        self.window_state
    }

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

    pub fn save_language_server_document(
        &self,
        workspace_id: WorkspaceId,
        path: &str,
        generation: u64,
        version: i64,
        text: &str,
    ) -> Result<(), LanguageServerError> {
        self.language_server_manager
            .as_ref()
            .ok_or(LanguageServerError::ManagerUnavailable)?
            .save_document(workspace_id, path, generation, version, text)
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
            return Ok(full_document_edit(request.text, formatted));
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

    pub fn commit_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        manager.commit_workspace_edit(transaction_id, authorization)?;
        self.reconcile_workspace_edit_resources(transaction_id, &operations, false);
        Ok(())
    }

    pub fn commit_workspace_edit_with_open_documents(
        &mut self,
        transaction_id: u64,
        authorization: &str,
        documents: &[crate::language_servers::WorkspaceEditOpenDocument],
    ) -> Result<(), WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        manager.commit_workspace_edit_with_documents(transaction_id, authorization, documents)?;
        self.reconcile_workspace_edit_resources(transaction_id, &operations, false);
        Ok(())
    }

    pub(crate) fn staged_workspace_edit(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<StagedWorkspaceEdit, WorkspaceEditError> {
        self.language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .staged_workspace_edit(transaction_id, authorization)
    }

    pub(crate) fn workspace_edit_model_directives(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<Vec<crate::language_servers::WorkspaceEditModelDirective>, WorkspaceEditError> {
        self.language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .workspace_edit_model_directives(transaction_id, authorization)
    }

    pub fn rollback_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        manager.rollback_workspace_edit(transaction_id, authorization)?;
        self.reconcile_workspace_edit_resources(transaction_id, &operations, true);
        Ok(())
    }

    fn reconcile_workspace_edit_resources(
        &mut self,
        transaction_id: u64,
        operations: &[StagedWorkspaceEditOperation],
        reverse: bool,
    ) {
        if reverse {
            if self.restore_workspace_edit_editor_recovery(transaction_id) {
                self.mark_persistent_change();
            }
            return;
        }
        let mut changed = false;
        for operation in operations {
            if let StagedWorkspaceEditOperation::CreateFile {
                workspace_id, path, ..
            }
            | StagedWorkspaceEditOperation::DeleteFile {
                workspace_id, path, ..
            } = operation
            {
                changed |= self.remove_deleted_editor_states(transaction_id, *workspace_id, path);
                continue;
            }
            let StagedWorkspaceEditOperation::RenameFile {
                workspace_id,
                old_path,
                new_path,
            } = operation
            else {
                continue;
            };
            changed |= self.remove_deleted_editor_states(transaction_id, *workspace_id, new_path);
            let renamed = self
                .editor_view_states
                .iter()
                .filter(|state| state.workspace_id() == *workspace_id)
                .filter_map(|state| {
                    remap_workspace_path(state.path(), old_path, new_path)
                        .map(|path| (state.clone(), path))
                })
                .collect::<Vec<_>>();
            for (state, path) in renamed {
                let title = self
                    .tab_title(state.workspace_id(), state.tab_id())
                    .unwrap_or_else(|| {
                        state
                            .path()
                            .rsplit('/')
                            .next()
                            .unwrap_or(state.path())
                            .to_owned()
                    });
                self.record_workspace_edit_editor_recovery(transaction_id, state.clone(), title);
                if let Some(current) = self.editor_view_states.iter_mut().find(|current| {
                    current.workspace_id() == state.workspace_id()
                        && current.tab_id() == state.tab_id()
                }) {
                    current.set_path(path.clone());
                }
                self.update_workspace_edit_editor_recovery(
                    transaction_id,
                    state.workspace_id(),
                    state.tab_id(),
                    path.clone(),
                    true,
                );
                self.set_editor_tab_state(
                    state.workspace_id(),
                    state.tab_id(),
                    TabKind::Editor,
                    path.rsplit('/').next().unwrap_or(&path),
                );
                changed = true;
            }
        }
        if !changed {
            return;
        }
        self.mark_persistent_change();
    }

    fn remove_deleted_editor_states(
        &mut self,
        transaction_id: u64,
        workspace_id: WorkspaceId,
        path: &str,
    ) -> bool {
        let removed = self
            .editor_view_states
            .iter()
            .filter(|state| {
                state.workspace_id() == workspace_id && path_is_at_or_below(state.path(), path)
            })
            .cloned()
            .collect::<Vec<_>>();
        if removed.is_empty() {
            return false;
        }
        for state in &removed {
            let title = self
                .tab_title(workspace_id, state.tab_id())
                .unwrap_or_else(|| {
                    state
                        .path()
                        .rsplit('/')
                        .next()
                        .unwrap_or(state.path())
                        .to_owned()
                });
            self.record_workspace_edit_editor_recovery(transaction_id, state.clone(), title);
            self.update_workspace_edit_editor_recovery(
                transaction_id,
                workspace_id,
                state.tab_id(),
                state.path().to_owned(),
                false,
            );
            self.set_editor_tab_state(workspace_id, state.tab_id(), TabKind::Blank, "Blank");
        }
        self.editor_view_states
            .retain(|state| !removed.contains(state));
        true
    }

    fn record_workspace_edit_editor_recovery(
        &mut self,
        transaction_id: u64,
        state: EditorViewState,
        original_title: String,
    ) {
        let states = self
            .workspace_edit_editor_recovery
            .entry(transaction_id)
            .or_default();
        if states.iter().any(|current| {
            current.original.workspace_id() == state.workspace_id()
                && current.original.tab_id() == state.tab_id()
        }) {
            return;
        }
        states.push(WorkspaceEditEditorRecovery {
            virtual_path: state.path().to_owned(),
            original: state,
            original_title,
            present: true,
        });
    }

    fn update_workspace_edit_editor_recovery(
        &mut self,
        transaction_id: u64,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        virtual_path: String,
        present: bool,
    ) {
        let Some(recovery) = self
            .workspace_edit_editor_recovery
            .get_mut(&transaction_id)
            .and_then(|states| {
                states.iter_mut().find(|state| {
                    state.original.workspace_id() == workspace_id
                        && state.original.tab_id() == tab_id
                })
            })
        else {
            return;
        };
        recovery.virtual_path = virtual_path;
        recovery.present = present;
    }

    fn restore_workspace_edit_editor_recovery(&mut self, transaction_id: u64) -> bool {
        let Some(recoveries) = self.workspace_edit_editor_recovery.remove(&transaction_id) else {
            return false;
        };
        for recovery in recoveries {
            let workspace_id = recovery.original.workspace_id();
            let tab_id = recovery.original.tab_id();
            self.editor_view_states
                .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
            self.editor_view_states.push(recovery.original);
            self.set_editor_tab_state(
                workspace_id,
                tab_id,
                TabKind::Editor,
                &recovery.original_title,
            );
        }
        true
    }

    fn tab_title(&self, workspace_id: WorkspaceId, tab_id: TabId) -> Option<String> {
        let workspace = self.workspaces.workspace(workspace_id)?;
        tab_title_in_node(workspace.root(), tab_id).map(str::to_owned)
    }

    fn set_editor_tab_state(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        kind: TabKind,
        title: &str,
    ) {
        let Some(pane_id) = tab_pane_id_in_workspace_list(&self.workspaces, workspace_id, tab_id)
        else {
            return;
        };
        if let Some(workspace) = self.workspace_mut(workspace_id)
            && let Some(pane) = workspace.root_mut().find_pane_mut(pane_id)
        {
            pane.set_tab_kind(tab_id, kind);
            pane.rename_tab(tab_id, title);
        }
    }

    pub fn finish_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        let finished = self
            .language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .finish_workspace_edit(transaction_id, authorization)?;
        if self
            .workspace_edit_editor_recovery
            .remove(&transaction_id)
            .is_some()
        {
            self.mark_persistent_change();
        }
        Ok(finished)
    }

    pub fn acknowledge_workspace_edit_completion(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        let acknowledged = self
            .language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .acknowledge_workspace_edit_completion(transaction_id, authorization)?;
        if self
            .workspace_edit_editor_recovery
            .remove(&transaction_id)
            .is_some()
        {
            self.mark_persistent_change();
        }
        Ok(acknowledged)
    }

    pub fn finalize_workspace_edit(
        &mut self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<crate::language_servers::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        let manager = self.language_server_manager.as_ref().ok_or_else(|| {
            WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
        })?;
        let operations = manager.workspace_edit_operations(transaction_id, authorization)?;
        let status = manager.finalize_workspace_edit(transaction_id, authorization)?;
        match status.phase {
            crate::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted => {
                self.reconcile_workspace_edit_resources(transaction_id, &operations, false);
                if self
                    .workspace_edit_editor_recovery
                    .remove(&transaction_id)
                    .is_some()
                {
                    self.mark_persistent_change();
                }
            }
            crate::language_servers::WorkspaceEditTransactionPhase::FinishedRolledBack => {
                self.reconcile_workspace_edit_resources(transaction_id, &operations, true);
            }
            _ => {}
        }
        Ok(status)
    }

    pub fn workspace_edit_status(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<crate::language_servers::WorkspaceEditTransactionStatus, WorkspaceEditError> {
        self.language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .workspace_edit_status(transaction_id, authorization)
    }

    pub fn workspace_edit_recoveries(
        &self,
    ) -> Result<Vec<crate::language_servers::WorkspaceEditRecovery>, WorkspaceEditError> {
        Ok(self
            .language_server_manager
            .as_ref()
            .ok_or_else(|| {
                WorkspaceEditError::Invalid("language server manager is unavailable".to_owned())
            })?
            .workspace_edit_recoveries())
    }

    fn workspace_edit_roots(&self) -> Result<Vec<WorkspaceEditRoot>, LanguageServerError> {
        self.workspaces
            .workspaces()
            .iter()
            .map(|workspace| {
                Ok(WorkspaceEditRoot {
                    workspace_id: workspace.id(),
                    path: std::fs::canonicalize(workspace.directory())
                        .map_err(|error| LanguageServerError::InvalidDocument(error.to_string()))?,
                })
            })
            .collect()
    }

    fn workspace_edit_root(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<WorkspaceEditRoot, LanguageServerError> {
        let workspace = self.workspaces.workspace(workspace_id).ok_or_else(|| {
            LanguageServerError::InvalidDocument("workspace does not exist".to_owned())
        })?;
        Ok(WorkspaceEditRoot {
            workspace_id,
            path: std::fs::canonicalize(workspace.directory())
                .map_err(|error| LanguageServerError::InvalidDocument(error.to_string()))?,
        })
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

    pub fn update_window_state(&mut self, window_state: WindowState) {
        if self.window_state != Some(window_state) {
            self.window_state = Some(window_state);
            self.mark_persistent_change_with_scope(PersistenceScope::Window);
        }
    }

    pub fn update_setting(&mut self, id: &str, value: SettingValue) -> Result<(), SettingsError> {
        if self.settings.update(id, value)? {
            self.mark_persistent_change_with_scope(PersistenceScope::Settings);
        }

        Ok(())
    }

    pub fn from_persisted(
        workspaces: Vec<Workspace>,
        active_workspace_id: Option<WorkspaceId>,
        file_tree_view_states: Vec<FileTreeViewState>,
        git_diff_view_states: Vec<GitDiffViewState>,
        editor_view_states: Vec<EditorViewState>,
        settings: Settings,
        window_state: Option<WindowState>,
    ) -> Option<Self> {
        let mut state = Self::from_workspaces_with_all_view_states(
            workspaces,
            active_workspace_id,
            file_tree_view_states,
            git_diff_view_states,
            editor_view_states,
        )?;
        state.settings = settings;
        state.window_state = window_state;
        Some(state)
    }

    pub fn file_tree_view_states(&self) -> &[FileTreeViewState] {
        &self.file_tree_view_states
    }

    pub fn git_diff_view_states(&self) -> &[GitDiffViewState] {
        &self.git_diff_view_states
    }

    pub fn editor_view_states(&self) -> &[EditorViewState] {
        &self.editor_view_states
    }

    pub(crate) fn workspace_edit_editor_recoveries(
        &self,
    ) -> impl Iterator<Item = (u64, &WorkspaceEditEditorRecovery)> {
        self.workspace_edit_editor_recovery
            .iter()
            .flat_map(|(transaction_id, states)| {
                states.iter().map(move |state| (*transaction_id, state))
            })
    }

    pub(crate) fn add_workspace_edit_editor_recovery(
        &mut self,
        transaction_id: u64,
        original: EditorViewState,
        original_title: String,
        applied_path: Option<String>,
    ) {
        self.workspace_edit_editor_recovery
            .entry(transaction_id)
            .or_default()
            .push(WorkspaceEditEditorRecovery {
                virtual_path: applied_path
                    .clone()
                    .unwrap_or_else(|| original.path().to_owned()),
                original,
                original_title,
                present: applied_path.is_some(),
            });
    }

    pub(crate) fn persistent_candidate(&self) -> PersistentStateCandidate {
        PersistentStateCandidate {
            state: Self {
                settings: self.settings.clone(),
                window_state: self.window_state,
                workspaces: self.workspaces.clone(),
                file_tree_view_states: self.file_tree_view_states.clone(),
                git_diff_view_states: self.git_diff_view_states.clone(),
                editor_view_states: self.editor_view_states.clone(),
                workspace_edit_editor_recovery: self.workspace_edit_editor_recovery.clone(),
                terminal_sessions: TerminalSessions::default(),
                language_server_manager: self.language_server_manager.clone(),
                formatter_manager: self.formatter_manager.clone(),
                next_workspace_id: self.next_workspace_id,
                next_pane_id: self.next_pane_id,
                next_split_id: self.next_split_id,
                next_tab_id: self.next_tab_id,
                instance_id: self.instance_id,
                persistent_revision: self.persistent_revision,
                persistence_scope: self.persistence_scope,
                tooling_capabilities: self.tooling_capabilities.clone(),
            },
            source_instance_id: self.instance_id,
            source_revision: self.persistent_revision,
        }
    }

    pub(crate) fn commit_persistent_candidate(
        &mut self,
        candidate: PersistentStateCandidate,
    ) -> bool {
        if !self.accepts_persistent_candidate(&candidate) {
            return false;
        }
        let Some(next_revision) = self.persistent_revision.checked_add(1) else {
            return false;
        };

        let candidate = candidate.state;

        self.terminal_sessions
            .retain(|workspace_id, tab_id| candidate.is_terminal_tab(workspace_id, tab_id));
        self.settings = candidate.settings;
        self.window_state = candidate.window_state;
        self.workspaces = candidate.workspaces;
        self.file_tree_view_states = candidate.file_tree_view_states;
        self.git_diff_view_states = candidate.git_diff_view_states;
        self.editor_view_states = candidate.editor_view_states;
        self.workspace_edit_editor_recovery = candidate.workspace_edit_editor_recovery;
        self.next_workspace_id = candidate.next_workspace_id;
        self.next_pane_id = candidate.next_pane_id;
        self.next_split_id = candidate.next_split_id;
        self.next_tab_id = candidate.next_tab_id;
        self.persistent_revision = next_revision;
        self.persistence_scope = PersistenceScope::Clean;

        if let Some(manager) = &self.language_server_manager {
            let workspace_ids = self
                .workspaces
                .workspaces()
                .iter()
                .map(Workspace::id)
                .collect::<HashSet<_>>();
            manager.retain_workspaces(&workspace_ids);
        }

        true
    }

    pub(crate) fn accepts_persistent_candidate(
        &self,
        candidate: &PersistentStateCandidate,
    ) -> bool {
        candidate.source_instance_id == self.instance_id
            && candidate.source_revision == self.persistent_revision
            && self.persistent_revision < u64::MAX
    }

    pub fn file_tree(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: Option<TabId>,
    ) -> Result<FileTree, FileTreeError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;
        let expanded_paths = match tab_id {
            Some(tab_id) if self.is_file_tree_tab(workspace_id, tab_id) => self
                .file_tree_view_state(workspace_id, tab_id)
                .map(FileTreeViewState::expanded_paths)
                .unwrap_or(&[]),
            Some(_) => return Err(FileTreeError::TabNotFound),
            None => &[],
        };

        FileTree::scan_with_expanded_paths(workspace.directory(), expanded_paths)
    }

    pub fn file_tree_root(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<&Path, FileTreeError> {
        self.file_tree_workspace_directory(workspace_id, tab_id)
    }

    pub fn file_tree_children(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        directory_path: &str,
    ) -> Result<FileTreeDirectory, FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;

        FileTree::scan_children(directory, directory_path)
    }

    pub fn set_file_tree_expanded_paths(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        expanded_paths: Vec<String>,
    ) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };

        if !self.is_file_tree_tab(workspace_id, tab_id) {
            return false;
        }

        let view_state = FileTreeViewState::new(workspace_id, tab_id, expanded_paths);
        self.remove_file_tree_view_state(workspace_id, tab_id);

        if !view_state.expanded_paths().is_empty() {
            self.file_tree_view_states.push(view_state);
        }

        true
    }

    pub fn create_file_tree_entry(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        parent_path: Option<&str>,
        name: &str,
        kind: FileTreeEntryKind,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::create_entry(directory, parent_path, name, kind).map(|_| ())
    }

    pub fn rename_file_tree_entry(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_path: &str,
        destination_path: &str,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::rename_entry(directory, source_path, destination_path).map(|_| ())
    }

    pub fn move_file_tree_entries(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::move_entries(directory, source_paths, target_directory_path).map(|_| ())
    }

    pub fn copy_file_tree_entries(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        source_paths: &[String],
        target_directory_path: Option<&str>,
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::copy_entries(directory, source_paths, target_directory_path).map(|_| ())
    }

    pub fn delete_file_tree_entries(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        paths: &[String],
    ) -> Result<(), FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::delete_entries(directory, paths)
    }

    pub fn resolve_file_tree_path(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        path: Option<&str>,
    ) -> Result<PathBuf, FileTreeError> {
        let directory = self.file_tree_workspace_directory(workspace_id, tab_id)?;
        FileTree::resolve_path(directory, path)
    }

    pub fn open_editor_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        source_tab_id: TabId,
        path: &str,
    ) -> Result<(), EditorError> {
        self.mark_persistent_change();

        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !self.is_editor_source_tab(workspace_id, source_tab_id) {
            return Err(EditorError::SourceTabNotFound);
        }

        let document = EditorDocument::read(workspace.directory(), path)?;
        let path = document.path().to_owned();

        if let Some((pane_id, tab_id)) = self.editor_tab(workspace_id, &path) {
            return if self.activate_tab(Some(workspace_id), pane_id, tab_id) {
                Ok(())
            } else {
                Err(EditorError::TabNotFound)
            };
        }

        let target_pane_id = workspace.root().largest_pane_id();
        let title = path
            .rsplit('/')
            .next()
            .expect("normalized editor paths have a file name")
            .to_owned();
        let tab = self.next_tab(TabKind::Editor, Some(title));
        let tab_id = tab.id();
        let view_state = EditorViewState::new(workspace_id, tab_id, path);
        let workspace = self
            .workspace_mut(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !workspace.add_tab_to_pane(target_pane_id, tab) {
            return Err(EditorError::TabNotFound);
        }

        workspace.activate_tab(target_pane_id, tab_id);
        self.editor_view_states.push(view_state);

        Ok(())
    }

    pub fn search_workspace(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        query: &str,
        mode: SearchMode,
    ) -> Result<WorkspaceSearchResults, SearchError> {
        let directory = self.search_workspace_directory(workspace_id, tab_id)?;

        WorkspaceSearch::query(directory, query, mode)
    }

    pub fn search_document(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        path: &str,
    ) -> Result<EditorDocument, SearchError> {
        let directory = self.search_workspace_directory(workspace_id, tab_id)?;

        WorkspaceSearch::document(directory, path)
    }

    pub fn editor_document(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<EditorDocument, EditorError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }

        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?;

        EditorDocument::read(workspace.directory(), view_state.path())
    }

    pub fn editor_session_target(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(WorkspaceId, String), EditorError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }
        let path = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?
            .path()
            .to_owned();
        Ok((workspace_id, path))
    }

    pub fn editor_location(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Result<EditorLocation, EditorError> {
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }
        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?;

        EditorLocation::resolve(workspace.directory(), view_state.path())
    }

    pub fn save_editor_document(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        content: &str,
    ) -> Result<(), EditorError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(EditorError::WorkspaceNotFound)?;

        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(EditorError::TabNotFound);
        }

        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(EditorError::TabNotFound)?;

        save_document(workspace.directory(), view_state.path(), content)
    }

    pub fn editor_git_line_hunks(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitLineHunk>, GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !self.is_editor_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        let view_state = self
            .editor_view_state(workspace_id, tab_id)
            .ok_or(GitError::TabNotFound)?;

        GitRepository::file_line_hunks(workspace.directory(), view_state.path())
    }

    pub fn git_status(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<GitRepositorySnapshot, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::snapshot(directory)
    }

    pub fn git_diff(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<GitDiff, GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let view_state = self
            .git_diff_view_state(workspace_id, tab_id)
            .ok_or(GitError::TabNotFound)?;

        if !self.is_git_diff_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        GitRepository::diff(workspace.directory(), view_state.path())
    }

    pub fn save_git_diff_file(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        path: &str,
        content: &str,
        stage: bool,
    ) -> Result<(), GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !self.is_git_diff_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        GitRepository::save_diff_file(workspace.directory(), path, content, stage)
    }

    pub fn open_git_diff_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        git_tab_id: TabId,
        path: &str,
    ) -> Result<(), GitError> {
        self.mark_persistent_change();

        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let path = GitRepository::normalize_path(path)?;

        self.git_workspace_directory(Some(workspace_id), git_tab_id)?;

        if let Some((pane_id, tab_id)) = self.git_diff_tab(workspace_id) {
            self.update_git_diff_view_state(workspace_id, tab_id, path);

            return if self.activate_tab(Some(workspace_id), pane_id, tab_id) {
                Ok(())
            } else {
                Err(GitError::TabNotFound)
            };
        }

        let target_pane_id = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?
            .root()
            .largest_pane_id();
        let tab = self.next_tab(TabKind::Diff, None);
        let tab_id = tab.id();
        let view_state = GitDiffViewState::new(workspace_id, tab_id, path);
        let workspace = self
            .workspace_mut(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !workspace.add_tab_to_pane(target_pane_id, tab) {
            return Err(GitError::TabNotFound);
        }

        workspace.activate_tab(target_pane_id, tab_id);
        self.git_diff_view_states.push(view_state);

        Ok(())
    }

    pub fn init_git_repository(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::init(directory)
    }

    pub fn stage_git_paths(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        paths: &[String],
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stage_paths(directory, paths)
    }

    pub fn unstage_git_paths(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        paths: &[String],
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::unstage_paths(directory, paths)
    }

    pub fn stage_all_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stage_all(directory)
    }

    pub fn unstage_all_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::unstage_all(directory)
    }

    pub fn commit_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        message: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::commit(directory, message)
    }

    pub fn switch_git_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        branch: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::switch_branch(directory, branch)
    }

    pub fn track_git_remote_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        branch: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::track_remote_branch(directory, branch)
    }

    pub fn create_git_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
        start_point: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::create_branch(directory, name, start_point)
    }

    pub fn delete_git_branch(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        branch: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::delete_branch(directory, branch)
    }

    pub fn fetch_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::fetch(directory)
    }

    pub fn pull_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        rebase: bool,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::pull(directory, rebase)
    }

    pub fn push_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        force: bool,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::push(directory, force)
    }

    pub fn stash_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stash(directory)
    }

    pub fn stash_staged_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stash_staged_changes(directory)
    }

    pub fn git_stashes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitStash>, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::stashes(directory)
    }

    pub fn apply_git_stash(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        selector: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::apply_stash(directory, selector)
    }

    pub fn drop_git_stash(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        selector: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::drop_stash(directory, selector)
    }

    pub fn git_remotes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitRemote>, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::remotes(directory)
    }

    pub fn add_git_remote(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
        url: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::add_remote(directory, name, url)
    }

    pub fn remove_git_remote(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::remove_remote(directory, name)
    }

    pub fn git_tags(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<Vec<GitTag>, GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::tags(directory)
    }

    pub fn create_git_tag(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::create_tag(directory, name)
    }

    pub fn delete_git_tag(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        name: &str,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::delete_tag(directory, name)
    }

    pub fn discard_all_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::discard_all_changes(directory)
    }

    pub fn discard_staged_git_changes(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<(), GitError> {
        let directory = self.git_workspace_directory(workspace_id, tab_id)?;

        GitRepository::discard_staged_changes(directory)
    }

    pub fn open_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(TerminalError::WorkspaceNotFound)?;
        let directory = self
            .terminal_workspace_directory(workspace_id, tab_id)?
            .to_path_buf();
        let size = TerminalSize::new(columns, rows)?;

        self.terminal_sessions
            .open(workspace_id, tab_id, &directory, size)
    }

    pub fn write_terminal_input(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        input: &str,
    ) -> Result<(), TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;

        self.terminal_sessions
            .write_input(workspace_id, tab_id, input)
    }

    pub fn read_terminal_output(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;

        self.terminal_sessions.read_output(workspace_id, tab_id)
    }

    pub fn resize_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
    ) -> Result<(), TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;
        let size = TerminalSize::new(columns, rows)?;

        self.terminal_sessions.resize(workspace_id, tab_id, size)
    }

    pub fn restart_terminal(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
        columns: u16,
        rows: u16,
        shell_path: &str,
    ) -> Result<TerminalOutput, TerminalError> {
        let workspace_id = self.terminal_workspace_id(workspace_id, tab_id)?;
        let directory = self
            .terminal_workspace_directory(workspace_id, tab_id)?
            .to_path_buf();
        let size = TerminalSize::new(columns, rows)?;
        let shell = available_shells()
            .into_iter()
            .find(|shell| shell.path() == shell_path)
            .ok_or_else(|| TerminalError::ShellNotAvailable(shell_path.to_owned()))?;

        self.terminal_sessions
            .restart(workspace_id, tab_id, &directory, size, &shell)
    }

    pub fn open_workspace(&mut self, directory: impl Into<PathBuf>) -> WorkspaceId {
        self.mark_persistent_change();

        let directory = directory.into();

        if let Some(workspace_id) = self
            .workspaces
            .workspaces()
            .iter()
            .find(|workspace| workspace.directory() == directory.as_path())
            .map(Workspace::id)
        {
            self.workspaces.activate_workspace(workspace_id);
            return workspace_id;
        }

        let workspace_id = self.next_workspace_id();
        let initial_pane = self.blank_pane();
        let workspace = Workspace::new(workspace_id, directory, initial_pane);

        self.workspaces.add_workspace(workspace);

        workspace_id
    }

    pub fn activate_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        self.mark_persistent_change_with_scope(PersistenceScope::ActiveWorkspace);
        self.workspaces.activate_workspace(workspace_id)
    }

    pub fn close_workspace(&mut self, workspace_id: Option<WorkspaceId>) -> Option<Workspace> {
        self.mark_persistent_change();

        let closed_workspace = match workspace_id {
            Some(workspace_id) => self.workspaces.close_workspace(workspace_id),
            None => self.workspaces.close_active_workspace(),
        };

        if let Some(workspace) = &closed_workspace {
            self.remove_workspace_file_tree_view_states(workspace.id());
            self.remove_workspace_git_diff_view_states(workspace.id());
            self.remove_workspace_editor_view_states(workspace.id());
            self.terminal_sessions.close_workspace(workspace.id());
        }

        closed_workspace
    }

    pub fn split_pane(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: Option<PaneId>,
        axis: SplitAxis,
        new_pane_first: bool,
    ) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let split_id = self.next_split_id();
        let new_pane = self.blank_pane();
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };
        let pane_id = pane_id.unwrap_or_else(|| workspace.active_pane_id());

        workspace.split_pane_with_new_pane_first(
            split_id,
            pane_id,
            axis,
            new_pane,
            0.5,
            new_pane_first,
        )
    }

    pub fn activate_pane(&mut self, workspace_id: Option<WorkspaceId>, pane_id: PaneId) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.activate_pane(pane_id)
    }

    pub fn move_pane(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        target_pane_id: PaneId,
        axis: SplitAxis,
        new_pane_first: bool,
    ) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let split_id = self.next_split_id();
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.move_pane_to_split(split_id, pane_id, target_pane_id, axis, 0.5, new_pane_first)
    }

    pub fn open_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: Option<PaneId>,
        title: Option<String>,
        kind: TabKind,
    ) -> bool {
        self.mark_persistent_change();

        if matches!(kind, TabKind::Diff | TabKind::Editor) {
            return false;
        }

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let tab = self.next_tab(kind, title);
        let tab_id = tab.id();
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };
        let pane_id = pane_id.unwrap_or_else(|| workspace.active_pane_id());

        if workspace.add_tab_to_pane(pane_id, tab) {
            workspace.activate_tab(pane_id, tab_id);
            true
        } else {
            false
        }
    }

    pub fn activate_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
    ) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.activate_tab(pane_id, tab_id)
    }

    pub fn set_tab_kind(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
        kind: TabKind,
    ) -> bool {
        self.mark_persistent_change();

        if matches!(kind, TabKind::Diff | TabKind::Editor) {
            return false;
        }

        let keep_file_tree_state = kind == TabKind::FileTree;
        let keep_terminal_session = kind == TabKind::Terminal;
        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let close_terminal_session = !keep_terminal_session
            && self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Terminal);
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        let updated = workspace.set_tab_kind(pane_id, tab_id, kind);

        if updated && !keep_file_tree_state {
            self.remove_file_tree_view_state(workspace_id, tab_id);
        }

        if updated {
            self.remove_git_diff_view_state(workspace_id, tab_id);
            self.remove_editor_view_state(workspace_id, tab_id);
        }

        if updated && close_terminal_session {
            self.terminal_sessions.close(workspace_id, tab_id);
        }

        updated
    }

    pub fn split_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        target_pane_id: PaneId,
        tab_id: TabId,
        axis: SplitAxis,
        new_pane_first: bool,
    ) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspaces.workspace(workspace_id) else {
            return false;
        };
        let Some(source_pane) = workspace.root().find_pane(pane_id) else {
            return false;
        };
        if !workspace.root().contains_pane(target_pane_id) {
            return false;
        }
        if !source_pane.contains_tab(tab_id) {
            return false;
        }

        let fallback_tab =
            (source_pane.tabs().len() == 1).then(|| self.next_tab(TabKind::Blank, None));
        let new_pane_id = self.next_pane_id();
        let split_id = self.next_split_id();

        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };
        let Some(source_pane) = workspace.root_mut().find_pane_mut(pane_id) else {
            return false;
        };
        let Some(tab) = source_pane.remove_tab(tab_id) else {
            return false;
        };

        if let Some(fallback_tab) = fallback_tab {
            source_pane.insert_tab(0, fallback_tab);
        }

        let new_pane = Pane::new(new_pane_id, tab);
        workspace.split_pane_with_new_pane_first(
            split_id,
            target_pane_id,
            axis,
            new_pane,
            0.5,
            new_pane_first,
        )
    }

    pub fn close_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        tab_id: TabId,
    ) -> Option<Tab> {
        self.mark_persistent_change();

        let workspace_id = self.resolve_workspace_id(workspace_id)?;
        let fallback_pane = self.blank_pane();
        let workspace = self.workspace_mut(workspace_id)?;

        let removed_tab = workspace.close_tab(pane_id, tab_id, fallback_pane);

        if removed_tab.is_some() {
            self.remove_file_tree_view_state(workspace_id, tab_id);
            self.remove_git_diff_view_state(workspace_id, tab_id);
            self.remove_editor_view_state(workspace_id, tab_id);
        }

        if removed_tab
            .as_ref()
            .is_some_and(|tab| tab.kind() == &TabKind::Terminal)
        {
            self.terminal_sessions.close(workspace_id, tab_id);
        }

        removed_tab
    }

    pub fn move_tab(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        pane_id: PaneId,
        target_pane_id: PaneId,
        tab_id: TabId,
        target_index: usize,
    ) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.move_tab_to_pane(pane_id, target_pane_id, tab_id, target_index)
    }

    pub fn resize_split(
        &mut self,
        workspace_id: Option<WorkspaceId>,
        split_id: SplitPaneId,
        ratio: f32,
    ) -> bool {
        self.mark_persistent_change();

        let Some(workspace_id) = self.resolve_workspace_id(workspace_id) else {
            return false;
        };
        let Some(workspace) = self.workspace_mut(workspace_id) else {
            return false;
        };

        workspace.resize_split(split_id, ratio)
    }

    fn workspace_mut(&mut self, workspace_id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.workspace_mut(workspace_id)
    }

    fn mark_persistent_change(&mut self) {
        self.mark_persistent_change_with_scope(PersistenceScope::Full);
    }

    fn mark_persistent_change_with_scope(&mut self, scope: PersistenceScope) {
        self.persistent_revision = self.persistent_revision.saturating_add(1);
        self.persistence_scope = match (self.persistence_scope, scope) {
            (PersistenceScope::Clean, scope) => scope,
            (current, scope) if current == scope => current,
            _ => PersistenceScope::Full,
        };
    }

    fn resolve_workspace_id(&self, workspace_id: Option<WorkspaceId>) -> Option<WorkspaceId> {
        workspace_id.or_else(|| self.workspaces.active_workspace_id())
    }

    fn next_workspace_id(&mut self) -> WorkspaceId {
        let workspace_id = WorkspaceId::new(self.next_workspace_id);
        self.next_workspace_id += 1;
        workspace_id
    }

    fn next_pane_id(&mut self) -> PaneId {
        let pane_id = PaneId::new(self.next_pane_id);
        self.next_pane_id += 1;
        pane_id
    }

    fn next_split_id(&mut self) -> SplitPaneId {
        let split_id = SplitPaneId::new(self.next_split_id);
        self.next_split_id += 1;
        split_id
    }

    fn next_tab_id(&mut self) -> TabId {
        let tab_id = TabId::new(self.next_tab_id);
        self.next_tab_id += 1;
        tab_id
    }

    fn blank_pane(&mut self) -> Pane {
        let pane_id = self.next_pane_id();
        let tab = self.next_tab(TabKind::Blank, None);

        Pane::new(pane_id, tab)
    }

    fn next_tab(&mut self, kind: TabKind, title: Option<String>) -> Tab {
        let title = title.unwrap_or_else(|| kind.default_title().to_owned());

        Tab::new(self.next_tab_id(), title, kind)
    }

    fn file_tree_view_state(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Option<&FileTreeViewState> {
        self.file_tree_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
    }

    fn remove_file_tree_view_state(&mut self, workspace_id: WorkspaceId, tab_id: TabId) {
        self.file_tree_view_states
            .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
    }

    fn remove_workspace_file_tree_view_states(&mut self, workspace_id: WorkspaceId) {
        self.file_tree_view_states
            .retain(|state| state.workspace_id() != workspace_id);
    }

    fn git_diff_view_state(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Option<&GitDiffViewState> {
        self.git_diff_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
    }

    fn git_diff_tab(&self, workspace_id: WorkspaceId) -> Option<(PaneId, TabId)> {
        self.git_diff_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id)
            .and_then(|state| {
                tab_pane_id_in_workspace_list(&self.workspaces, workspace_id, state.tab_id())
                    .map(|pane_id| (pane_id, state.tab_id()))
            })
    }

    fn update_git_diff_view_state(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        path: impl Into<String>,
    ) {
        if let Some(state) = self
            .git_diff_view_states
            .iter_mut()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
        {
            state.set_path(path);
        }
    }

    fn remove_git_diff_view_state(&mut self, workspace_id: WorkspaceId, tab_id: TabId) {
        self.git_diff_view_states
            .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
    }

    fn remove_workspace_git_diff_view_states(&mut self, workspace_id: WorkspaceId) {
        self.git_diff_view_states
            .retain(|state| state.workspace_id() != workspace_id);
    }

    fn editor_view_state(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Option<&EditorViewState> {
        self.editor_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
    }

    fn editor_tab(&self, workspace_id: WorkspaceId, path: &str) -> Option<(PaneId, TabId)> {
        self.editor_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id && state.path() == path)
            .and_then(|state| {
                tab_pane_id_in_workspace_list(&self.workspaces, workspace_id, state.tab_id())
                    .map(|pane_id| (pane_id, state.tab_id()))
            })
    }

    fn remove_editor_view_state(&mut self, workspace_id: WorkspaceId, tab_id: TabId) {
        self.editor_view_states
            .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
    }

    fn remove_workspace_editor_view_states(&mut self, workspace_id: WorkspaceId) {
        self.editor_view_states
            .retain(|state| state.workspace_id() != workspace_id);
    }

    fn is_file_tree_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::FileTree)
    }

    fn is_search_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Search)
    }

    fn is_editor_source_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.is_file_tree_tab(workspace_id, tab_id) || self.is_search_tab(workspace_id, tab_id)
    }

    fn is_terminal_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Terminal)
    }

    fn is_git_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Git)
    }

    fn is_git_diff_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Diff)
    }

    fn is_editor_tab(&self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        self.tab_kind(workspace_id, tab_id) == Some(&TabKind::Editor)
    }

    fn file_tree_workspace_directory(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<&Path, FileTreeError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(FileTreeError::WorkspaceNotFound)?;

        if !self.is_file_tree_tab(workspace_id, tab_id) {
            return Err(FileTreeError::TabNotFound);
        }

        Ok(workspace.directory())
    }

    fn terminal_workspace_id(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<WorkspaceId, TerminalError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(TerminalError::WorkspaceNotFound)?;

        self.terminal_workspace_directory(workspace_id, tab_id)?;

        Ok(workspace_id)
    }

    fn terminal_workspace_directory(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Result<&Path, TerminalError> {
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(TerminalError::WorkspaceNotFound)?;

        if !self.is_terminal_tab(workspace_id, tab_id) {
            return Err(TerminalError::TabNotFound);
        }

        Ok(workspace.directory())
    }

    fn git_workspace_directory(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<&Path, GitError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(GitError::WorkspaceNotFound)?;

        if !self.is_git_tab(workspace_id, tab_id) {
            return Err(GitError::TabNotFound);
        }

        Ok(workspace.directory())
    }

    fn search_workspace_directory(
        &self,
        workspace_id: Option<WorkspaceId>,
        tab_id: TabId,
    ) -> Result<&Path, SearchError> {
        let workspace_id = self
            .resolve_workspace_id(workspace_id)
            .ok_or(SearchError::WorkspaceNotFound)?;
        let workspace = self
            .workspaces
            .workspace(workspace_id)
            .ok_or(SearchError::WorkspaceNotFound)?;

        if !self.is_search_tab(workspace_id, tab_id) {
            return Err(SearchError::TabNotFound);
        }

        Ok(workspace.directory())
    }

    fn tab_kind(&self, workspace_id: WorkspaceId, tab_id: TabId) -> Option<&TabKind> {
        let workspace = self.workspaces.workspace(workspace_id)?;

        tab_kind_in_node(workspace.root(), tab_id)
    }

    fn from_workspace_list(
        workspaces: WorkspaceList,
        file_tree_view_states: Vec<FileTreeViewState>,
        git_diff_view_states: Vec<GitDiffViewState>,
        editor_view_states: Vec<EditorViewState>,
    ) -> Option<Self> {
        let next_ids = NextIds::from_workspaces(&workspaces)?;

        if file_tree_view_states.iter().any(|state| {
            tab_kind_in_workspace_list(&workspaces, state.workspace_id(), state.tab_id())
                != Some(&TabKind::FileTree)
        }) {
            return None;
        }

        if !git_diff_view_states_are_valid(&workspaces, &git_diff_view_states) {
            return None;
        }

        if !editor_view_states_are_valid(&workspaces, &editor_view_states) {
            return None;
        }

        Some(Self {
            settings: Settings::default(),
            window_state: None,
            workspaces,
            file_tree_view_states,
            git_diff_view_states,
            editor_view_states,
            workspace_edit_editor_recovery: HashMap::new(),
            terminal_sessions: TerminalSessions::default(),
            language_server_manager: None,
            formatter_manager: None,
            next_workspace_id: next_ids.workspace_id,
            next_pane_id: next_ids.pane_id,
            next_split_id: next_ids.split_id,
            next_tab_id: next_ids.tab_id,
            instance_id: next_state_instance_id(),
            persistent_revision: 0,
            persistence_scope: PersistenceScope::Clean,
            tooling_capabilities: crate::events::ToolingCapabilities::default(),
        })
    }
}

#[derive(Debug, Default)]
struct MaxIds {
    workspace_id: u64,
    pane_id: u64,
    split_id: u64,
    tab_id: u64,
}

impl MaxIds {
    fn visit_workspace(&mut self, workspace: &Workspace) {
        self.workspace_id = self.workspace_id.max(workspace.id().value());
        self.visit_pane_node(workspace.root());
    }

    fn visit_pane_node(&mut self, node: &crate::tree::PaneNode) {
        match node {
            crate::tree::PaneNode::Leaf(pane) => self.visit_pane(pane),
            crate::tree::PaneNode::Split(split) => {
                self.split_id = self.split_id.max(split.id().value());
                self.visit_pane_node(split.first());
                self.visit_pane_node(split.second());
            }
        }
    }

    fn visit_pane(&mut self, pane: &Pane) {
        self.pane_id = self.pane_id.max(pane.id().value());

        for tab in pane.tabs() {
            self.tab_id = self.tab_id.max(tab.id().value());
        }
    }
}

#[derive(Debug)]
struct NextIds {
    workspace_id: u64,
    pane_id: u64,
    split_id: u64,
    tab_id: u64,
}

impl NextIds {
    fn from_workspaces(workspaces: &WorkspaceList) -> Option<Self> {
        let mut max_ids = MaxIds::default();

        for workspace in workspaces.workspaces() {
            max_ids.visit_workspace(workspace);
        }

        Some(Self {
            workspace_id: next_id_after(max_ids.workspace_id)?,
            pane_id: next_id_after(max_ids.pane_id)?,
            split_id: next_id_after(max_ids.split_id)?,
            tab_id: next_id_after(max_ids.tab_id)?,
        })
    }
}

fn next_id_after(id: u64) -> Option<u64> {
    id.checked_add(1)
}

fn tab_kind_in_workspace_list(
    workspaces: &WorkspaceList,
    workspace_id: WorkspaceId,
    tab_id: TabId,
) -> Option<&TabKind> {
    let workspace = workspaces.workspace(workspace_id)?;

    tab_kind_in_node(workspace.root(), tab_id)
}

fn tab_pane_id_in_workspace_list(
    workspaces: &WorkspaceList,
    workspace_id: WorkspaceId,
    tab_id: TabId,
) -> Option<PaneId> {
    let workspace = workspaces.workspace(workspace_id)?;

    tab_pane_id_in_node(workspace.root(), tab_id)
}

fn tab_kind_in_node(node: &PaneNode, tab_id: TabId) -> Option<&TabKind> {
    match node {
        PaneNode::Leaf(pane) => pane
            .tabs()
            .iter()
            .find(|tab| tab.id() == tab_id)
            .map(Tab::kind),
        PaneNode::Split(split) => tab_kind_in_node(split.first(), tab_id)
            .or_else(|| tab_kind_in_node(split.second(), tab_id)),
    }
}

fn tab_title_in_node(node: &PaneNode, tab_id: TabId) -> Option<&str> {
    match node {
        PaneNode::Leaf(pane) => pane
            .tabs()
            .iter()
            .find(|tab| tab.id() == tab_id)
            .map(Tab::title),
        PaneNode::Split(split) => tab_title_in_node(split.first(), tab_id)
            .or_else(|| tab_title_in_node(split.second(), tab_id)),
    }
}

fn tab_pane_id_in_node(node: &PaneNode, tab_id: TabId) -> Option<PaneId> {
    match node {
        PaneNode::Leaf(pane) if pane.contains_tab(tab_id) => Some(pane.id()),
        PaneNode::Leaf(_) => None,
        PaneNode::Split(split) => tab_pane_id_in_node(split.first(), tab_id)
            .or_else(|| tab_pane_id_in_node(split.second(), tab_id)),
    }
}

fn git_diff_view_states_are_valid(
    workspaces: &WorkspaceList,
    view_states: &[GitDiffViewState],
) -> bool {
    let mut tabs_with_view_state = HashSet::new();
    let mut workspaces_with_view_state = HashSet::new();

    for state in view_states {
        let key = (state.workspace_id(), state.tab_id());

        if tab_kind_in_workspace_list(workspaces, key.0, key.1) != Some(&TabKind::Diff)
            || !tabs_with_view_state.insert(key)
            || !workspaces_with_view_state.insert(state.workspace_id())
        {
            return false;
        }
    }

    workspaces.workspaces().iter().all(|workspace| {
        diff_tabs_have_view_state(workspace.root(), workspace.id(), &tabs_with_view_state)
    })
}

fn diff_tabs_have_view_state(
    node: &PaneNode,
    workspace_id: WorkspaceId,
    tabs_with_view_state: &HashSet<(WorkspaceId, TabId)>,
) -> bool {
    match node {
        PaneNode::Leaf(pane) => pane.tabs().iter().all(|tab| {
            tab.kind() != &TabKind::Diff || tabs_with_view_state.contains(&(workspace_id, tab.id()))
        }),
        PaneNode::Split(split) => {
            diff_tabs_have_view_state(split.first(), workspace_id, tabs_with_view_state)
                && diff_tabs_have_view_state(split.second(), workspace_id, tabs_with_view_state)
        }
    }
}

fn editor_view_states_are_valid(
    workspaces: &WorkspaceList,
    view_states: &[EditorViewState],
) -> bool {
    let mut tabs_with_view_state = HashSet::new();
    let mut paths_with_view_state = HashSet::new();

    for state in view_states {
        let tab_key = (state.workspace_id(), state.tab_id());
        let path_key = (state.workspace_id(), state.path());

        if !normalize_editor_path(state.path()).is_ok_and(|path| path == state.path())
            || tab_kind_in_workspace_list(workspaces, tab_key.0, tab_key.1)
                != Some(&TabKind::Editor)
            || !tabs_with_view_state.insert(tab_key)
            || !paths_with_view_state.insert(path_key)
        {
            return false;
        }
    }

    workspaces.workspaces().iter().all(|workspace| {
        editor_tabs_have_view_state(workspace.root(), workspace.id(), &tabs_with_view_state)
    })
}

fn editor_tabs_have_view_state(
    node: &PaneNode,
    workspace_id: WorkspaceId,
    tabs_with_view_state: &HashSet<(WorkspaceId, TabId)>,
) -> bool {
    match node {
        PaneNode::Leaf(pane) => pane.tabs().iter().all(|tab| {
            tab.kind() != &TabKind::Editor
                || tabs_with_view_state.contains(&(workspace_id, tab.id()))
        }),
        PaneNode::Split(split) => {
            editor_tabs_have_view_state(split.first(), workspace_id, tabs_with_view_state)
                && editor_tabs_have_view_state(split.second(), workspace_id, tabs_with_view_state)
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            window_state: None,
            workspaces: WorkspaceList::new(),
            file_tree_view_states: Vec::new(),
            git_diff_view_states: Vec::new(),
            editor_view_states: Vec::new(),
            workspace_edit_editor_recovery: HashMap::new(),
            terminal_sessions: TerminalSessions::default(),
            language_server_manager: None,
            formatter_manager: None,
            next_workspace_id: 1,
            next_pane_id: 1,
            next_split_id: 1,
            next_tab_id: 1,
            instance_id: next_state_instance_id(),
            persistent_revision: 0,
            persistence_scope: PersistenceScope::Clean,
            tooling_capabilities: crate::events::ToolingCapabilities::default(),
        }
    }
}

fn next_state_instance_id() -> u64 {
    NEXT_STATE_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_document_rejects_zero_tab_size() {
        let state = State::new();
        let cancellation = LanguageServerRequestCancellation::new();

        let result = state.format_document(
            DocumentFormattingRequest {
                workspace_id: WorkspaceId::new(1),
                path: "missing.rs",
                language_id: "rust",
                generation: 1,
                version: 1,
                text: "",
                options: crate::language_servers::LanguageServerFormattingOptions {
                    tab_size: 0,
                    insert_spaces: true,
                },
            },
            &cancellation,
        );

        assert!(matches!(
            result,
            Err(FormattingError::Formatter(FormatterError::InvalidOptions(
                _
            )))
        ));
    }

    #[test]
    fn opening_workspace_creates_active_workspace() {
        let mut state = State::new();

        let workspace_id = state.open_workspace("/workspaces/main");

        assert_eq!(workspace_id, WorkspaceId::new(1));
        assert_eq!(state.workspaces().active_workspace_id(), Some(workspace_id));
        assert_eq!(state.workspaces().workspaces().len(), 1);
    }

    #[test]
    fn persistent_candidates_are_isolated_until_commit() {
        let mut state = State::new();
        state.open_workspace("/workspaces/first");
        let mut candidate = state.persistent_candidate();

        candidate.state_mut().open_workspace("/workspaces/second");

        assert_eq!(state.workspaces().workspaces().len(), 1);

        assert!(state.commit_persistent_candidate(candidate));

        assert_eq!(state.workspaces().workspaces().len(), 2);
        assert_eq!(
            state
                .workspaces()
                .active_workspace()
                .expect("workspace should be active")
                .directory(),
            Path::new("/workspaces/second")
        );
    }

    #[test]
    fn committing_candidates_preserves_valid_terminal_sessions() {
        let root = test_directory("persistent-terminal");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Terminal,
        ));
        state
            .open_terminal(Some(workspace_id), TabId::new(1), 80, 24)
            .expect("terminal should open");
        let mut candidate = state.persistent_candidate();
        assert!(candidate.state_mut().open_tab(
            Some(workspace_id),
            Some(PaneId::new(1)),
            None,
            TabKind::Search,
        ));

        assert!(state.commit_persistent_candidate(candidate));

        assert_eq!(state.terminal_sessions.len(), 1);
        assert!(
            state
                .read_terminal_output(Some(workspace_id), TabId::new(1))
                .is_ok()
        );

        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn committing_candidates_removes_invalid_terminal_sessions() {
        let root = test_directory("closed-persistent-terminal");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Terminal,
        ));
        state
            .open_terminal(Some(workspace_id), TabId::new(1), 80, 24)
            .expect("terminal should open");
        let mut candidate = state.persistent_candidate();
        assert!(candidate.state_mut().set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Search,
        ));

        assert!(state.commit_persistent_candidate(candidate));

        assert_eq!(state.terminal_sessions.len(), 0);

        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stale_and_cross_state_candidates_are_rejected() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        let first_candidate = state.persistent_candidate();
        let stale_candidate = state.persistent_candidate();
        let other_candidate = State::new().persistent_candidate();

        assert!(state.commit_persistent_candidate(first_candidate));
        assert!(!state.commit_persistent_candidate(stale_candidate));
        assert!(!state.commit_persistent_candidate(other_candidate));

        let candidate_before_direct_mutation = state.persistent_candidate();
        state.open_workspace("/workspaces/direct");

        assert!(!state.commit_persistent_candidate(candidate_before_direct_mutation));
        assert_eq!(state.workspaces().workspaces().len(), 2);
    }

    #[test]
    fn opening_existing_workspace_path_activates_existing_workspace() {
        let mut state = State::new();

        let first_workspace_id = state.open_workspace("/workspaces/first");
        let second_workspace_id = state.open_workspace("/workspaces/second");
        let reopened_workspace_id = state.open_workspace("/workspaces/first");

        assert_eq!(second_workspace_id, WorkspaceId::new(2));
        assert_eq!(reopened_workspace_id, first_workspace_id);
        assert_eq!(
            state.workspaces().active_workspace_id(),
            Some(first_workspace_id)
        );
        assert_eq!(state.workspaces().workspaces().len(), 2);
        assert_eq!(
            state.open_workspace("/workspaces/third"),
            WorkspaceId::new(3)
        );
    }

    #[test]
    fn opening_tab_adds_it_to_active_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(state.open_tab(None, None, None, TabKind::Search));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let pane = workspace
            .active_pane()
            .expect("workspace should have an active pane");

        assert_eq!(pane.tabs().len(), 2);
        assert_eq!(pane.active_tab().title(), "Search");
    }

    #[test]
    fn generic_tab_operations_cannot_create_specialized_tabs() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(!state.open_tab(None, None, None, TabKind::Diff));
        assert!(!state.open_tab(None, None, None, TabKind::Editor));
        assert!(!state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Diff,));
        assert!(!state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Editor,));

        let active_tab = state
            .workspaces()
            .active_workspace()
            .expect("workspace should exist")
            .active_pane()
            .expect("pane should exist")
            .active_tab();

        assert_eq!(active_tab.kind(), &TabKind::Blank);
        assert!(state.git_diff_view_states().is_empty());
        assert!(state.editor_view_states().is_empty());
    }

    #[test]
    fn restoring_diff_tabs_requires_exactly_one_view_state() {
        let workspace_id = WorkspaceId::new(1);
        let tab_id = TabId::new(1);
        let workspace = Workspace::new(
            workspace_id,
            "/workspaces/main",
            Pane::new(PaneId::new(1), Tab::new(tab_id, "Diff", TabKind::Diff)),
        );

        assert!(State::from_workspaces(vec![workspace.clone()], Some(workspace_id)).is_none());
        assert!(
            State::from_workspaces_with_view_states(
                vec![workspace],
                Some(workspace_id),
                Vec::new(),
                vec![
                    GitDiffViewState::new(workspace_id, tab_id, "README.md"),
                    GitDiffViewState::new(workspace_id, tab_id, "README.md"),
                ],
            )
            .is_none()
        );
    }

    #[test]
    fn restoring_editor_tabs_requires_unique_normalized_view_state() {
        let workspace_id = WorkspaceId::new(1);
        let first_tab_id = TabId::new(1);
        let second_tab_id = TabId::new(2);
        let mut pane = Pane::new(
            PaneId::new(1),
            Tab::new(first_tab_id, "main.rs", TabKind::Editor),
        );
        pane.add_tab(Tab::new(second_tab_id, "lib.rs", TabKind::Editor));
        let workspace = Workspace::new(workspace_id, "/workspaces/main", pane);

        assert!(State::from_workspaces(vec![workspace.clone()], Some(workspace_id)).is_none());
        assert!(
            State::from_workspaces_with_all_view_states(
                vec![workspace.clone()],
                Some(workspace_id),
                Vec::new(),
                Vec::new(),
                vec![
                    EditorViewState::new(workspace_id, first_tab_id, "src/main.rs"),
                    EditorViewState::new(workspace_id, second_tab_id, "src/main.rs"),
                ],
            )
            .is_none()
        );
        assert!(
            State::from_workspaces_with_all_view_states(
                vec![workspace],
                Some(workspace_id),
                Vec::new(),
                Vec::new(),
                vec![
                    EditorViewState::new(workspace_id, first_tab_id, "src/../main.rs"),
                    EditorViewState::new(workspace_id, second_tab_id, "src/lib.rs"),
                ],
            )
            .is_none()
        );
    }

    #[test]
    fn setting_tab_kind_updates_kind_and_default_title() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Git));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let pane = workspace
            .active_pane()
            .expect("workspace should have an active pane");

        assert_eq!(pane.active_tab().title(), "Git");
        assert_eq!(pane.active_tab().kind(), &TabKind::Git);
    }

    #[test]
    fn opening_git_diff_tab_places_it_in_largest_pane() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Git,
        ));
        assert!(state.split_pane(
            Some(workspace_id),
            Some(PaneId::new(1)),
            SplitAxis::Horizontal,
            false,
        ));
        assert!(state.resize_split(Some(workspace_id), SplitPaneId::new(1), 0.7));

        state
            .open_git_diff_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .expect("diff tab should open");

        let workspace = state
            .workspaces()
            .workspace(workspace_id)
            .expect("workspace should exist");
        let largest_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("largest pane should exist");

        assert_eq!(workspace.active_pane_id(), PaneId::new(1));
        assert_eq!(largest_pane.active_tab_id(), TabId::new(3));
        assert_eq!(largest_pane.active_tab().kind(), &TabKind::Diff);
        assert_eq!(state.git_diff_view_states()[0].path(), "src/main.rs");
    }

    #[test]
    fn opening_existing_git_diff_tab_reuses_it_and_updates_focus_path() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Git,
        ));

        state
            .open_git_diff_tab(Some(workspace_id), TabId::new(1), "README.md")
            .expect("diff tab should open");
        state
            .open_git_diff_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .expect("existing diff tab should activate");

        let workspace = state
            .workspaces()
            .workspace(workspace_id)
            .expect("workspace should exist");
        let pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("pane should exist");

        assert_eq!(pane.tabs().len(), 2);
        assert_eq!(state.git_diff_view_states().len(), 1);
        assert_eq!(state.git_diff_view_states()[0].path(), "src/main.rs");
        assert_eq!(pane.active_tab_id(), TabId::new(2));
        assert_eq!(pane.active_tab().title(), "Diff");
    }

    #[test]
    fn editor_tabs_use_the_largest_pane_and_reuse_only_the_same_path() {
        let root = test_directory("editor-tabs");
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn library() {}").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        assert!(state.split_pane(
            Some(workspace_id),
            Some(PaneId::new(1)),
            SplitAxis::Horizontal,
            false,
        ));
        assert!(state.resize_split(Some(workspace_id), SplitPaneId::new(1), 0.7));

        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .unwrap();
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/lib.rs")
            .unwrap();
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .unwrap();

        let workspace = state.workspaces().workspace(workspace_id).unwrap();
        let largest_pane = workspace.root().find_pane(PaneId::new(1)).unwrap();
        let smaller_pane = workspace.root().find_pane(PaneId::new(2)).unwrap();

        assert_eq!(workspace.active_pane_id(), PaneId::new(1));
        assert_eq!(largest_pane.active_tab_id(), TabId::new(3));
        assert_eq!(largest_pane.active_tab().title(), "main.rs");
        assert_eq!(largest_pane.tabs().len(), 3);
        assert_eq!(smaller_pane.tabs().len(), 1);
        assert_eq!(state.editor_view_states().len(), 2);
        assert_eq!(state.editor_view_states()[0].path(), "src/main.rs");
        assert_eq!(state.editor_view_states()[1].path(), "src/lib.rs");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_resource_rename_updates_and_rolls_back_editor_paths_and_titles() {
        let root = test_directory("editor-resource-rename");
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .unwrap();
        let operations = vec![StagedWorkspaceEditOperation::RenameFile {
            workspace_id,
            old_path: "src/main.rs".to_owned(),
            new_path: "src/renamed.rs".to_owned(),
        }];

        state.reconcile_workspace_edit_resources(1, &operations, false);
        assert_eq!(state.editor_view_states()[0].path(), "src/renamed.rs");
        assert_eq!(
            state
                .workspaces()
                .workspace(workspace_id)
                .unwrap()
                .root()
                .find_pane(PaneId::new(1))
                .unwrap()
                .active_tab()
                .title(),
            "renamed.rs"
        );
        state.reconcile_workspace_edit_resources(1, &operations, true);
        assert_eq!(state.editor_view_states()[0].path(), "src/main.rs");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overwrite_rename_removes_destination_tab_and_restores_both_tabs_on_rollback() {
        let root = test_directory("editor-overwrite-rename");
        std::fs::write(root.join("source.rs"), "source").unwrap();
        std::fs::write(root.join("destination.rs"), "destination").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "source.rs")
            .unwrap();
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "destination.rs")
            .unwrap();
        assert_eq!(state.editor_view_states().len(), 2);
        let operations = vec![StagedWorkspaceEditOperation::RenameFile {
            workspace_id,
            old_path: "source.rs".to_owned(),
            new_path: "destination.rs".to_owned(),
        }];

        state.reconcile_workspace_edit_resources(11, &operations, false);
        assert_eq!(state.editor_view_states().len(), 1);
        assert_eq!(state.editor_view_states()[0].path(), "destination.rs");

        state.reconcile_workspace_edit_resources(11, &operations, true);
        let mut paths = state
            .editor_view_states()
            .iter()
            .map(|state| state.path())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        assert_eq!(paths, ["destination.rs", "source.rs"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overwrite_create_removes_old_tab_lineage_and_persists_replacement_semantics() {
        let root = test_directory("editor-overwrite-create-lineage");
        let database_root = test_directory("editor-overwrite-create-lineage-store");
        std::fs::write(root.join("a.rs"), "old content").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "a.rs")
            .unwrap();
        let operations = vec![
            StagedWorkspaceEditOperation::CreateFile {
                workspace_id,
                path: "a.rs".to_owned(),
            },
            StagedWorkspaceEditOperation::RenameFile {
                workspace_id,
                old_path: "a.rs".to_owned(),
                new_path: "b.rs".to_owned(),
            },
            StagedWorkspaceEditOperation::TextDocument { document: 0 },
        ];

        state.reconcile_workspace_edit_resources(15, &operations, false);
        assert!(state.editor_view_states().is_empty());
        assert_eq!(
            state
                .workspaces()
                .workspace(workspace_id)
                .unwrap()
                .root()
                .find_pane(PaneId::new(1))
                .unwrap()
                .tabs()
                .iter()
                .find(|tab| tab.id() == TabId::new(2))
                .unwrap()
                .kind(),
            &TabKind::Blank
        );

        let store =
            crate::persistence::StateStore::open(database_root.join("state.sqlite3")).unwrap();
        store.save(&state).unwrap();
        assert!(store.load().unwrap().editor_view_states().is_empty());

        state.reconcile_workspace_edit_resources(15, &operations, true);
        assert_eq!(state.editor_view_states().len(), 1);
        assert_eq!(state.editor_view_states()[0].path(), "a.rs");
        assert_eq!(
            state
                .workspaces()
                .workspace(workspace_id)
                .unwrap()
                .root()
                .find_pane(PaneId::new(1))
                .unwrap()
                .tabs()
                .iter()
                .find(|tab| tab.id() == TabId::new(2))
                .unwrap()
                .kind(),
            &TabKind::Editor
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(database_root);
    }

    #[test]
    fn persisted_overwrite_rename_restores_tabs_independent_of_tab_id_order() {
        let root = test_directory("editor-overwrite-rename-persistence");
        let database_root = test_directory("editor-overwrite-rename-persistence-store");
        let database = database_root.join("state.sqlite3");
        std::fs::write(root.join("source.rs"), "source").unwrap();
        std::fs::write(root.join("destination.rs"), "destination").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "destination.rs")
            .unwrap();
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "source.rs")
            .unwrap();
        let operations = vec![StagedWorkspaceEditOperation::RenameFile {
            workspace_id,
            old_path: "source.rs".to_owned(),
            new_path: "destination.rs".to_owned(),
        }];
        state.reconcile_workspace_edit_resources(14, &operations, false);
        let store = crate::persistence::StateStore::open(&database).unwrap();
        store.save(&state).unwrap();

        store.restore_workspace_edit_editor_recovery(14).unwrap();
        let restored = store.load().unwrap();

        let paths = restored
            .editor_view_states()
            .iter()
            .map(|state| (state.tab_id(), state.path()))
            .collect::<HashMap<_, _>>();
        assert_eq!(paths[&TabId::new(2)], "destination.rs");
        assert_eq!(paths[&TabId::new(3)], "source.rs");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(database_root);
    }

    #[test]
    fn file_resource_chain_restores_original_tab_identity_view_and_title() {
        let root = test_directory("editor-file-resource-chain");
        std::fs::write(root.join("first.rs"), "first").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "first.rs")
            .unwrap();
        state
            .workspace_mut(workspace_id)
            .unwrap()
            .root_mut()
            .find_pane_mut(PaneId::new(1))
            .unwrap()
            .rename_tab(TabId::new(2), "Pinned source");
        let operations = vec![
            StagedWorkspaceEditOperation::RenameFile {
                workspace_id,
                old_path: "first.rs".to_owned(),
                new_path: "second.rs".to_owned(),
            },
            StagedWorkspaceEditOperation::RenameFile {
                workspace_id,
                old_path: "second.rs".to_owned(),
                new_path: "third.rs".to_owned(),
            },
            StagedWorkspaceEditOperation::DeleteFile {
                workspace_id,
                path: "third.rs".to_owned(),
                recursive: false,
            },
        ];

        state.reconcile_workspace_edit_resources(12, &operations, false);
        assert!(state.editor_view_states().is_empty());
        state.reconcile_workspace_edit_resources(12, &operations, true);

        assert_eq!(state.editor_view_states().len(), 1);
        assert_eq!(state.editor_view_states()[0].tab_id(), TabId::new(2));
        assert_eq!(state.editor_view_states()[0].path(), "first.rs");
        let tab = state
            .workspaces()
            .workspace(workspace_id)
            .unwrap()
            .root()
            .find_pane(PaneId::new(1))
            .unwrap()
            .tabs()
            .iter()
            .find(|tab| tab.id() == TabId::new(2))
            .unwrap();
        assert_eq!(tab.kind(), &TabKind::Editor);
        assert_eq!(tab.title(), "Pinned source");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn directory_resource_chain_persists_exact_editor_recovery() {
        let root = test_directory("editor-directory-resource-chain");
        let database_root = test_directory("editor-directory-resource-chain-store");
        let database = database_root.join("state.sqlite3");
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::write(root.join("src/nested/main.rs"), "main").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/nested/main.rs")
            .unwrap();
        state
            .workspace_mut(workspace_id)
            .unwrap()
            .root_mut()
            .find_pane_mut(PaneId::new(1))
            .unwrap()
            .rename_tab(TabId::new(2), "Pinned nested file");
        let operations = vec![
            StagedWorkspaceEditOperation::RenameFile {
                workspace_id,
                old_path: "src".to_owned(),
                new_path: "moved".to_owned(),
            },
            StagedWorkspaceEditOperation::DeleteFile {
                workspace_id,
                path: "moved".to_owned(),
                recursive: true,
            },
        ];
        state.reconcile_workspace_edit_resources(13, &operations, false);
        let store = crate::persistence::StateStore::open(&database).unwrap();
        store.save(&state).unwrap();

        store.restore_workspace_edit_editor_recovery(13).unwrap();
        let restored = store.load().unwrap();

        assert_eq!(restored.editor_view_states().len(), 1);
        assert_eq!(restored.editor_view_states()[0].tab_id(), TabId::new(2));
        assert_eq!(
            restored.editor_view_states()[0].path(),
            "src/nested/main.rs"
        );
        let tab = restored
            .workspaces()
            .workspace(workspace_id)
            .unwrap()
            .root()
            .find_pane(PaneId::new(1))
            .unwrap()
            .tabs()
            .iter()
            .find(|tab| tab.id() == TabId::new(2))
            .unwrap();
        assert_eq!(tab.kind(), &TabKind::Editor);
        assert_eq!(tab.title(), "Pinned nested file");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(database_root);
    }

    #[test]
    fn startup_recovers_persisted_editor_directory_rename_before_state_load() {
        let root = test_directory("editor-resource-rename-startup");
        let database_root = test_directory("editor-resource-rename-database");
        let database = database_root.join("state.sqlite3");
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::write(root.join("src/nested/main.rs"), "fn main() {}").unwrap();
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/nested/main.rs")
            .unwrap();
        store.save(&state).unwrap();

        let paths = crate::language_servers::LanguageServerPaths::new(
            database_root.join("language-servers"),
            database_root.join("language-server-cache"),
        );
        let manager =
            crate::language_servers::LanguageServerManager::open(paths, store.clone()).unwrap();
        let staged = manager
            .stage_workspace_edit(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "rename",
                    "oldUri": format!("file://{}", root.join("src").display()),
                    "newUri": format!("file://{}", root.join("renamed").display())
                }]}),
                &state.workspace_edit_roots().unwrap(),
            )
            .unwrap();
        state.attach_language_server_manager(manager);
        state
            .commit_workspace_edit(staged.transaction_id, &staged.authorization)
            .unwrap();
        store.save(&state).unwrap();
        assert_eq!(
            state.editor_view_states()[0].path(),
            "renamed/nested/main.rs"
        );
        drop(state);

        let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
        let reopened_paths = crate::language_servers::LanguageServerPaths::new(
            database_root.join("language-servers"),
            database_root.join("language-server-cache"),
        );
        let restarted_manager = crate::language_servers::LanguageServerManager::open(
            reopened_paths,
            reopened_store.clone(),
        )
        .unwrap();
        assert!(matches!(
            restarted_manager.workspace_edit_status(staged.transaction_id, &staged.authorization),
            Err(crate::language_servers::WorkspaceEditError::Invalid(_))
        ));
        let recovery = restarted_manager
            .workspace_edit_recoveries()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            recovery.status.phase,
            crate::language_servers::WorkspaceEditTransactionPhase::FinishedRolledBack
        );
        let recovered = reopened_store.load().unwrap();
        assert_eq!(
            recovered.editor_view_states()[0].path(),
            "src/nested/main.rs"
        );
        assert_eq!(
            recovered
                .workspaces()
                .workspace(workspace_id)
                .unwrap()
                .root()
                .find_pane(PaneId::new(1))
                .unwrap()
                .active_tab()
                .title(),
            "main.rs"
        );
        assert!(root.join("src/nested/main.rs").is_file());
        assert!(!root.join("renamed").exists());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(database_root);
    }

    #[test]
    fn finished_workspace_edit_survives_unrelated_full_save_without_stale_editor_recovery() {
        let root = test_directory("finished-editor-recovery-workspace");
        let database_root = test_directory("finished-editor-recovery-database");
        let database = database_root.join("state.sqlite3");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let paths = crate::language_servers::LanguageServerPaths::new(
            database_root.join("language-servers"),
            database_root.join("language-server-cache"),
        );
        let manager =
            crate::language_servers::LanguageServerManager::open(paths.clone(), store.clone())
                .unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
            .unwrap();
        let staged = manager
            .stage_workspace_edit(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "rename",
                    "oldUri": format!("file://{}", root.join("src").display()),
                    "newUri": format!("file://{}", root.join("renamed").display())
                }]}),
                &state.workspace_edit_roots().unwrap(),
            )
            .unwrap();
        state.attach_language_server_manager(manager);
        state
            .commit_workspace_edit(staged.transaction_id, &staged.authorization)
            .unwrap();
        store.save(&state).unwrap();
        state
            .finish_workspace_edit(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(state.editor_view_states()[0].path(), "renamed/main.rs");

        assert!(state.activate_workspace(workspace_id));
        store.save(&state).unwrap();
        drop(state);

        let restarted_manager =
            crate::language_servers::LanguageServerManager::open(paths, store.clone()).unwrap();
        let restarted = store.load().unwrap();
        assert_eq!(restarted.editor_view_states()[0].path(), "renamed/main.rs");
        assert_eq!(restarted.workspace_edit_editor_recoveries().count(), 0);
        assert!(root.join("renamed/main.rs").is_file());
        assert!(!root.join("src").exists());
        let recovery = restarted_manager
            .workspace_edit_recoveries()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            recovery.status.phase,
            crate::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(database_root);
    }

    #[test]
    fn workspace_resource_delete_persists_and_restores_editor_tabs() {
        let root = test_directory("editor-resource-delete");
        std::fs::write(root.join("open.rs"), "fn main() {}").unwrap();
        let database = root.join("state.sqlite3");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "open.rs")
            .unwrap();
        let operations = vec![StagedWorkspaceEditOperation::DeleteFile {
            workspace_id,
            path: "open.rs".to_owned(),
            recursive: false,
        }];
        state.reconcile_workspace_edit_resources(9, &operations, false);
        assert!(state.editor_view_states().is_empty());
        let store = crate::persistence::StateStore::open(&database).unwrap();
        store.save(&state).unwrap();
        store.restore_workspace_edit_editor_recovery(9).unwrap();
        let restored = store.load().unwrap();
        assert_eq!(restored.editor_view_states()[0].path(), "open.rs");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn opening_editor_tabs_requires_a_supported_source_and_existing_file() {
        let root = test_directory("editor-open-validation");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);

        assert!(matches!(
            state.open_editor_tab(Some(workspace_id), TabId::new(1), "missing.txt"),
            Err(EditorError::SourceTabNotFound)
        ));
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        assert!(matches!(
            state.open_editor_tab(Some(workspace_id), TabId::new(1), "missing.txt"),
            Err(EditorError::FileNotFound(_))
        ));
        assert!(state.editor_view_states().is_empty());

        let workspace = state.workspaces().workspace(workspace_id).unwrap();
        assert_eq!(workspace.active_pane().unwrap().tabs().len(), 1);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn search_tabs_query_preview_and_open_editor_results() {
        let root = test_directory("search-tab");
        std::fs::write(root.join("notes.txt"), "first\nSearch target\n").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);

        assert!(matches!(
            state.search_workspace(
                Some(workspace_id),
                TabId::new(1),
                "target",
                SearchMode::Content,
            ),
            Err(SearchError::TabNotFound)
        ));
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::Search,
        ));

        let results = state
            .search_workspace(
                Some(workspace_id),
                TabId::new(1),
                "target",
                SearchMode::Content,
            )
            .unwrap();
        assert_eq!(results.matches().len(), 1);
        assert_eq!(results.matches()[0].line_number(), Some(2));
        let document = state
            .search_document(Some(workspace_id), TabId::new(1), "notes.txt")
            .unwrap();
        assert_eq!(document.content(), "first\nSearch target\n");

        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
            .unwrap();
        let workspace = state.workspaces().workspace(workspace_id).unwrap();
        assert_eq!(
            workspace.active_pane().unwrap().active_tab().kind(),
            &TabKind::Editor
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn editor_document_uses_tab_state_and_saves_existing_file() {
        let root = test_directory("editor-document");
        std::fs::write(root.join("notes.txt"), "before").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
            .unwrap();

        let document = state
            .editor_document(Some(workspace_id), TabId::new(2))
            .unwrap();
        assert_eq!(document.content(), "before");

        state
            .save_editor_document(Some(workspace_id), TabId::new(2), "after")
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "after"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn editor_git_line_hunks_use_the_editor_view_path() {
        let root = test_directory("editor-git-line-hunks");
        std::fs::write(root.join("notes.txt"), "first\nsecond\n").unwrap();
        GitRepository::init(&root).expect("repository should initialize");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
            .unwrap();

        let hunks = state
            .editor_git_line_hunks(Some(workspace_id), TabId::new(2))
            .expect("editor line hunks should load");

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].new_start(), 1);
        assert_eq!(hunks[0].new_lines(), 2);
        assert!(matches!(
            state.editor_git_line_hunks(Some(workspace_id), TabId::new(1)),
            Err(GitError::TabNotFound)
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closing_editor_tabs_and_workspaces_removes_view_state() {
        let root = test_directory("editor-cleanup");
        std::fs::write(root.join("notes.txt"), "notes").unwrap();
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(
            Some(workspace_id),
            PaneId::new(1),
            TabId::new(1),
            TabKind::FileTree,
        ));
        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
            .unwrap();

        assert!(
            state
                .close_tab(Some(workspace_id), PaneId::new(1), TabId::new(2))
                .is_some()
        );
        assert!(state.editor_view_states().is_empty());

        state
            .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
            .unwrap();
        assert!(state.close_workspace(Some(workspace_id)).is_some());
        assert!(state.editor_view_states().is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn splitting_tab_moves_it_to_a_new_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        state.open_tab(None, None, None, TabKind::Search);

        assert!(state.split_tab(
            None,
            PaneId::new(1),
            PaneId::new(1),
            TabId::new(2),
            SplitAxis::Horizontal,
            false,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");

        assert_eq!(workspace.root().pane_count(), 2);
        assert_eq!(workspace.active_pane_id(), PaneId::new(2));

        let source_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("source pane should remain");
        let new_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("new pane should exist");

        assert_eq!(source_pane.tabs().len(), 1);
        assert_eq!(new_pane.active_tab().id(), TabId::new(2));
    }

    #[test]
    fn splitting_only_tab_keeps_source_pane_valid() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");

        assert!(state.split_tab(
            None,
            PaneId::new(1),
            PaneId::new(1),
            TabId::new(1),
            SplitAxis::Vertical,
            false,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let source_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("source pane should remain");
        let new_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("new pane should exist");

        assert_eq!(source_pane.tabs().len(), 1);
        assert_eq!(new_pane.active_tab().id(), TabId::new(1));
    }

    #[test]
    fn moving_pane_reuses_existing_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.move_pane(
            None,
            PaneId::new(1),
            PaneId::new(2),
            SplitAxis::Vertical,
            false,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");

        assert_eq!(workspace.root().pane_count(), 2);
        assert!(workspace.root().contains_pane(PaneId::new(1)));
        assert!(workspace.root().contains_pane(PaneId::new(2)));
        assert_eq!(workspace.active_pane_id(), PaneId::new(1));
    }

    #[test]
    fn moving_tab_to_another_pane_adds_it_to_target_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        state.open_tab(None, None, None, TabKind::Search);
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.move_tab(None, PaneId::new(1), PaneId::new(2), TabId::new(2), 1,));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let source_pane = workspace
            .root()
            .find_pane(PaneId::new(1))
            .expect("source pane should remain");
        let target_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("target pane should exist");

        assert_eq!(source_pane.tabs().len(), 1);
        assert_eq!(
            target_pane.tabs().iter().map(Tab::id).collect::<Vec<_>>(),
            vec![TabId::new(3), TabId::new(2)]
        );
        assert_eq!(target_pane.active_tab_id(), TabId::new(2));
        assert_eq!(workspace.active_pane_id(), PaneId::new(2));
    }

    #[test]
    fn moving_last_tab_to_another_pane_removes_source_pane() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.move_tab(
            None,
            PaneId::new(1),
            PaneId::new(2),
            TabId::new(1),
            usize::MAX,
        ));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let target_pane = workspace
            .root()
            .find_pane(PaneId::new(2))
            .expect("target pane should exist");

        assert_eq!(workspace.root().pane_count(), 1);
        assert!(!workspace.root().contains_pane(PaneId::new(1)));
        assert_eq!(
            target_pane.tabs().iter().map(Tab::id).collect::<Vec<_>>(),
            vec![TabId::new(2), TabId::new(1)]
        );
        assert_eq!(target_pane.active_tab_id(), TabId::new(1));
        assert_eq!(workspace.active_pane_id(), PaneId::new(2));
    }

    #[test]
    fn resizing_split_updates_server_owned_ratio() {
        let mut state = State::new();
        state.open_workspace("/workspaces/main");
        assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

        assert!(state.resize_split(None, SplitPaneId::new(1), 0.7));
        assert!(!state.resize_split(None, SplitPaneId::new(1), 1.0));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active");
        let crate::tree::PaneNode::Split(split) = workspace.root() else {
            panic!("workspace root should be split");
        };

        assert_eq!(split.ratio(), 0.7);
    }

    #[test]
    fn resized_split_survives_workspace_switches() {
        let mut state = State::new();
        let first_workspace_id = state.open_workspace("/workspaces/first");
        assert!(state.split_pane(Some(first_workspace_id), None, SplitAxis::Horizontal, false,));
        assert!(state.resize_split(Some(first_workspace_id), SplitPaneId::new(1), 0.7));

        state.open_workspace("/workspaces/second");
        assert!(state.activate_workspace(first_workspace_id));

        let workspace = state
            .workspaces()
            .active_workspace()
            .expect("first workspace should be active again");
        let crate::tree::PaneNode::Split(split) = workspace.root() else {
            panic!("workspace root should be split");
        };

        assert_eq!(split.ratio(), 0.7);
    }

    #[test]
    fn file_tree_requires_a_workspace() {
        let state = State::new();

        let error = state
            .file_tree(None, None)
            .expect_err("missing workspace should fail");

        assert!(matches!(error, FileTreeError::WorkspaceNotFound));
    }

    #[test]
    fn file_tree_expanded_paths_are_stored_for_file_tree_tabs() {
        let root = test_directory("file-tree-state");
        std::fs::create_dir(root.join("src")).expect("test directory should be created");
        let mut state = State::new();
        let workspace_id = state.open_workspace(&root);
        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::FileTree));

        assert!(state.set_file_tree_expanded_paths(
            Some(workspace_id),
            TabId::new(1),
            vec!["src".to_owned(), "missing".to_owned()],
        ));

        let file_tree = state
            .file_tree(Some(workspace_id), Some(TabId::new(1)))
            .expect("file tree should load");

        assert_eq!(file_tree.expanded_paths(), &["src/"]);
        assert_eq!(state.file_tree_view_states().len(), 1);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn file_tree_expanded_paths_require_a_file_tree_tab() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");

        assert!(!state.set_file_tree_expanded_paths(
            Some(workspace_id),
            TabId::new(1),
            vec!["src".to_owned()],
        ));

        let error = state
            .file_tree(Some(workspace_id), Some(TabId::new(1)))
            .expect_err("blank tabs should not expose file tree state");

        assert!(matches!(error, FileTreeError::TabNotFound));
    }

    #[test]
    fn terminal_sessions_require_a_terminal_tab() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");

        let error = state
            .read_terminal_output(Some(workspace_id), TabId::new(1))
            .expect_err("blank tabs should not expose terminal sessions");

        assert!(matches!(error, TerminalError::TabNotFound));
        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Terminal));

        let error = state
            .read_terminal_output(Some(workspace_id), TabId::new(1))
            .expect_err("terminal tabs should require a started session");

        assert!(matches!(error, TerminalError::SessionNotFound));
    }

    #[test]
    fn closing_tab_removes_file_tree_view_state() {
        let mut state = State::new();
        let workspace_id = state.open_workspace("/workspaces/main");
        assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::FileTree));
        assert!(state.set_file_tree_expanded_paths(
            Some(workspace_id),
            TabId::new(1),
            vec!["src".to_owned()],
        ));

        assert!(
            state
                .close_tab(Some(workspace_id), PaneId::new(1), TabId::new(1))
                .is_some()
        );

        assert!(state.file_tree_view_states().is_empty());
    }

    fn test_directory(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kosmos-core-state-{}-{name}-{nanos}",
            std::process::id()
        ));

        std::fs::create_dir_all(&root).expect("test root should be created");

        root
    }
}

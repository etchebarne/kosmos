use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

mod editor;
mod file_tree;
mod git;
mod search;
mod terminal;
mod tooling;
mod workspace;
mod workspace_edits;

use crate::formatters::FormatterManager;
use crate::language_servers::{
    LanguageServerManager, LanguageServerPosition, LanguageServerTextEdit,
};
use crate::settings::{ResolvedSettings, SettingValue, Settings, SettingsError};
use crate::tabs::editor::{EditorViewState, normalize_path as normalize_editor_path};
use crate::tabs::file_tree::{FileTreeError, FileTreeViewState};
use crate::tabs::git::{GitDiffViewState, GitError};
use crate::tabs::search::SearchError;
use crate::tabs::terminal::{TerminalError, TerminalSessions, TerminalViewState};
use crate::tree::{
    Pane, PaneId, PaneNode, SplitPaneId, Tab, TabId, TabKind, Workspace, WorkspaceId, WorkspaceList,
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
    terminal_view_states: Vec<TerminalViewState>,
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
    settings_revision: u64,
    persistence_scope: PersistenceScope,
    tooling_capabilities: crate::events::ToolingCapabilities,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenEditorLocation {
    workspaces: WorkspaceList,
    source_tab_id: TabId,
    workspace_id: WorkspaceId,
    tab_id: TabId,
    path: String,
}

#[derive(Debug)]
pub enum FileTreeGitDecorationsError {
    FileTree(FileTreeError),
    Git(GitError),
}

impl std::fmt::Display for FileTreeGitDecorationsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileTree(error) => error.fmt(formatter),
            Self::Git(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for FileTreeGitDecorationsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FileTree(error) => Some(error),
            Self::Git(error) => Some(error),
        }
    }
}

impl OpenEditorLocation {
    pub fn workspaces(&self) -> &WorkspaceList {
        &self.workspaces
    }

    pub fn source_tab_id(&self) -> TabId {
        self.source_tab_id
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    pub fn path(&self) -> &str {
        &self.path
    }
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
    source_settings: Settings,
    source_instance_id: u64,
    source_revision: u64,
    settings_persisted: bool,
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

    pub(crate) fn mark_settings_persisted(&mut self) {
        if self.settings_persisted || self.state.settings == self.source_settings {
            return;
        }

        self.state.settings_revision = self.state.settings_revision.saturating_add(1);
        self.settings_persisted = true;
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

    pub fn resolved_settings(&self) -> ResolvedSettings {
        ResolvedSettings::new(self.settings_revision, &self.settings)
    }

    pub fn settings_revision(&self) -> u64 {
        self.settings_revision
    }

    pub fn window_state(&self) -> Option<WindowState> {
        self.window_state
    }

    pub fn update_window_state(&mut self, window_state: WindowState) {
        if self.window_state != Some(window_state) {
            self.window_state = Some(window_state);
            self.mark_persistent_change_with_scope(PersistenceScope::Window);
        }
    }

    pub fn update_setting(&mut self, id: &str, value: SettingValue) -> Result<bool, SettingsError> {
        let changed = self.settings.update(id, value)?;
        if changed {
            self.mark_persistent_change_with_scope(PersistenceScope::Settings);
        }

        Ok(changed)
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

    pub fn terminal_view_states(&self) -> &[TerminalViewState] {
        &self.terminal_view_states
    }

    pub(crate) fn add_terminal_view_state(&mut self, view_state: TerminalViewState) -> bool {
        if !view_state.directory().is_absolute()
            || !self.is_terminal_tab(view_state.workspace_id(), view_state.tab_id())
            || self
                .terminal_view_state(view_state.workspace_id(), view_state.tab_id())
                .is_some()
        {
            return false;
        }

        self.terminal_view_states.push(view_state);
        true
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
        let terminal_view_states = self.refreshed_terminal_view_states();

        PersistentStateCandidate {
            state: Self {
                settings: self.settings.clone(),
                window_state: self.window_state,
                workspaces: self.workspaces.clone(),
                file_tree_view_states: self.file_tree_view_states.clone(),
                git_diff_view_states: self.git_diff_view_states.clone(),
                editor_view_states: self.editor_view_states.clone(),
                terminal_view_states,
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
                settings_revision: self.settings_revision,
                persistence_scope: self.persistence_scope,
                tooling_capabilities: self.tooling_capabilities.clone(),
            },
            source_settings: self.settings.clone(),
            source_instance_id: self.instance_id,
            source_revision: self.persistent_revision,
            settings_persisted: false,
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
        self.terminal_view_states = candidate.terminal_view_states;
        self.workspace_edit_editor_recovery = candidate.workspace_edit_editor_recovery;
        self.next_workspace_id = candidate.next_workspace_id;
        self.next_pane_id = candidate.next_pane_id;
        self.next_split_id = candidate.next_split_id;
        self.next_tab_id = candidate.next_tab_id;
        self.settings_revision = candidate.settings_revision;
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

    fn terminal_view_state(
        &self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
    ) -> Option<&TerminalViewState> {
        self.terminal_view_states
            .iter()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
    }

    fn update_terminal_view_state(
        &mut self,
        workspace_id: WorkspaceId,
        tab_id: TabId,
        directory: impl Into<std::path::PathBuf>,
    ) {
        let directory = directory.into();

        if let Some(state) = self
            .terminal_view_states
            .iter_mut()
            .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
        {
            state.set_directory(directory);
        } else {
            self.terminal_view_states
                .push(TerminalViewState::new(workspace_id, tab_id, directory));
        }
    }

    fn remove_terminal_view_state(&mut self, workspace_id: WorkspaceId, tab_id: TabId) {
        self.terminal_view_states
            .retain(|state| state.workspace_id() != workspace_id || state.tab_id() != tab_id);
    }

    fn remove_workspace_terminal_view_states(&mut self, workspace_id: WorkspaceId) {
        self.terminal_view_states
            .retain(|state| state.workspace_id() != workspace_id);
    }

    fn refreshed_terminal_view_states(&self) -> Vec<TerminalViewState> {
        let mut view_states = self.terminal_view_states.clone();
        view_states.retain(|state| self.is_terminal_tab(state.workspace_id(), state.tab_id()));

        for (workspace_id, tab_id, directory) in self.terminal_sessions.working_directories() {
            if !self.is_terminal_tab(workspace_id, tab_id) {
                continue;
            }

            if let Some(state) = view_states
                .iter_mut()
                .find(|state| state.workspace_id() == workspace_id && state.tab_id() == tab_id)
            {
                state.set_directory(directory);
            } else {
                view_states.push(TerminalViewState::new(workspace_id, tab_id, directory));
            }
        }

        view_states
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

    fn editor_source_tab_id(&self, workspace_id: WorkspaceId) -> Option<TabId> {
        self.workspaces
            .workspace(workspace_id)?
            .root()
            .first_tab_id_matching(&|tab| matches!(tab.kind(), TabKind::FileTree | TabKind::Search))
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
            terminal_view_states: Vec::new(),
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
            settings_revision: 0,
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
            terminal_view_states: Vec::new(),
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
            settings_revision: 0,
            persistence_scope: PersistenceScope::Clean,
            tooling_capabilities: crate::events::ToolingCapabilities::default(),
        }
    }
}

fn next_state_instance_id() -> u64 {
    NEXT_STATE_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}
#[cfg(test)]
mod tests;

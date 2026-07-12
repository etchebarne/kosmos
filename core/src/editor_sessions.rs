use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};

use crate::language_servers::WorkspaceEditOpenDocument;
use crate::language_servers::{StagedWorkspaceEdit, StagedWorkspaceEditOperation};
use crate::tabs::editor::{MAX_EDITOR_FILE_BYTES, normalize_path};
use crate::tree::{TabId, WorkspaceId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EditorSessionId {
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
}

impl EditorSessionId {
    pub const fn new(workspace_id: WorkspaceId, tab_id: TabId) -> Self {
        Self {
            workspace_id,
            tab_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorSessionSnapshot {
    pub id: EditorSessionId,
    pub path: String,
    pub content: String,
    pub saved_content: String,
    pub revision: u64,
}

impl EditorSessionSnapshot {
    pub fn is_dirty(&self) -> bool {
        self.content != self.saved_content
    }
}

#[derive(Clone, Default)]
pub struct EditorSessionRegistry {
    sessions: HashMap<EditorSessionId, EditorSessionSnapshot>,
    save_gates: HashMap<EditorSessionId, EditorSessionSaveGate>,
}

#[derive(Clone)]
pub struct EditorSessionSaveGate {
    state: Arc<(Mutex<EditorSessionSaveGateState>, Condvar)>,
}

#[derive(Default)]
struct EditorSessionSaveGateState {
    next_sequence: u64,
    next_to_run: u64,
    completed: std::collections::BTreeSet<u64>,
}

pub struct EditorSessionSaveTicket {
    gate: EditorSessionSaveGate,
    sequence: u64,
    pending: bool,
}

pub struct EditorSessionSavePermit {
    gate: EditorSessionSaveGate,
    sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditorSessionUpdate {
    Applied(EditorSessionSnapshot),
    Stale(EditorSessionSnapshot),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditorSessionError {
    ContentTooLarge,
    InvalidPath(String),
    Missing(EditorSessionId),
    PathMismatch { expected: String, received: String },
    StaleRevision { expected: u64, received: u64 },
}

impl EditorSessionRegistry {
    pub fn open(
        &mut self,
        id: EditorSessionId,
        path: &str,
        content: String,
        revision: u64,
    ) -> Result<EditorSessionUpdate, EditorSessionError> {
        let path = normalized_path(path)?;
        bounded_content(&content)?;

        let Some(current) = self.sessions.get_mut(&id) else {
            let snapshot = EditorSessionSnapshot {
                id,
                path,
                saved_content: content.clone(),
                content,
                revision,
            };
            self.sessions.insert(id, snapshot.clone());
            self.save_gates.entry(id).or_default();
            return Ok(EditorSessionUpdate::Applied(snapshot));
        };

        if current.path != path {
            return Err(EditorSessionError::PathMismatch {
                expected: current.path.clone(),
                received: path,
            });
        }
        if revision < current.revision
            || (revision == current.revision && content != current.content)
        {
            return Ok(EditorSessionUpdate::Stale(current.clone()));
        }
        if revision > current.revision {
            current.content = content;
            current.revision = revision;
        }
        Ok(EditorSessionUpdate::Applied(current.clone()))
    }

    pub fn change(
        &mut self,
        id: EditorSessionId,
        content: String,
        revision: u64,
    ) -> Result<EditorSessionUpdate, EditorSessionError> {
        bounded_content(&content)?;
        let current = self
            .sessions
            .get_mut(&id)
            .ok_or(EditorSessionError::Missing(id))?;
        if revision <= current.revision {
            return Ok(EditorSessionUpdate::Stale(current.clone()));
        }
        current.content = content;
        current.revision = revision;
        Ok(EditorSessionUpdate::Applied(current.clone()))
    }

    pub fn mark_saved(
        &mut self,
        id: EditorSessionId,
        revision: u64,
    ) -> Result<EditorSessionSnapshot, EditorSessionError> {
        let current = self
            .sessions
            .get_mut(&id)
            .ok_or(EditorSessionError::Missing(id))?;
        if revision != current.revision {
            return Err(EditorSessionError::StaleRevision {
                expected: current.revision,
                received: revision,
            });
        }
        current.saved_content = current.content.clone();
        Ok(current.clone())
    }

    pub fn prepare_save(
        &mut self,
        id: EditorSessionId,
        revision: u64,
    ) -> Result<(EditorSessionSnapshot, EditorSessionSaveTicket), EditorSessionError> {
        let session = self
            .sessions
            .get(&id)
            .cloned()
            .ok_or(EditorSessionError::Missing(id))?;
        if session.revision != revision {
            return Err(EditorSessionError::StaleRevision {
                expected: session.revision,
                received: revision,
            });
        }
        let ticket = self.save_gates.entry(id).or_default().issue();
        Ok((session, ticket))
    }

    pub fn complete_save(
        &mut self,
        id: EditorSessionId,
        revision: u64,
        saved_content: String,
    ) -> Result<EditorSessionSnapshot, EditorSessionError> {
        let current = self
            .sessions
            .get_mut(&id)
            .ok_or(EditorSessionError::Missing(id))?;
        if revision > current.revision {
            return Err(EditorSessionError::StaleRevision {
                expected: current.revision,
                received: revision,
            });
        }
        if revision == current.revision {
            current.content = saved_content.clone();
        }
        current.saved_content = saved_content;
        Ok(current.clone())
    }

    pub fn snapshot(&self, id: EditorSessionId) -> Option<EditorSessionSnapshot> {
        self.sessions.get(&id).cloned()
    }

    pub fn dirty_for_workspace(&self, workspace_id: WorkspaceId) -> Vec<EditorSessionSnapshot> {
        self.sessions
            .values()
            .filter(|session| session.id.workspace_id == workspace_id && session.is_dirty())
            .cloned()
            .collect()
    }

    pub fn dirty_for_ids(&self, ids: &[EditorSessionId]) -> Vec<EditorSessionSnapshot> {
        ids.iter()
            .filter_map(|id| self.sessions.get(id))
            .filter(|session| session.is_dirty())
            .cloned()
            .collect()
    }

    pub fn ids_for_workspace(&self, workspace_id: WorkspaceId) -> Vec<EditorSessionId> {
        self.sessions
            .keys()
            .filter(|id| id.workspace_id == workspace_id)
            .copied()
            .collect()
    }

    pub fn ids(&self) -> Vec<EditorSessionId> {
        self.sessions.keys().copied().collect()
    }

    pub fn remove(&mut self, id: EditorSessionId) {
        self.sessions.remove(&id);
        self.save_gates.remove(&id);
    }

    pub fn remove_workspace(&mut self, workspace_id: WorkspaceId) {
        self.sessions
            .retain(|id, _| id.workspace_id != workspace_id);
        self.save_gates
            .retain(|id, _| id.workspace_id != workspace_id);
    }

    pub fn workspace_edit_observations(&self) -> Vec<WorkspaceEditOpenDocument> {
        self.sessions
            .values()
            .map(|session| WorkspaceEditOpenDocument {
                workspace_id: session.id.workspace_id,
                path: session.path.clone(),
                generation: session.revision,
                version: i64::try_from(session.revision).unwrap_or(i64::MAX),
                text: session.content.clone(),
                saved_text: session.saved_content.clone(),
            })
            .collect()
    }

    pub fn apply_workspace_edit(&mut self, edit: &StagedWorkspaceEdit) {
        for operation in &edit.operations {
            match operation {
                StagedWorkspaceEditOperation::TextDocument { document } => {
                    let Some(document) = edit.documents.get(*document) else {
                        continue;
                    };
                    for session in self.sessions.values_mut().filter(|session| {
                        session.id.workspace_id == document.workspace_id
                            && session.path == document.path
                    }) {
                        session.content = document.new_text.clone();
                        session.saved_content = document.new_text.clone();
                        advance_revision(session);
                    }
                }
                StagedWorkspaceEditOperation::RenameFile {
                    workspace_id,
                    old_path,
                    new_path,
                } => {
                    for session in self
                        .sessions
                        .values_mut()
                        .filter(|session| session.id.workspace_id == *workspace_id)
                    {
                        if let Some(path) = remap_path(&session.path, old_path, new_path) {
                            session.path = path;
                            advance_revision(session);
                        }
                    }
                }
                StagedWorkspaceEditOperation::DeleteFile {
                    workspace_id, path, ..
                } => {
                    let removed = self
                        .sessions
                        .iter()
                        .filter(|(_, session)| {
                            session.id.workspace_id == *workspace_id
                                && path_at_or_below(&session.path, path)
                        })
                        .map(|(id, _)| *id)
                        .collect::<Vec<_>>();
                    self.sessions.retain(|_, session| {
                        session.id.workspace_id != *workspace_id
                            || !path_at_or_below(&session.path, path)
                    });
                    for id in removed {
                        self.save_gates.remove(&id);
                    }
                }
                StagedWorkspaceEditOperation::CreateFile { .. } => {}
            }
        }
    }
}

impl Default for EditorSessionSaveGate {
    fn default() -> Self {
        Self {
            state: Arc::new((
                Mutex::new(EditorSessionSaveGateState::default()),
                Condvar::new(),
            )),
        }
    }
}

impl EditorSessionSaveGate {
    fn issue(&self) -> EditorSessionSaveTicket {
        let (state, _) = &*self.state;
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        EditorSessionSaveTicket {
            gate: self.clone(),
            sequence,
            pending: true,
        }
    }

    fn complete(&self, sequence: u64) {
        let (state, wake) = &*self.state;
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        state.completed.insert(sequence);
        while {
            let next = state.next_to_run;
            state.completed.remove(&next)
        } {
            state.next_to_run = state.next_to_run.saturating_add(1);
        }
        wake.notify_all();
    }
}

impl EditorSessionSaveTicket {
    pub fn acquire(mut self) -> EditorSessionSavePermit {
        let (state, wake) = &*self.gate.state;
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        while state.next_to_run != self.sequence {
            state = wake.wait(state).unwrap_or_else(|error| error.into_inner());
        }
        self.pending = false;
        EditorSessionSavePermit {
            gate: self.gate.clone(),
            sequence: self.sequence,
        }
    }
}

impl Drop for EditorSessionSaveTicket {
    fn drop(&mut self) {
        if self.pending {
            self.gate.complete(self.sequence);
        }
    }
}

impl Drop for EditorSessionSavePermit {
    fn drop(&mut self) {
        self.gate.complete(self.sequence);
    }
}

fn advance_revision(session: &mut EditorSessionSnapshot) {
    session.revision = session.revision.saturating_add(1);
}

fn remap_path(path: &str, source: &str, destination: &str) -> Option<String> {
    if path == source {
        return Some(destination.to_owned());
    }
    path.strip_prefix(source)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .map(|suffix| format!("{destination}/{suffix}"))
}

fn path_at_or_below(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn normalized_path(path: &str) -> Result<String, EditorSessionError> {
    normalize_path(path).map_err(|_| EditorSessionError::InvalidPath(path.to_owned()))
}

fn bounded_content(content: &str) -> Result<(), EditorSessionError> {
    (content.len() <= MAX_EDITOR_FILE_BYTES)
        .then_some(())
        .ok_or(EditorSessionError::ContentTooLarge)
}

impl fmt::Display for EditorSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContentTooLarge => {
                formatter.write_str("editor session content exceeds the size limit")
            }
            Self::InvalidPath(path) => write!(formatter, "invalid editor session path: {path:?}"),
            Self::Missing(_) => formatter.write_str("editor session does not exist"),
            Self::PathMismatch { expected, received } => write!(
                formatter,
                "editor session path changed from {expected:?} to {received:?}"
            ),
            Self::StaleRevision { expected, received } => write!(
                formatter,
                "editor session revision {received} does not match current revision {expected}"
            ),
        }
    }
}

impl std::error::Error for EditorSessionError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> EditorSessionId {
        EditorSessionId::new(WorkspaceId::new(1), TabId::new(2))
    }

    #[test]
    fn changes_are_revisioned_and_stale_updates_are_rejected() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), "src/main.rs", "one".to_owned(), 1)
            .unwrap();
        let changed = sessions.change(id(), "two".to_owned(), 2).unwrap();
        assert!(matches!(changed, EditorSessionUpdate::Applied(_)));
        let stale = sessions.change(id(), "one".to_owned(), 1).unwrap();
        assert!(matches!(stale, EditorSessionUpdate::Stale(snapshot) if snapshot.content == "two"));
        assert_eq!(sessions.snapshot(id()).unwrap().revision, 2);
    }

    #[test]
    fn dirty_status_tracks_the_saved_baseline() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), "src/main.rs", "one".to_owned(), 1)
            .unwrap();
        sessions.change(id(), "two".to_owned(), 2).unwrap();
        assert!(sessions.snapshot(id()).unwrap().is_dirty());
        sessions.mark_saved(id(), 2).unwrap();
        assert!(!sessions.snapshot(id()).unwrap().is_dirty());
    }

    #[test]
    fn content_limit_is_enforced_before_session_mutation() {
        let mut sessions = EditorSessionRegistry::default();
        assert_eq!(
            sessions.open(
                id(),
                "src/main.rs",
                "x".repeat(MAX_EDITOR_FILE_BYTES + 1),
                1
            ),
            Err(EditorSessionError::ContentTooLarge)
        );
        assert!(sessions.snapshot(id()).is_none());
    }
}

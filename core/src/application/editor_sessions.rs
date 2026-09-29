use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};

use ropey::Rope;

use crate::language_servers::WorkspaceEditOpenDocument;
use crate::language_servers::{StagedWorkspaceEdit, StagedWorkspaceEditOperation};
use crate::state::{
    DocumentScope, DocumentSessionTarget, path_is_at_or_below, remap_workspace_path,
};
use crate::tabs::editor::{DocumentFingerprint, MAX_EDITOR_FILE_BYTES, normalize_path};
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
    pub saved_generation: u64,
}

impl EditorSessionSnapshot {
    pub fn is_dirty(&self) -> bool {
        self.content != self.saved_content
    }
}

/// Lightweight acknowledgement of a session update; it never carries document text so
/// frequent edits stay cheap regardless of document size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditorSessionAck {
    pub revision: u64,
    pub saved_generation: u64,
}

/// One text replacement, addressed in UTF-16 code units like the renderer's model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorTextEdit {
    pub offset: usize,
    pub length: usize,
    pub text: String,
}

#[derive(Clone)]
struct EditorSession {
    id: EditorSessionId,
    root: std::path::PathBuf,
    scope: DocumentScope,
    path: String,
    content: Rope,
    saved_content: Rope,
    revision: u64,
    saved_generation: u64,
    disk: Option<DocumentFingerprint>,
}

impl EditorSession {
    fn new(
        id: EditorSessionId,
        target: &DocumentSessionTarget,
        content: Rope,
        saved_content: Rope,
        revision: u64,
    ) -> Self {
        Self {
            id,
            root: target.root.clone(),
            scope: target.scope,
            path: target.path.clone(),
            content,
            saved_content,
            revision,
            saved_generation: 0,
            disk: None,
        }
    }

    fn is_dirty(&self) -> bool {
        self.content != self.saved_content
    }

    fn in_workspace(&self, workspace_id: WorkspaceId) -> bool {
        self.id.workspace_id == workspace_id && self.scope == DocumentScope::Workspace
    }

    fn ack(&self) -> EditorSessionAck {
        EditorSessionAck {
            revision: self.revision,
            saved_generation: self.saved_generation,
        }
    }

    fn snapshot(&self) -> EditorSessionSnapshot {
        let content = self.content.to_string();
        let saved_content = if self.is_dirty() {
            self.saved_content.to_string()
        } else {
            content.clone()
        };
        EditorSessionSnapshot {
            id: self.id,
            path: self.path.clone(),
            content,
            saved_content,
            revision: self.revision,
            saved_generation: self.saved_generation,
        }
    }

    fn set_saved(&mut self, saved_content: Rope) {
        self.saved_content = saved_content;
        self.saved_generation = self.saved_generation.saturating_add(1);
    }
}

#[derive(Clone, Default)]
pub struct EditorSessionRegistry {
    sessions: HashMap<EditorSessionId, EditorSession>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditorSessionUpdate {
    Applied(EditorSessionAck),
    Stale(EditorSessionAck),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditorSessionError {
    ContentTooLarge,
    InvalidEdit,
    InvalidPath(String),
    Missing(EditorSessionId),
    PathMismatch { expected: String, received: String },
    StaleRevision { expected: u64, received: u64 },
}

impl EditorSessionRegistry {
    pub fn restore(
        &mut self,
        id: EditorSessionId,
        target: &DocumentSessionTarget,
        content: String,
        saved_content: String,
        revision: u64,
    ) -> Result<EditorSessionUpdate, EditorSessionError> {
        if self.sessions.contains_key(&id) {
            return self.open(id, target, content, revision);
        }

        normalized_path(&target.path)?;
        bounded_content(content.len())?;
        bounded_content(saved_content.len())?;
        let session = EditorSession::new(
            id,
            target,
            Rope::from_str(&content),
            Rope::from_str(&saved_content),
            revision,
        );
        let ack = session.ack();
        self.insert(session);
        Ok(EditorSessionUpdate::Applied(ack))
    }

    pub fn open(
        &mut self,
        id: EditorSessionId,
        target: &DocumentSessionTarget,
        content: String,
        revision: u64,
    ) -> Result<EditorSessionUpdate, EditorSessionError> {
        let path = normalized_path(&target.path)?;
        bounded_content(content.len())?;

        let Some(current) = self.sessions.get_mut(&id) else {
            let content = Rope::from_str(&content);
            let session = EditorSession::new(id, target, content.clone(), content, revision);
            let ack = session.ack();
            self.insert(session);
            return Ok(EditorSessionUpdate::Applied(ack));
        };

        if current.path != path {
            return Err(EditorSessionError::PathMismatch {
                expected: current.path.clone(),
                received: path,
            });
        }
        if revision < current.revision
            || (revision == current.revision && current.content != content)
        {
            return Ok(EditorSessionUpdate::Stale(current.ack()));
        }
        if revision > current.revision {
            current.content = Rope::from_str(&content);
            current.revision = revision;
        }
        Ok(EditorSessionUpdate::Applied(current.ack()))
    }

    /// Starts tracking a document read from disk, unless the tab already has a session.
    pub fn open_from_disk(
        &mut self,
        id: EditorSessionId,
        target: &DocumentSessionTarget,
        content: &str,
        disk: Option<DocumentFingerprint>,
    ) -> Result<(), EditorSessionError> {
        if self.sessions.contains_key(&id) {
            return Ok(());
        }
        normalized_path(&target.path)?;
        bounded_content(content.len())?;
        let content = Rope::from_str(content);
        let mut session = EditorSession::new(id, target, content.clone(), content, 0);
        session.disk = disk;
        self.insert(session);
        Ok(())
    }

    /// Applies ordered edits made on top of `base_revision`. Edits based on any other
    /// revision are rejected as stale so the caller can resynchronize the full text.
    pub fn change(
        &mut self,
        id: EditorSessionId,
        base_revision: u64,
        revision: u64,
        edits: &[EditorTextEdit],
    ) -> Result<EditorSessionUpdate, EditorSessionError> {
        let current = self
            .sessions
            .get_mut(&id)
            .ok_or(EditorSessionError::Missing(id))?;
        if base_revision != current.revision || revision <= current.revision {
            return Ok(EditorSessionUpdate::Stale(current.ack()));
        }
        let content = apply_edits(&current.content, edits)?;
        bounded_content(content.len_bytes())?;
        current.content = content;
        current.revision = revision;
        Ok(EditorSessionUpdate::Applied(current.ack()))
    }

    /// Replaces the whole text when `revision` is newer than the session's.
    pub fn replace(
        &mut self,
        id: EditorSessionId,
        content: String,
        revision: u64,
    ) -> Result<EditorSessionUpdate, EditorSessionError> {
        bounded_content(content.len())?;
        let current = self
            .sessions
            .get_mut(&id)
            .ok_or(EditorSessionError::Missing(id))?;
        if revision <= current.revision {
            return Ok(EditorSessionUpdate::Stale(current.ack()));
        }
        current.content = Rope::from_str(&content);
        current.revision = revision;
        Ok(EditorSessionUpdate::Applied(current.ack()))
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
        current.set_saved(current.content.clone());
        current.disk = None;
        Ok(current.snapshot())
    }

    pub fn prepare_save(
        &mut self,
        id: EditorSessionId,
        revision: u64,
    ) -> Result<(EditorSessionSnapshot, EditorSessionSaveTicket), EditorSessionError> {
        let session = self
            .sessions
            .get(&id)
            .ok_or(EditorSessionError::Missing(id))?;
        if session.revision != revision {
            return Err(EditorSessionError::StaleRevision {
                expected: session.revision,
                received: revision,
            });
        }
        let snapshot = session.snapshot();
        let ticket = self.save_gates.entry(id).or_default().issue();
        Ok((snapshot, ticket))
    }

    pub fn complete_save(
        &mut self,
        id: EditorSessionId,
        revision: u64,
        saved_content: String,
    ) -> Result<EditorSessionAck, EditorSessionError> {
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
        let saved_content = Rope::from_str(&saved_content);
        if revision == current.revision {
            current.content = saved_content.clone();
        }
        current.set_saved(saved_content);
        // The write just changed the file's metadata; the next observation re-reads it.
        current.disk = None;
        Ok(current.ack())
    }

    /// Where the session's document is read from and saved to.
    pub fn target(&self, id: EditorSessionId) -> Option<DocumentSessionTarget> {
        self.sessions.get(&id).map(|session| DocumentSessionTarget {
            workspace_id: session.id.workspace_id,
            root: session.root.clone(),
            path: session.path.clone(),
            scope: session.scope,
        })
    }

    /// The session's text, unless it exceeds `max_bytes`.
    pub fn content_within(&self, id: EditorSessionId, max_bytes: usize) -> Option<String> {
        self.sessions
            .get(&id)
            .filter(|session| session.content.len_bytes() <= max_bytes)
            .map(|session| session.content.to_string())
    }

    pub fn disk_fingerprint(&self, id: EditorSessionId) -> Option<DocumentFingerprint> {
        self.sessions.get(&id).and_then(|session| session.disk)
    }

    /// Records the document's current disk content. A clean session follows the disk,
    /// while a dirty session keeps its text and only moves its saved baseline.
    pub fn observe_disk_content(
        &mut self,
        id: EditorSessionId,
        disk_content: &str,
        disk: Option<DocumentFingerprint>,
    ) {
        let Some(current) = self.sessions.get_mut(&id) else {
            return;
        };
        current.disk = disk;
        if current.saved_content == disk_content {
            return;
        }
        let disk_content = Rope::from_str(disk_content);
        if !current.is_dirty() {
            current.content = disk_content.clone();
            advance_revision(current);
        }
        current.set_saved(disk_content);
    }

    /// Moves sessions whose document path was renamed at or below `source`.
    pub fn retarget_path(&mut self, workspace_id: WorkspaceId, source: &str, destination: &str) {
        for session in self
            .sessions
            .values_mut()
            .filter(|session| session.in_workspace(workspace_id))
        {
            if let Some(path) = remap_workspace_path(&session.path, source, destination) {
                session.path = path;
            }
        }
    }

    /// Returns the sessions whose document lives at or below `path`.
    /// Returns the paths of unsaved sessions whose document lives at or below `path`.
    pub fn unsaved_paths_at_or_below(&self, workspace_id: WorkspaceId, path: &str) -> Vec<String> {
        self.sessions
            .values()
            .filter(|session| {
                session.in_workspace(workspace_id)
                    && path_is_at_or_below(&session.path, path)
                    && session.is_dirty()
            })
            .map(|session| session.path.clone())
            .collect()
    }

    pub fn contains(&self, id: EditorSessionId) -> bool {
        self.sessions.contains_key(&id)
    }

    pub fn snapshot(&self, id: EditorSessionId) -> Option<EditorSessionSnapshot> {
        self.sessions.get(&id).map(EditorSession::snapshot)
    }

    pub fn ack(&self, id: EditorSessionId) -> Option<EditorSessionAck> {
        self.sessions.get(&id).map(EditorSession::ack)
    }

    pub fn dirty_for_workspace(&self, workspace_id: WorkspaceId) -> Vec<EditorSessionSnapshot> {
        self.sessions
            .values()
            .filter(|session| session.id.workspace_id == workspace_id && session.is_dirty())
            .map(EditorSession::snapshot)
            .collect()
    }

    pub fn dirty_for_ids(&self, ids: &[EditorSessionId]) -> Vec<EditorSessionSnapshot> {
        ids.iter()
            .filter_map(|id| self.sessions.get(id))
            .filter(|session| session.is_dirty())
            .map(EditorSession::snapshot)
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
            .filter(|session| session.scope == DocumentScope::Workspace)
            .map(|session| WorkspaceEditOpenDocument {
                workspace_id: session.id.workspace_id,
                path: session.path.clone(),
                generation: session.revision,
                version: i64::try_from(session.revision).unwrap_or(i64::MAX),
                text: session.content.to_string(),
                saved_text: session.saved_content.to_string(),
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
                        session.in_workspace(document.workspace_id) && session.path == document.path
                    }) {
                        let text = Rope::from_str(&document.new_text);
                        session.content = text.clone();
                        session.set_saved(text);
                        session.disk = None;
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
                        .filter(|session| session.in_workspace(*workspace_id))
                    {
                        if let Some(path) = remap_workspace_path(&session.path, old_path, new_path)
                        {
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
                            session.in_workspace(*workspace_id)
                                && path_is_at_or_below(&session.path, path)
                        })
                        .map(|(id, _)| *id)
                        .collect::<Vec<_>>();
                    self.sessions.retain(|_, session| {
                        !session.in_workspace(*workspace_id)
                            || !path_is_at_or_below(&session.path, path)
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

impl EditorSessionRegistry {
    fn insert(&mut self, session: EditorSession) {
        self.save_gates.entry(session.id).or_default();
        self.sessions.insert(session.id, session);
    }
}

/// Applies edits to a copy so an invalid edit leaves the session untouched; rope clones
/// share structure, so the copy is cheap.
fn apply_edits(content: &Rope, edits: &[EditorTextEdit]) -> Result<Rope, EditorSessionError> {
    let mut content = content.clone();
    for edit in edits {
        apply_edit(&mut content, edit)?;
    }
    Ok(content)
}

fn apply_edit(content: &mut Rope, edit: &EditorTextEdit) -> Result<(), EditorSessionError> {
    let end = edit
        .offset
        .checked_add(edit.length)
        .filter(|end| *end <= content.len_utf16_cu())
        .ok_or(EditorSessionError::InvalidEdit)?;
    let start = content.utf16_cu_to_char(edit.offset);
    let end = content.utf16_cu_to_char(end);
    content
        .try_remove(start..end)
        .and_then(|()| content.try_insert(start, &edit.text))
        .map_err(|_| EditorSessionError::InvalidEdit)
}

fn advance_revision(session: &mut EditorSession) {
    session.revision = session.revision.saturating_add(1);
}

fn normalized_path(path: &str) -> Result<String, EditorSessionError> {
    normalize_path(path).map_err(|_| EditorSessionError::InvalidPath(path.to_owned()))
}

fn bounded_content(len: usize) -> Result<(), EditorSessionError> {
    (len <= MAX_EDITOR_FILE_BYTES)
        .then_some(())
        .ok_or(EditorSessionError::ContentTooLarge)
}

impl fmt::Display for EditorSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContentTooLarge => write!(
                formatter,
                "The document is too large to edit (the limit is {}).",
                crate::byte_size::format_byte_size(MAX_EDITOR_FILE_BYTES)
            ),
            Self::InvalidEdit => formatter.write_str("editor edit does not fit the document"),
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

    fn target(path: &str) -> DocumentSessionTarget {
        DocumentSessionTarget {
            workspace_id: WorkspaceId::new(1),
            root: std::path::PathBuf::from("/workspace"),
            path: path.to_owned(),
            scope: DocumentScope::Workspace,
        }
    }

    fn edit(offset: usize, length: usize, text: &str) -> EditorTextEdit {
        EditorTextEdit {
            offset,
            length,
            text: text.to_owned(),
        }
    }

    #[test]
    fn changes_are_revisioned_and_stale_updates_are_rejected() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("src/main.rs"), "one".to_owned(), 1)
            .unwrap();
        let changed = sessions.replace(id(), "two".to_owned(), 2).unwrap();
        assert!(matches!(changed, EditorSessionUpdate::Applied(_)));
        let stale = sessions.replace(id(), "one".to_owned(), 1).unwrap();
        assert!(matches!(stale, EditorSessionUpdate::Stale(ack) if ack.revision == 2));
        assert_eq!(sessions.snapshot(id()).unwrap().content, "two");
    }

    #[test]
    fn incremental_edits_apply_in_order_on_their_base_revision() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("src/main.rs"), "hello world".to_owned(), 1)
            .unwrap();

        let update = sessions
            .change(id(), 1, 2, &[edit(6, 5, "rust"), edit(0, 5, "goodbye")])
            .unwrap();

        assert!(matches!(update, EditorSessionUpdate::Applied(ack) if ack.revision == 2));
        assert_eq!(sessions.snapshot(id()).unwrap().content, "goodbye rust");
    }

    #[test]
    fn incremental_edits_use_utf16_offsets() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("a.txt"), "é😀x\r\ny".to_owned(), 1)
            .unwrap();

        // "é" is one UTF-16 unit, the emoji two, then "x" and a CRLF line break.
        sessions
            .change(id(), 1, 2, &[edit(3, 1, "Z"), edit(4, 2, "\n")])
            .unwrap();

        assert_eq!(sessions.snapshot(id()).unwrap().content, "é😀Z\ny");
    }

    #[test]
    fn edits_on_an_outdated_base_are_stale_and_leave_the_text_untouched() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("a.txt"), "abc".to_owned(), 1)
            .unwrap();
        sessions.replace(id(), "xyz".to_owned(), 2).unwrap();

        let update = sessions.change(id(), 1, 3, &[edit(0, 1, "A")]).unwrap();

        assert!(matches!(update, EditorSessionUpdate::Stale(ack) if ack.revision == 2));
        assert_eq!(sessions.snapshot(id()).unwrap().content, "xyz");
    }

    #[test]
    fn out_of_range_edits_are_rejected_without_partial_application() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("a.txt"), "abc".to_owned(), 1)
            .unwrap();

        let result = sessions.change(id(), 1, 2, &[edit(0, 1, "A"), edit(2, 5, "")]);

        assert_eq!(result, Err(EditorSessionError::InvalidEdit));
        let snapshot = sessions.snapshot(id()).unwrap();
        assert_eq!((snapshot.content.as_str(), snapshot.revision), ("abc", 1));
    }

    #[test]
    fn saved_generation_advances_whenever_the_baseline_moves() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("a.txt"), "one".to_owned(), 1)
            .unwrap();
        sessions.replace(id(), "two".to_owned(), 2).unwrap();
        sessions.observe_disk_content(id(), "disk", None);
        assert_eq!(sessions.ack(id()).unwrap().saved_generation, 1);
        sessions.mark_saved(id(), 2).unwrap();
        assert_eq!(sessions.ack(id()).unwrap().saved_generation, 2);
    }

    #[test]
    fn disk_content_reloads_clean_sessions_and_rebases_dirty_ones() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("src/main.rs"), "one".to_owned(), 1)
            .unwrap();
        sessions.observe_disk_content(id(), "disk", None);
        let clean = sessions.snapshot(id()).unwrap();
        assert_eq!((clean.content.as_str(), clean.revision), ("disk", 2));

        sessions.replace(id(), "edited".to_owned(), 3).unwrap();
        sessions.observe_disk_content(id(), "disk two", None);
        let dirty = sessions.snapshot(id()).unwrap();
        assert_eq!(dirty.content, "edited");
        assert_eq!(dirty.saved_content, "disk two");
        assert_eq!(dirty.revision, 3);
    }

    #[test]
    fn retargeting_moves_sessions_at_or_below_the_renamed_path() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("src/main.rs"), "one".to_owned(), 1)
            .unwrap();
        sessions.retarget_path(WorkspaceId::new(1), "src", "app");
        assert_eq!(sessions.snapshot(id()).unwrap().path, "app/main.rs");
        sessions.replace(id(), "two".to_owned(), 2).unwrap();
        assert_eq!(
            sessions.unsaved_paths_at_or_below(WorkspaceId::new(1), "app"),
            ["app/main.rs"]
        );
        assert!(
            sessions
                .unsaved_paths_at_or_below(WorkspaceId::new(1), "ap")
                .is_empty()
        );
    }

    #[test]
    fn repository_scoped_sessions_stay_out_of_workspace_path_operations() {
        let mut sessions = EditorSessionRegistry::default();
        let diff = DocumentSessionTarget {
            scope: DocumentScope::Repository,
            ..target("src/main.rs")
        };
        sessions.open(id(), &diff, "one".to_owned(), 1).unwrap();
        sessions.replace(id(), "two".to_owned(), 2).unwrap();

        sessions.retarget_path(WorkspaceId::new(1), "src", "app");

        assert_eq!(sessions.snapshot(id()).unwrap().path, "src/main.rs");
        assert!(
            sessions
                .unsaved_paths_at_or_below(WorkspaceId::new(1), "src")
                .is_empty()
        );
        assert!(sessions.workspace_edit_observations().is_empty());
        assert_eq!(sessions.dirty_for_workspace(WorkspaceId::new(1)).len(), 1);
    }

    #[test]
    fn dirty_status_tracks_the_saved_baseline() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .open(id(), &target("src/main.rs"), "one".to_owned(), 1)
            .unwrap();
        sessions.replace(id(), "two".to_owned(), 2).unwrap();
        assert!(sessions.snapshot(id()).unwrap().is_dirty());
        sessions.mark_saved(id(), 2).unwrap();
        assert!(!sessions.snapshot(id()).unwrap().is_dirty());
    }

    #[test]
    fn restored_session_preserves_unsaved_content() {
        let mut sessions = EditorSessionRegistry::default();
        sessions
            .restore(
                id(),
                &target("src/main.rs"),
                "unsaved".to_owned(),
                "saved".to_owned(),
                3,
            )
            .unwrap();

        let restored = sessions.snapshot(id()).unwrap();
        assert_eq!(restored.content, "unsaved");
        assert_eq!(restored.saved_content, "saved");
        assert!(restored.is_dirty());
    }

    #[test]
    fn content_limit_is_enforced_before_session_mutation() {
        let mut sessions = EditorSessionRegistry::default();
        assert_eq!(
            sessions.open(
                id(),
                &target("src/main.rs"),
                "x".repeat(MAX_EDITOR_FILE_BYTES + 1),
                1
            ),
            Err(EditorSessionError::ContentTooLarge)
        );
        assert!(sessions.snapshot(id()).is_none());
    }
}

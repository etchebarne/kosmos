use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use percent_encoding::percent_decode_str;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::tabs::editor::{EditorLocation, MAX_EDITOR_FILE_BYTES};
use crate::tree::WorkspaceId;

use super::secure_edit::{FileIdentity, SecureEditFile, SecureReplaceError, random_token};
use super::{
    LanguageServerError, LanguageServerPosition, LanguageServerRange, LanguageServerTextEdit,
};

pub const MAX_WORKSPACE_EDIT_DOCUMENTS: usize = 64;
pub const MAX_WORKSPACE_EDIT_EDITS: usize = 4_096;
pub const MAX_WORKSPACE_EDIT_REPLACEMENT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_WORKSPACE_EDIT_STAGED_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORKSPACE_EDIT_TRANSACTIONS: usize = 16;
const WORKSPACE_EDIT_TRANSACTION_TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceEditTransactionPhase {
    Staged,
    Committed,
    RolledBack,
    RecoveryRequired,
    FinishedCommitted,
    FinishedRolledBack,
    FinishedUncommitted,
}

impl WorkspaceEditTransactionPhase {
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::FinishedCommitted | Self::FinishedRolledBack | Self::FinishedUncommitted
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceEditTransactionStatus {
    pub transaction_id: u64,
    pub phase: WorkspaceEditTransactionPhase,
    pub retry_rollback: bool,
    pub can_finalize: bool,
}

#[derive(Clone, Debug)]
pub struct WorkspaceEditRoot {
    pub workspace_id: WorkspaceId,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct WorkspaceEditOpenDocument {
    pub workspace_id: WorkspaceId,
    pub path: String,
    pub generation: u64,
    pub version: i64,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedWorkspaceEdit {
    pub transaction_id: u64,
    pub authorization: String,
    pub documents: Vec<StagedWorkspaceEditDocument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedWorkspaceEditDocument {
    pub workspace_id: WorkspaceId,
    pub path: String,
    pub original_text: String,
    pub new_text: String,
    pub generation: Option<u64>,
    pub version: Option<i64>,
}

#[derive(Debug, Default)]
pub struct WorkspaceEditTransactions {
    next_id: AtomicU64,
    transactions: Mutex<HashMap<u64, WorkspaceEditTransaction>>,
}

#[derive(Debug)]
struct WorkspaceEditTransaction {
    created_at: Instant,
    authorization: String,
    owner: Option<u64>,
    documents: Vec<TransactionDocument>,
    phase: WorkspaceEditTransactionPhase,
}

#[derive(Debug)]
struct TransactionDocument {
    workspace_id: WorkspaceId,
    path: String,
    original_text: String,
    new_text: String,
    original_hash: String,
    new_hash: String,
    open: Option<OpenDocumentIdentity>,
    closed: Option<ClosedDocument>,
}

#[derive(Debug)]
struct ClosedDocument {
    file: SecureEditFile,
    state: ClosedDocumentState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClosedDocumentState {
    Original(FileIdentity),
    Applied(FileIdentity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OpenDocumentIdentity {
    generation: u64,
    version: i64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WorkspaceEditError {
    Invalid(String),
    Unsupported(String),
    Stale(String),
    Limit(String),
    Expired,
    Io(String),
    Recovery(String),
}

impl WorkspaceEditTransactions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stage(
        &self,
        edit: &Value,
        roots: &[WorkspaceEditRoot],
        open_documents: &[WorkspaceEditOpenDocument],
    ) -> Result<StagedWorkspaceEdit, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        if transactions
            .values()
            .filter(|transaction| !transaction.phase.is_finished())
            .count()
            >= MAX_WORKSPACE_EDIT_TRANSACTIONS
        {
            return Err(WorkspaceEditError::Limit(
                "too many workspace edit transactions are active".to_owned(),
            ));
        }
        let documents = prepare_documents(edit, roots, open_documents)?;
        let id = allocate_transaction_id(&self.next_id, &transactions)?;
        let authorization = random_token().map_err(WorkspaceEditError::Io)?;
        let staged = StagedWorkspaceEdit {
            transaction_id: id,
            authorization: authorization.clone(),
            documents: documents
                .iter()
                .map(|document| StagedWorkspaceEditDocument {
                    workspace_id: document.workspace_id,
                    path: document.path.clone(),
                    original_text: document.original_text.clone(),
                    new_text: document.new_text.clone(),
                    generation: document.open.map(|open| open.generation),
                    version: document.open.map(|open| open.version),
                })
                .collect(),
        };
        transactions.insert(
            id,
            WorkspaceEditTransaction {
                created_at: Instant::now(),
                authorization,
                owner: None,
                documents,
                phase: WorkspaceEditTransactionPhase::Staged,
            },
        );
        Ok(staged)
    }

    pub fn commit_closed(
        &self,
        transaction_id: u64,
        authorization: &str,
        open_documents: &[WorkspaceEditOpenDocument],
    ) -> Result<(), WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get_mut(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        match transaction.phase {
            WorkspaceEditTransactionPhase::Committed
            | WorkspaceEditTransactionPhase::FinishedCommitted => return Ok(()),
            WorkspaceEditTransactionPhase::Staged => {}
            WorkspaceEditTransactionPhase::RecoveryRequired => {
                return Err(recovery_required(transaction_id));
            }
            _ => {
                return Err(WorkspaceEditError::Invalid(
                    "workspace edit transaction can no longer be committed".to_owned(),
                ));
            }
        }
        validate_open_documents(&transaction.documents, open_documents)?;
        validate_closed_hashes(&transaction.documents, false)?;

        for index in 0..transaction.documents.len() {
            if transaction.documents[index].closed.is_none() {
                continue;
            }
            if let Err(error) = apply_closed_document(&mut transaction.documents[index]) {
                return match rollback_applied(&mut transaction.documents) {
                    Ok(()) => Err(error),
                    Err(rollback) => {
                        transaction.phase = WorkspaceEditTransactionPhase::RecoveryRequired;
                        Err(WorkspaceEditError::Recovery(format!(
                            "{error}; closed-file rollback also failed: {rollback}; retry rollback or explicitly finalize transaction {transaction_id}"
                        )))
                    }
                };
            }
        }
        transaction.phase = WorkspaceEditTransactionPhase::Committed;
        Ok(())
    }

    pub fn rollback(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get_mut(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        match transaction.phase {
            WorkspaceEditTransactionPhase::RolledBack
            | WorkspaceEditTransactionPhase::FinishedRolledBack
            | WorkspaceEditTransactionPhase::FinishedUncommitted => return Ok(()),
            WorkspaceEditTransactionPhase::FinishedCommitted => {
                return Err(WorkspaceEditError::Invalid(
                    "a durably completed workspace edit cannot be rolled back".to_owned(),
                ));
            }
            _ => {}
        }
        rollback_transaction(transaction, transaction_id)?;
        validate_closed_hashes(&transaction.documents, false)?;
        transaction.phase = WorkspaceEditTransactionPhase::RolledBack;
        Ok(())
    }

    pub fn finish(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get_mut(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        transaction.phase = match transaction.phase {
            WorkspaceEditTransactionPhase::Staged => {
                WorkspaceEditTransactionPhase::FinishedUncommitted
            }
            WorkspaceEditTransactionPhase::Committed => {
                WorkspaceEditTransactionPhase::FinishedCommitted
            }
            WorkspaceEditTransactionPhase::RolledBack => {
                WorkspaceEditTransactionPhase::FinishedRolledBack
            }
            WorkspaceEditTransactionPhase::RecoveryRequired => {
                return Err(recovery_required(transaction_id));
            }
            finished => finished,
        };
        transaction.created_at = Instant::now();
        Ok(true)
    }

    pub fn finalize(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get_mut(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        transaction.phase = if transaction_has_applied_files(transaction) {
            WorkspaceEditTransactionPhase::FinishedCommitted
        } else {
            WorkspaceEditTransactionPhase::FinishedRolledBack
        };
        transaction.created_at = Instant::now();
        Ok(transaction_status(transaction_id, transaction))
    }

    pub fn status(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        Ok(transaction_status(transaction_id, transaction))
    }

    pub fn claim_owner(
        &self,
        transaction_id: u64,
        authorization: &str,
        owner: u64,
    ) -> Result<(), WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get_mut(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        match transaction.owner {
            Some(existing) if existing != owner && !transaction.phase.is_finished() => {
                Err(WorkspaceEditError::Invalid(
                    "workspace edit transaction belongs to another connection".to_owned(),
                ))
            }
            _ => {
                transaction.owner = Some(owner);
                Ok(())
            }
        }
    }

    pub fn cancel_owned(
        &self,
        transaction_id: u64,
        authorization: &str,
        owner: u64,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get_mut(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        if transaction.owner != Some(owner) {
            return Err(WorkspaceEditError::Invalid(
                "workspace edit cancellation came from a non-owning connection".to_owned(),
            ));
        }
        if transaction.phase == WorkspaceEditTransactionPhase::FinishedCommitted {
            return Ok(transaction_status(transaction_id, transaction));
        }
        if !transaction.phase.is_finished() {
            rollback_transaction(transaction, transaction_id)?;
            transaction.phase = WorkspaceEditTransactionPhase::FinishedRolledBack;
            transaction.created_at = Instant::now();
        }
        Ok(transaction_status(transaction_id, transaction))
    }

    pub fn disconnect_owner(&self, owner: u64) -> Vec<WorkspaceEditTransactionStatus> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let mut statuses = Vec::new();
        for (transaction_id, transaction) in transactions.iter_mut() {
            if transaction.owner != Some(owner) || transaction.phase.is_finished() {
                continue;
            }
            if rollback_transaction(transaction, *transaction_id).is_ok() {
                transaction.phase = WorkspaceEditTransactionPhase::FinishedRolledBack;
                transaction.created_at = Instant::now();
            }
            transaction.owner = None;
            statuses.push(transaction_status(*transaction_id, transaction));
        }
        statuses
    }
}

pub(super) fn validate_text_edits(
    text: &str,
    edits: &[LanguageServerTextEdit],
) -> Result<(), LanguageServerError> {
    validate_and_span_edits(text, edits)
        .map(|_| ())
        .map_err(|error| LanguageServerError::Protocol(error.to_string()))
}

fn prepare_documents(
    edit: &Value,
    roots: &[WorkspaceEditRoot],
    open_documents: &[WorkspaceEditOpenDocument],
) -> Result<Vec<TransactionDocument>, WorkspaceEditError> {
    if roots.is_empty() {
        return Err(WorkspaceEditError::Invalid(
            "workspace edit has no open workspace roots".to_owned(),
        ));
    }
    let object = edit.as_object().ok_or_else(|| {
        WorkspaceEditError::Invalid("workspace edit must be an object".to_owned())
    })?;
    if object.contains_key("changes") && object.contains_key("documentChanges") {
        return Err(WorkspaceEditError::Invalid(
            "workspace edit must not contain both changes and documentChanges".to_owned(),
        ));
    }

    let mut requested = Vec::new();
    if let Some(changes) = object.get("changes") {
        let changes = changes.as_object().ok_or_else(|| {
            WorkspaceEditError::Invalid("workspace edit changes must be an object".to_owned())
        })?;
        for (uri, edits) in changes {
            requested.push((uri.clone(), None, parse_edits(edits)?));
        }
    } else if let Some(changes) = object.get("documentChanges") {
        let changes = changes.as_array().ok_or_else(|| {
            WorkspaceEditError::Invalid(
                "workspace edit documentChanges must be an array".to_owned(),
            )
        })?;
        for change in changes {
            if change.get("kind").is_some() {
                return Err(WorkspaceEditError::Unsupported(
                    "workspace edit resource operations are not supported".to_owned(),
                ));
            }
            let document = change
                .get("textDocument")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    WorkspaceEditError::Unsupported(
                        "workspace edit documentChanges only supports text document edits"
                            .to_owned(),
                    )
                })?;
            let uri = document.get("uri").and_then(Value::as_str).ok_or_else(|| {
                WorkspaceEditError::Invalid("text document edit requires a URI".to_owned())
            })?;
            let version = match document.get("version") {
                None | Some(Value::Null) => None,
                Some(version) => Some(version.as_i64().ok_or_else(|| {
                    WorkspaceEditError::Invalid(
                        "text document edit version must be an integer or null".to_owned(),
                    )
                })?),
            };
            requested.push((
                uri.to_owned(),
                version,
                parse_edits(change.get("edits").unwrap_or(&Value::Null))?,
            ));
        }
    } else {
        return Ok(Vec::new());
    }

    if requested.len() > MAX_WORKSPACE_EDIT_DOCUMENTS {
        return Err(WorkspaceEditError::Limit(format!(
            "workspace edit exceeds the {MAX_WORKSPACE_EDIT_DOCUMENTS}-document limit"
        )));
    }
    let edit_count = requested
        .iter()
        .map(|(_, _, edits)| edits.len())
        .sum::<usize>();
    if edit_count > MAX_WORKSPACE_EDIT_EDITS {
        return Err(WorkspaceEditError::Limit(format!(
            "workspace edit exceeds the {MAX_WORKSPACE_EDIT_EDITS}-edit limit"
        )));
    }
    let replacement_bytes = requested
        .iter()
        .flat_map(|(_, _, edits)| edits)
        .try_fold(0_usize, |total, edit| {
            total.checked_add(edit.new_text.len())
        })
        .ok_or_else(|| {
            WorkspaceEditError::Limit("workspace edit replacement size overflowed".to_owned())
        })?;
    if replacement_bytes > MAX_WORKSPACE_EDIT_REPLACEMENT_BYTES {
        return Err(WorkspaceEditError::Limit(format!(
            "workspace edit exceeds the {MAX_WORKSPACE_EDIT_REPLACEMENT_BYTES}-byte replacement limit"
        )));
    }

    let open_by_path = open_documents
        .iter()
        .map(|document| ((document.workspace_id, document.path.as_str()), document))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut documents = Vec::with_capacity(requested.len());
    let mut staged_bytes = 0_usize;
    for (uri, requested_version, edits) in requested {
        let resolved = resolve_uri(&uri, roots)?;
        if !seen.insert((resolved.workspace_id, resolved.path.clone())) {
            return Err(WorkspaceEditError::Invalid(
                "workspace edit contains duplicate document edits".to_owned(),
            ));
        }
        let open = open_by_path
            .get(&(resolved.workspace_id, resolved.path.as_str()))
            .copied();
        if let (Some(version), Some(open)) = (requested_version, open)
            && version != open.version
        {
            return Err(WorkspaceEditError::Stale(format!(
                "open document {} has version {}, not {version}",
                resolved.path, open.version
            )));
        }
        let (original_text, closed) = match open {
            Some(open) => (open.text.clone(), None),
            None => {
                let (file, content) =
                    SecureEditFile::snapshot(&resolved.workspace_root, &resolved.path)
                        .map_err(WorkspaceEditError::Io)?;
                let identity = file.original_identity();
                (
                    content,
                    Some(ClosedDocument {
                        file,
                        state: ClosedDocumentState::Original(identity),
                    }),
                )
            }
        };
        if original_text.len() > MAX_EDITOR_FILE_BYTES {
            return Err(WorkspaceEditError::Limit(format!(
                "workspace edit input for {} exceeds the {MAX_EDITOR_FILE_BYTES}-byte limit",
                resolved.path
            )));
        }
        let spans = validate_and_span_edits(&original_text, &edits)?;
        let new_text = apply_edits(&original_text, &edits, spans);
        if new_text.len() > MAX_EDITOR_FILE_BYTES {
            return Err(WorkspaceEditError::Limit(format!(
                "workspace edit output for {} exceeds the {MAX_EDITOR_FILE_BYTES}-byte limit",
                resolved.path
            )));
        }
        staged_bytes = staged_bytes
            .checked_add(original_text.len())
            .and_then(|bytes| bytes.checked_add(new_text.len()))
            .ok_or_else(|| {
                WorkspaceEditError::Limit("workspace edit staged size overflowed".to_owned())
            })?;
        if staged_bytes > MAX_WORKSPACE_EDIT_STAGED_BYTES {
            return Err(WorkspaceEditError::Limit(format!(
                "workspace edit exceeds the {MAX_WORKSPACE_EDIT_STAGED_BYTES}-byte staged output limit"
            )));
        }
        documents.push(TransactionDocument {
            workspace_id: resolved.workspace_id,
            path: resolved.path,
            original_hash: content_hash(&original_text),
            new_hash: content_hash(&new_text),
            original_text,
            new_text,
            open: open.map(|open| OpenDocumentIdentity {
                generation: open.generation,
                version: open.version,
            }),
            closed,
        });
    }
    Ok(documents)
}

struct ResolvedDocument {
    workspace_id: WorkspaceId,
    workspace_root: PathBuf,
    path: String,
}

fn resolve_uri(
    uri: &str,
    roots: &[WorkspaceEditRoot],
) -> Result<ResolvedDocument, WorkspaceEditError> {
    let encoded_path = uri.strip_prefix("file://").ok_or_else(|| {
        WorkspaceEditError::Unsupported("workspace edit only supports file URIs".to_owned())
    })?;
    if !encoded_path.starts_with('/') || encoded_path.starts_with("//") {
        return Err(WorkspaceEditError::Unsupported(
            "workspace edit file URI authorities are not supported".to_owned(),
        ));
    }
    let decoded = percent_decode_str(encoded_path)
        .decode_utf8()
        .map_err(|_| {
            WorkspaceEditError::Invalid("workspace edit URI is not valid UTF-8".to_owned())
        })?;
    if decoded.contains('\0') {
        return Err(WorkspaceEditError::Invalid(
            "workspace edit URI contains a null byte".to_owned(),
        ));
    }
    let requested = PathBuf::from(decoded.as_ref());
    let metadata = fs::symlink_metadata(&requested).map_err(|error| {
        WorkspaceEditError::Io(format!(
            "could not inspect {}: {error}",
            requested.display()
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(WorkspaceEditError::Unsupported(
            "workspace edit final symlinks are not supported".to_owned(),
        ));
    }
    if !metadata.is_file() {
        return Err(WorkspaceEditError::Invalid(
            "workspace edit target must be a regular file".to_owned(),
        ));
    }
    let canonical =
        fs::canonicalize(&requested).map_err(|error| WorkspaceEditError::Io(error.to_string()))?;
    let mut matched = None;
    for root in roots {
        let canonical_root = fs::canonicalize(&root.path)
            .map_err(|error| WorkspaceEditError::Io(error.to_string()))?;
        let Ok(relative) = canonical.strip_prefix(&canonical_root) else {
            continue;
        };
        if matched
            .as_ref()
            .is_some_and(|(_, existing_len, _): &(WorkspaceId, usize, String)| {
                *existing_len >= canonical_root.as_os_str().len()
            })
        {
            continue;
        }
        let relative = relative.to_str().ok_or_else(|| {
            WorkspaceEditError::Invalid("workspace edit path is not valid UTF-8".to_owned())
        })?;
        let location = EditorLocation::resolve(&canonical_root, relative)
            .map_err(|error| WorkspaceEditError::Invalid(error.to_string()))?;
        matched = Some((
            root.workspace_id,
            canonical_root.as_os_str().len(),
            location.relative_path().to_owned(),
        ));
    }
    let (workspace_id, _, path) = matched.ok_or_else(|| {
        WorkspaceEditError::Unsupported("workspace edit path is outside the workspace".to_owned())
    })?;
    let workspace_root = fs::canonicalize(
        roots
            .iter()
            .find(|root| root.workspace_id == workspace_id)
            .map(|root| root.path.as_path())
            .ok_or_else(|| WorkspaceEditError::Invalid("workspace root disappeared".to_owned()))?,
    )
    .map_err(|error| WorkspaceEditError::Io(error.to_string()))?;
    Ok(ResolvedDocument {
        workspace_id,
        workspace_root,
        path,
    })
}

fn parse_edits(value: &Value) -> Result<Vec<LanguageServerTextEdit>, WorkspaceEditError> {
    let edits = value.as_array().ok_or_else(|| {
        WorkspaceEditError::Invalid("workspace edit document edits must be an array".to_owned())
    })?;
    edits
        .iter()
        .map(|edit| {
            let range = edit.get("range").ok_or_else(|| {
                WorkspaceEditError::Invalid("workspace text edit requires a range".to_owned())
            })?;
            let new_text = edit.get("newText").and_then(Value::as_str).ok_or_else(|| {
                WorkspaceEditError::Invalid("workspace text edit requires newText".to_owned())
            })?;
            Ok(LanguageServerTextEdit {
                range: parse_range(range)?,
                new_text: new_text.to_owned(),
            })
        })
        .collect()
}

fn parse_range(value: &Value) -> Result<LanguageServerRange, WorkspaceEditError> {
    Ok(LanguageServerRange {
        start: parse_position(value.get("start"))?,
        end: parse_position(value.get("end"))?,
    })
}

fn parse_position(value: Option<&Value>) -> Result<LanguageServerPosition, WorkspaceEditError> {
    let value = value.and_then(Value::as_object).ok_or_else(|| {
        WorkspaceEditError::Invalid("workspace text edit position must be an object".to_owned())
    })?;
    let line = value
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let character = value
        .get("character")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    match (line, character) {
        (Some(line), Some(character)) => Ok(LanguageServerPosition { line, character }),
        _ => Err(WorkspaceEditError::Invalid(
            "workspace text edit position is invalid".to_owned(),
        )),
    }
}

fn validate_and_span_edits(
    text: &str,
    edits: &[LanguageServerTextEdit],
) -> Result<Vec<(usize, usize, usize)>, WorkspaceEditError> {
    let mut spans = edits
        .iter()
        .enumerate()
        .map(|(index, edit)| {
            let start = position_offset(text, edit.range.start)?;
            let end = position_offset(text, edit.range.end)?;
            if start > end {
                return Err(WorkspaceEditError::Invalid(
                    "workspace text edit range is reversed".to_owned(),
                ));
            }
            Ok((start, end, index))
        })
        .collect::<Result<Vec<_>, _>>()?;
    spans.sort_unstable_by_key(|(start, end, index)| (*start, *end, *index));
    for pair in spans.windows(2) {
        let (start, end, _) = pair[0];
        let (next_start, next_end, _) = pair[1];
        if next_start < end || (start == next_start && (start == end || next_start == next_end)) {
            return Err(WorkspaceEditError::Invalid(
                "workspace text edits overlap or conflict".to_owned(),
            ));
        }
    }
    Ok(spans)
}

fn position_offset(
    text: &str,
    position: LanguageServerPosition,
) -> Result<usize, WorkspaceEditError> {
    let target_line = usize::try_from(position.line).map_err(|_| {
        WorkspaceEditError::Invalid("workspace text edit line is invalid".to_owned())
    })?;
    let mut line_start = 0;
    for _ in 0..target_line {
        let next = text[line_start..].find('\n').ok_or_else(|| {
            WorkspaceEditError::Invalid(
                "workspace text edit line is outside the document".to_owned(),
            )
        })?;
        line_start += next + 1;
    }
    let mut line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |offset| line_start + offset);
    if text.as_bytes().get(line_end.saturating_sub(1)) == Some(&b'\r') {
        line_end -= 1;
    }
    let target = usize::try_from(position.character).map_err(|_| {
        WorkspaceEditError::Invalid("workspace text edit character is invalid".to_owned())
    })?;
    let content = &text[line_start..line_end];
    let mut utf16 = 0;
    for (offset, character) in content.char_indices() {
        if utf16 == target {
            return Ok(line_start + offset);
        }
        utf16 += character.len_utf16();
        if utf16 > target {
            return Err(WorkspaceEditError::Invalid(
                "workspace text edit splits a UTF-16 surrogate pair".to_owned(),
            ));
        }
    }
    if utf16 == target {
        Ok(line_end)
    } else {
        Err(WorkspaceEditError::Invalid(
            "workspace text edit character is outside the line".to_owned(),
        ))
    }
}

fn apply_edits(
    text: &str,
    edits: &[LanguageServerTextEdit],
    mut spans: Vec<(usize, usize, usize)>,
) -> String {
    spans.sort_unstable_by(|left, right| right.0.cmp(&left.0).then_with(|| right.2.cmp(&left.2)));
    let mut result = text.to_owned();
    for (start, end, index) in spans {
        result.replace_range(start..end, &edits[index].new_text);
    }
    result
}

fn validate_open_documents(
    documents: &[TransactionDocument],
    open_documents: &[WorkspaceEditOpenDocument],
) -> Result<(), WorkspaceEditError> {
    for document in documents {
        let Some(expected) = document.open else {
            continue;
        };
        let current = open_documents
            .iter()
            .find(|open| open.workspace_id == document.workspace_id && open.path == document.path);
        if !current.is_some_and(|current| {
            current.generation == expected.generation
                && current.version == expected.version
                && content_hash(&current.text) == document.original_hash
        }) {
            return Err(WorkspaceEditError::Stale(format!(
                "open document {} changed while the workspace edit was staged",
                document.path
            )));
        }
    }
    Ok(())
}

fn validate_closed_hashes(
    documents: &[TransactionDocument],
    expect_new: bool,
) -> Result<(), WorkspaceEditError> {
    for document in documents
        .iter()
        .filter(|document| document.closed.is_some())
    {
        let closed = document
            .closed
            .as_ref()
            .expect("closed document should exist");
        let (identity, expected) = match (expect_new, closed.state) {
            (true, ClosedDocumentState::Applied(identity)) => (identity, &document.new_hash),
            (false, ClosedDocumentState::Original(identity)) => (identity, &document.original_hash),
            _ => {
                return Err(WorkspaceEditError::Stale(format!(
                    "closed file {} is not in the expected transaction state",
                    document.path
                )));
            }
        };
        closed.file.validate(identity, expected).map_err(|error| {
            WorkspaceEditError::Stale(format!("closed file {} changed: {error}", document.path))
        })?;
    }
    Ok(())
}

fn apply_closed_document(document: &mut TransactionDocument) -> Result<(), WorkspaceEditError> {
    let closed = document.closed.as_mut().ok_or_else(|| {
        WorkspaceEditError::Invalid("workspace edit target is not a closed file".to_owned())
    })?;
    let ClosedDocumentState::Original(identity) = closed.state else {
        return Err(WorkspaceEditError::Invalid(
            "workspace edit target was already applied".to_owned(),
        ));
    };
    match closed
        .file
        .replace(identity, &document.original_hash, &document.new_text)
    {
        Ok(applied) => {
            closed.state = ClosedDocumentState::Applied(applied);
            Ok(())
        }
        Err(SecureReplaceError::Safe(error)) => Err(WorkspaceEditError::Stale(format!(
            "{}: {error}",
            document.path
        ))),
        Err(SecureReplaceError::InstalledChanged {
            message,
            installed_identity,
        }) => {
            closed.state = ClosedDocumentState::Applied(installed_identity);
            Err(WorkspaceEditError::Recovery(format!(
                "{}: {message}",
                document.path
            )))
        }
    }
}

fn rollback_applied(documents: &mut [TransactionDocument]) -> Result<(), WorkspaceEditError> {
    let mut errors = Vec::new();
    for document in documents.iter_mut().rev() {
        let Some(closed) = document.closed.as_mut() else {
            continue;
        };
        let ClosedDocumentState::Applied(identity) = closed.state else {
            continue;
        };
        match closed
            .file
            .replace(identity, &document.new_hash, &document.original_text)
        {
            Ok(original) => closed.state = ClosedDocumentState::Original(original),
            Err(SecureReplaceError::Safe(error)) => {
                errors.push(format!("{}: {error}", document.path));
            }
            Err(SecureReplaceError::InstalledChanged {
                message,
                installed_identity,
            }) => {
                closed.state = ClosedDocumentState::Original(installed_identity);
                errors.push(format!("{}: {message}", document.path));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(WorkspaceEditError::Io(errors.join("; ")))
    }
}

fn rollback_transaction(
    transaction: &mut WorkspaceEditTransaction,
    transaction_id: u64,
) -> Result<(), WorkspaceEditError> {
    rollback_applied(&mut transaction.documents).map_err(|error| {
        transaction.phase = WorkspaceEditTransactionPhase::RecoveryRequired;
        WorkspaceEditError::Recovery(format!(
            "closed-file rollback is incomplete: {error}; retry rollback or explicitly finalize transaction {transaction_id}"
        ))
    })
}

fn recovery_required(transaction_id: u64) -> WorkspaceEditError {
    WorkspaceEditError::Recovery(format!(
        "transaction {transaction_id} has incomplete filesystem recovery; retry rollback or explicitly finalize it"
    ))
}

fn transaction_status(
    transaction_id: u64,
    transaction: &WorkspaceEditTransaction,
) -> WorkspaceEditTransactionStatus {
    WorkspaceEditTransactionStatus {
        transaction_id,
        phase: transaction.phase,
        retry_rollback: transaction.phase == WorkspaceEditTransactionPhase::RecoveryRequired,
        can_finalize: transaction.phase == WorkspaceEditTransactionPhase::RecoveryRequired,
    }
}

fn content_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn remove_expired(transactions: &mut HashMap<u64, WorkspaceEditTransaction>) {
    transactions.retain(|_, transaction| {
        (!transaction.phase.is_finished()
            && (transaction.phase != WorkspaceEditTransactionPhase::Staged
                || transaction.created_at.elapsed() <= WORKSPACE_EDIT_TRANSACTION_TTL))
            || (transaction.phase.is_finished()
                && transaction.created_at.elapsed() <= WORKSPACE_EDIT_TRANSACTION_TTL)
    });
}

fn transaction_has_applied_files(transaction: &WorkspaceEditTransaction) -> bool {
    transaction.documents.iter().any(|document| {
        document
            .closed
            .as_ref()
            .is_some_and(|closed| matches!(closed.state, ClosedDocumentState::Applied(_)))
    })
}

fn validate_authorization(
    transaction: &WorkspaceEditTransaction,
    authorization: &str,
) -> Result<(), WorkspaceEditError> {
    if transaction.authorization == authorization {
        Ok(())
    } else {
        Err(WorkspaceEditError::Invalid(
            "workspace edit transaction authorization is invalid".to_owned(),
        ))
    }
}

fn allocate_transaction_id(
    next_id: &AtomicU64,
    transactions: &HashMap<u64, WorkspaceEditTransaction>,
) -> Result<u64, WorkspaceEditError> {
    for _ in 0..=transactions.len() {
        let id = next_id.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        if id != 0 && !transactions.contains_key(&id) {
            return Ok(id);
        }
    }
    Err(WorkspaceEditError::Limit(
        "workspace edit transaction ID space is exhausted".to_owned(),
    ))
}

impl fmt::Display for WorkspaceEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid workspace edit: {message}"),
            Self::Unsupported(message) => formatter.write_str(message),
            Self::Stale(message) => formatter.write_str(message),
            Self::Limit(message) => formatter.write_str(message),
            Self::Expired => {
                formatter.write_str("workspace edit transaction expired or does not exist")
            }
            Self::Io(message) => write!(
                formatter,
                "workspace edit filesystem operation failed: {message}"
            ),
            Self::Recovery(message) => {
                write!(formatter, "workspace edit recovery required: {message}")
            }
        }
    }
}

impl std::error::Error for WorkspaceEditError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::AtomicU64;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    fn edit(start: (u32, u32), end: (u32, u32)) -> LanguageServerTextEdit {
        LanguageServerTextEdit {
            range: LanguageServerRange {
                start: LanguageServerPosition {
                    line: start.0,
                    character: start.1,
                },
                end: LanguageServerPosition {
                    line: end.0,
                    character: end.1,
                },
            },
            new_text: String::new(),
        }
    }

    #[test]
    fn validates_utf16_reversed_and_overlapping_ranges() {
        assert!(validate_text_edits("a😀b\r\n", &[edit((0, 1), (0, 3))]).is_ok());
        assert!(validate_text_edits("a😀b\r\n", &[edit((0, 2), (0, 3))]).is_err());
        assert!(validate_text_edits("text", &[edit((0, 3), (0, 1))]).is_err());
        assert!(
            validate_text_edits("text", &[edit((0, 0), (0, 3)), edit((0, 2), (0, 4))]).is_err()
        );
        assert!(
            validate_text_edits("text", &[edit((0, 2), (0, 2)), edit((0, 2), (0, 2))]).is_err()
        );
    }

    #[test]
    fn stages_changes_and_document_changes_and_rejects_duplicates_and_resources() {
        let root = test_directory("forms");
        let path = root.join("a file.txt");
        fs::write(&path, "hello").unwrap();
        let roots = roots(&root);
        let uri = file_uri(&path);
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({
                    "changes": { uri.clone(): [{ "range": range(0, 5), "newText": "world" }] }
                }),
                &roots,
                &[],
            )
            .unwrap();
        assert_eq!(staged.documents[0].new_text, "world");
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();

        assert!(manager.stage(&serde_json::json!({
            "documentChanges": [
                { "textDocument": { "uri": uri, "version": null }, "edits": [] },
                { "textDocument": { "uri": file_uri(&path), "version": null }, "edits": [] }
            ]
        }), &roots, &[]).is_err());
        assert!(matches!(
            manager.stage(
                &serde_json::json!({
                    "documentChanges": [{ "kind": "create", "uri": file_uri(&root.join("new")) }]
                }),
                &roots,
                &[]
            ),
            Err(WorkspaceEditError::Unsupported(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsupported_outside_and_final_symlink_uris() {
        let root = test_directory("paths");
        let outside = test_directory("outside");
        fs::write(root.join("inside.txt"), "in").unwrap();
        fs::write(outside.join("outside.txt"), "out").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let roots = roots(&root);
        assert!(matches!(
            manager.stage(
                &serde_json::json!({ "changes": { "https://example/a": [] } }),
                &roots,
                &[]
            ),
            Err(WorkspaceEditError::Unsupported(_))
        ));
        assert!(matches!(
            manager.stage(
                &serde_json::json!({ "changes": { file_uri(&outside.join("outside.txt")): [] } }),
                &roots,
                &[]
            ),
            Err(WorkspaceEditError::Unsupported(_))
        ));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("inside.txt"), root.join("link.txt")).unwrap();
            assert!(matches!(
                manager.stage(
                    &serde_json::json!({ "changes": { file_uri(&root.join("link.txt")): [] } }),
                    &roots,
                    &[]
                ),
                Err(WorkspaceEditError::Unsupported(_))
            ));
        }
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn checks_open_versions_and_closed_hashes_then_rolls_back() {
        let root = test_directory("commit");
        let open_path = root.join("open.txt");
        let closed_path = root.join("closed.txt");
        fs::write(&open_path, "open").unwrap();
        fs::write(&closed_path, "closed").unwrap();
        let roots = roots(&root);
        let open = vec![WorkspaceEditOpenDocument {
            workspace_id: WorkspaceId::new(1),
            path: "open.txt".to_owned(),
            generation: 7,
            version: 3,
            text: "dirty open".to_owned(),
        }];
        let manager = WorkspaceEditTransactions::new();
        assert!(matches!(manager.stage(&serde_json::json!({
            "documentChanges": [{ "textDocument": { "uri": file_uri(&open_path), "version": 2 }, "edits": [] }]
        }), &roots, &open), Err(WorkspaceEditError::Stale(_))));
        let open_staged = manager
            .stage(
                &serde_json::json!({
                    "documentChanges": [{
                        "textDocument": { "uri": file_uri(&open_path), "version": 3 },
                        "edits": [{ "range": range(0, 4), "newText": "updated" }]
                    }]
                }),
                &roots,
                &open,
            )
            .unwrap();
        let mut raced_open = open.clone();
        raced_open[0].version = 4;
        raced_open[0].text = "newer".to_owned();
        assert!(matches!(
            manager.commit_closed(
                open_staged.transaction_id,
                &open_staged.authorization,
                &raced_open,
            ),
            Err(WorkspaceEditError::Stale(_))
        ));
        manager
            .finish(open_staged.transaction_id, &open_staged.authorization)
            .unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&closed_path): [{ "range": range(0, 6), "newText": "changed" }]
                }}),
                &roots,
                &open,
            )
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &open)
            .unwrap();
        assert_eq!(fs::read_to_string(&closed_path).unwrap(), "changed");
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&closed_path).unwrap(), "closed");
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();

        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&closed_path): [{ "range": range(0, 6), "newText": "changed" }]
                }}),
                &roots,
                &open,
            )
            .unwrap();
        fs::write(&closed_path, "raced").unwrap();
        assert!(matches!(
            manager.commit_closed(staged.transaction_id, &staged.authorization, &open),
            Err(WorkspaceEditError::Stale(_))
        ));
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn enforces_document_edit_replacement_and_output_limits() {
        let root = test_directory("limits");
        fs::write(root.join("file.txt"), "x").unwrap();
        let roots = roots(&root);
        let manager = WorkspaceEditTransactions::new();
        let too_many = (0..=MAX_WORKSPACE_EDIT_EDITS)
            .map(|index| serde_json::json!({ "range": range(0, 0), "newText": index.to_string() }))
            .collect::<Vec<_>>();
        assert!(matches!(
            manager.stage(
                &serde_json::json!({ "changes": { file_uri(&root.join("file.txt")): too_many } }),
                &roots,
                &[]
            ),
            Err(WorkspaceEditError::Limit(_))
        ));
        assert!(matches!(manager.stage(&serde_json::json!({ "changes": { file_uri(&root.join("file.txt")): [{ "range": range(0, 1), "newText": "x".repeat(MAX_EDITOR_FILE_BYTES + 1) }] } }), &roots, &[]), Err(WorkspaceEditError::Limit(_))));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ancestor_symlink_replacement_cannot_escape_commit_or_rollback() {
        use std::os::unix::fs::symlink;

        let root = test_directory("ancestor-race");
        let outside = test_directory("ancestor-race-outside");
        fs::create_dir(root.join("source")).unwrap();
        fs::write(root.join("source/file.txt"), "inside").unwrap();
        fs::write(outside.join("file.txt"), "outside").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let roots = roots(&root);
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&root.join("source/file.txt")): [{ "range": range(0, 6), "newText": "edited" }]
                }}),
                &roots,
                &[],
            )
            .unwrap();
        fs::rename(root.join("source"), root.join("source-held")).unwrap();
        symlink(&outside, root.join("source")).unwrap();
        assert!(
            manager
                .commit_closed(staged.transaction_id, &staged.authorization, &[])
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(outside.join("file.txt")).unwrap(),
            "outside"
        );
        fs::remove_file(root.join("source")).unwrap();
        fs::rename(root.join("source-held"), root.join("source")).unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();

        fs::rename(root.join("source"), root.join("source-held")).unwrap();
        symlink(&outside, root.join("source")).unwrap();
        assert!(matches!(
            manager.rollback(staged.transaction_id, &staged.authorization),
            Err(WorkspaceEditError::Recovery(_))
        ));
        assert_eq!(
            fs::read_to_string(outside.join("file.txt")).unwrap(),
            "outside"
        );
        fs::remove_file(root.join("source")).unwrap();
        fs::rename(root.join("source-held"), root.join("source")).unwrap();
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("source/file.txt")).unwrap(),
            "inside"
        );
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn committed_recovery_material_does_not_expire_and_requires_authorization() {
        let root = test_directory("recovery-retention");
        let path = root.join("file.txt");
        fs::write(&path, "before").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&path): [{ "range": range(0, 6), "newText": "after" }]
                }}),
                &roots(&root),
                &[],
            )
            .unwrap();
        assert!(matches!(
            manager.commit_closed(staged.transaction_id, "wrong", &[]),
            Err(WorkspaceEditError::Invalid(_))
        ));
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        {
            let mut transactions = manager.transactions.lock().unwrap();
            transactions
                .get_mut(&staged.transaction_id)
                .unwrap()
                .created_at =
                Instant::now() - WORKSPACE_EDIT_TRANSACTION_TTL - Duration::from_secs(1);
            remove_expired(&mut transactions);
            assert!(transactions.contains_key(&staged.transaction_id));
        }
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn transaction_responses_are_idempotent_and_finished_tombstones_do_not_use_capacity() {
        let root = test_directory("idempotent");
        let path = root.join("file.txt");
        fs::write(&path, "before").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&path): [{ "range": range(0, 6), "newText": "after" }]
                }}),
                &roots(&root),
                &[],
            )
            .unwrap();

        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert_eq!(
            manager
                .status(staged.transaction_id, &staged.authorization)
                .unwrap()
                .phase,
            WorkspaceEditTransactionPhase::Committed
        );
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(
            manager
                .status(staged.transaction_id, &staged.authorization)
                .unwrap()
                .phase,
            WorkspaceEditTransactionPhase::FinishedCommitted
        );

        let mut active = Vec::new();
        for _ in 0..MAX_WORKSPACE_EDIT_TRANSACTIONS {
            active.push(
                manager
                    .stage(&serde_json::json!({ "changes": {} }), &roots(&root), &[])
                    .unwrap(),
            );
        }
        assert!(matches!(
            manager.stage(&serde_json::json!({ "changes": {} }), &roots(&root), &[]),
            Err(WorkspaceEditError::Limit(_))
        ));
        for transaction in active {
            manager
                .finish(transaction.transaction_id, &transaction.authorization)
                .unwrap();
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn owner_disconnect_rolls_back_unacknowledged_closed_files() {
        let root = test_directory("disconnect-rollback");
        let path = root.join("file.txt");
        fs::write(&path, "before").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&path): [{ "range": range(0, 6), "newText": "after" }]
                }}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .claim_owner(staged.transaction_id, &staged.authorization, 42)
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "after");

        let statuses = manager.disconnect_owner(42);
        assert_eq!(
            statuses[0].phase,
            WorkspaceEditTransactionPhase::FinishedRolledBack
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "before");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn durable_finish_and_cancellation_have_one_commit_point() {
        let root = test_directory("commit-point");
        let path = root.join("file.txt");
        fs::write(&path, "before").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let make_edit = || {
            manager
                .stage(
                    &serde_json::json!({ "changes": {
                        file_uri(&path): [{ "range": range(0, 6), "newText": "after" }]
                    }}),
                    &roots(&root),
                    &[],
                )
                .unwrap()
        };

        let completed = make_edit();
        manager
            .claim_owner(completed.transaction_id, &completed.authorization, 7)
            .unwrap();
        manager
            .commit_closed(completed.transaction_id, &completed.authorization, &[])
            .unwrap();
        manager
            .finish(completed.transaction_id, &completed.authorization)
            .unwrap();
        assert_eq!(
            manager
                .cancel_owned(completed.transaction_id, &completed.authorization, 7)
                .unwrap()
                .phase,
            WorkspaceEditTransactionPhase::FinishedCommitted
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "after");

        fs::write(&path, "before").unwrap();
        let cancelled = make_edit();
        manager
            .claim_owner(cancelled.transaction_id, &cancelled.authorization, 8)
            .unwrap();
        manager
            .commit_closed(cancelled.transaction_id, &cancelled.authorization, &[])
            .unwrap();
        assert_eq!(
            manager
                .cancel_owned(cancelled.transaction_id, &cancelled.authorization, 8)
                .unwrap()
                .phase,
            WorkspaceEditTransactionPhase::FinishedRolledBack
        );
        manager
            .finish(cancelled.transaction_id, &cancelled.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "before");
        let _ = fs::remove_dir_all(root);
    }

    fn range(start: u32, end: u32) -> Value {
        serde_json::json!({ "start": { "line": 0, "character": start }, "end": { "line": 0, "character": end } })
    }

    fn roots(root: &Path) -> Vec<WorkspaceEditRoot> {
        vec![WorkspaceEditRoot {
            workspace_id: WorkspaceId::new(1),
            path: root.to_path_buf(),
        }]
    }

    fn file_uri(path: &Path) -> String {
        format!("file://{}", path.to_string_lossy().replace(' ', "%20"))
    }

    fn test_directory(name: &str) -> PathBuf {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "kosmos-workspace-edits-{}-{name}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}

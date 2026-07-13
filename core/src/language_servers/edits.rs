use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::cell::Cell;

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::tabs::editor::MAX_EDITOR_FILE_BYTES;
use crate::tree::WorkspaceId;

use super::secure_edit::{
    FileIdentity, SecureMutationOutcome, SecurePathKind, SecurePathSnapshot, SecureTombstone,
    SecureWorkspace, prepared_file_name, random_token,
};
use super::{
    LanguageServerError, LanguageServerPosition, LanguageServerRange, LanguageServerTextEdit,
};

pub const MAX_WORKSPACE_EDIT_DOCUMENTS: usize = 64;
pub const MAX_WORKSPACE_EDIT_EDITS: usize = 4_096;
pub const MAX_WORKSPACE_EDIT_REPLACEMENT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_WORKSPACE_EDIT_STAGED_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORKSPACE_EDIT_TRANSACTIONS: usize = 16;
const MAX_WORKSPACE_EDIT_RECOVERIES: usize = MAX_WORKSPACE_EDIT_OUTCOMES;
const WORKSPACE_EDIT_TRANSACTION_TTL: Duration = Duration::from_secs(30);
const MAX_WORKSPACE_EDIT_OUTCOMES: usize = 64;
const WORKSPACE_EDIT_OUTCOME_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[cfg(test)]
thread_local! {
    static FAIL_NEXT_RESOURCE_REFRESH: Cell<bool> = const { Cell::new(false) };
    static NEXT_CLEANUP_FAULT: Cell<Option<CleanupFault>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanupFault {
    AfterMarkDiscarding,
    BeforeMarkDiscarded,
    AfterMarkDiscarded,
}

#[cfg(test)]
fn fail_next_cleanup_at(fault: CleanupFault) {
    NEXT_CLEANUP_FAULT.set(Some(fault));
}

fn inject_cleanup_fault(fault: CleanupFault) -> Result<(), WorkspaceEditError> {
    #[cfg(test)]
    if NEXT_CLEANUP_FAULT.get() == Some(fault) {
        NEXT_CLEANUP_FAULT.set(None);
        return Err(WorkspaceEditError::Io(format!(
            "injected {fault:?} cleanup failure"
        )));
    }
    let _ = fault;
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEditTransactionPhase {
    Staged,
    Committed,
    FinishingCommitted,
    CommittedCleanupRequired,
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

    fn is_commit_decided(self) -> bool {
        matches!(
            self,
            Self::FinishingCommitted | Self::CommittedCleanupRequired | Self::FinishedCommitted
        )
    }

    fn is_recovery(self) -> bool {
        matches!(
            self,
            Self::RecoveryRequired | Self::CommittedCleanupRequired
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceEditTransactionStatus {
    pub transaction_id: u64,
    pub phase: WorkspaceEditTransactionPhase,
    pub retry_rollback: bool,
    pub can_finalize: bool,
    pub requires_acknowledgement: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEditRecovery {
    pub transaction_id: u64,
    pub authorization: String,
    pub status: WorkspaceEditTransactionStatus,
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
    /// The last saved content observed by the adapter when the edit was staged.
    pub saved_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedWorkspaceEdit {
    pub transaction_id: u64,
    pub authorization: String,
    pub documents: Vec<StagedWorkspaceEditDocument>,
    pub operations: Vec<StagedWorkspaceEditOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StagedWorkspaceEditOperation {
    TextDocument {
        document: usize,
    },
    CreateFile {
        workspace_id: WorkspaceId,
        path: String,
    },
    RenameFile {
        workspace_id: WorkspaceId,
        old_path: String,
        new_path: String,
    },
    DeleteFile {
        workspace_id: WorkspaceId,
        path: String,
        recursive: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedWorkspaceEditDocument {
    pub workspace_id: WorkspaceId,
    pub path: String,
    pub original_path: String,
    pub original_text: String,
    pub new_text: String,
    pub generation: Option<u64>,
    pub version: Option<i64>,
}

/// A complete Monaco mutation selected by core from a point-in-time document observation.
///
/// This is intentionally data-only. Adapters own their model objects, locks, and view state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEditModelDirective {
    pub workspace_id: WorkspaceId,
    pub original_path: String,
    pub path: Option<String>,
    pub generation: u64,
    pub version: i64,
    pub original_text: String,
    pub text: String,
}

#[derive(Debug)]
pub struct WorkspaceEditTransactions {
    next_id: AtomicU64,
    transactions: Mutex<HashMap<u64, WorkspaceEditTransaction>>,
    outcomes: Mutex<HashMap<u64, WorkspaceEditOutcome>>,
    acknowledgements: Mutex<HashMap<u64, WorkspaceEditAcknowledgement>>,
    recovery_authorizations: Mutex<HashMap<u64, String>>,
    store: Option<crate::persistence::StateStore>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceEditOutcome {
    authorization_hash: String,
    #[serde(default)]
    authorization_expires_at: Option<u64>,
    phase: WorkspaceEditTransactionPhase,
    created_at: u64,
    operations: Vec<WorkspaceEditOutcomeOperation>,
}

#[derive(Clone, Debug)]
struct WorkspaceEditAcknowledgement {
    authorization_hash: String,
    created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum WorkspaceEditOutcomeOperation {
    TextDocument {
        document: usize,
    },
    CreateFile {
        workspace_id: u64,
        path: String,
    },
    RenameFile {
        workspace_id: u64,
        old_path: String,
        new_path: String,
    },
    DeleteFile {
        workspace_id: u64,
        path: String,
        recursive: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceEditWatchDisposition {
    Active,
    Committed,
    RolledBack,
    Unrelated,
}

#[derive(Debug)]
struct WorkspaceEditTransaction {
    created_at: Instant,
    authorization: String,
    documents: Vec<TransactionDocument>,
    operations: Vec<TransactionOperation>,
    staged: StagedWorkspaceEdit,
    model_directives: Vec<WorkspaceEditModelDirective>,
    phase: WorkspaceEditTransactionPhase,
}

#[derive(Debug)]
struct TransactionDocument {
    workspace_id: WorkspaceId,
    path: String,
    original_text: String,
    new_text: String,
    original_hash: String,
    open: Option<OpenDocumentIdentity>,
    open_path: Option<String>,
    closed: Option<ClosedDocument>,
}

#[derive(Debug)]
struct ClosedDocument {
    workspace: Arc<SecureWorkspace>,
    workspace_root: PathBuf,
    original: Option<SecurePathSnapshot>,
    mode: libc::mode_t,
    state: ClosedDocumentState,
    tombstone: Option<SecureTombstone>,
    prepared_name: Option<String>,
    prepared: Option<FileIdentity>,
    operation_phase: JournalOperationPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClosedDocumentState {
    Pending,
    Original(FileIdentity),
    Applied(FileIdentity),
}

#[derive(Debug)]
enum TransactionOperation {
    TextDocument(usize),
    Resource(Box<ResourceOperation>),
}

#[derive(Debug)]
struct ResourceOperation {
    workspace_id: WorkspaceId,
    workspace: Arc<SecureWorkspace>,
    workspace_root: PathBuf,
    operation_index: usize,
    phase: JournalOperationPhase,
    kind: ResourceOperationKind,
}

#[derive(Debug)]
enum ResourceOperationKind {
    Create {
        path: String,
        previous: Option<SecurePathSnapshot>,
        tombstone: Option<SecureTombstone>,
        prepared_name: Option<String>,
        prepared: Option<FileIdentity>,
        created: Option<FileIdentity>,
    },
    Rename {
        old_path: String,
        new_path: String,
        source: SecurePathSnapshot,
        previous: Option<SecurePathSnapshot>,
        tombstone: Option<SecureTombstone>,
        moved: bool,
    },
    Delete {
        path: String,
        snapshot: SecurePathSnapshot,
        recursive: bool,
        tombstone: Option<SecureTombstone>,
    },
}

struct PreparedTransaction {
    documents: Vec<TransactionDocument>,
    operations: Vec<TransactionOperation>,
    staged_operations: Vec<StagedWorkspaceEditOperation>,
}

enum RequestedChange {
    Text {
        uri: String,
        version: Option<i64>,
        edits: Vec<LanguageServerTextEdit>,
    },
    Create {
        uri: String,
        overwrite: bool,
        ignore_if_exists: bool,
    },
    Rename {
        old_uri: String,
        new_uri: String,
        overwrite: bool,
        ignore_if_exists: bool,
    },
    Delete {
        uri: String,
        recursive: bool,
        ignore_if_not_exists: bool,
    },
}

enum VirtualEvent {
    Create(String),
    Rename(String, String),
    Delete(String),
}

enum VirtualPath {
    Physical(String),
    Created,
    Missing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum JournalPhase {
    Applying,
    Applied,
    RecoveryRequired,
    RollingBack,
    RolledBack,
    FinishingCommitted,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum JournalOperationPhase {
    #[default]
    NotStarted,
    Applying,
    Applied,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum JournalCleanupPhase {
    #[default]
    Pending,
    Discarding,
    Discarded,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum JournalRollbackPhase {
    #[default]
    Pending,
    RemovingApplied,
    AppliedRemoved,
    RestoringBackup,
    Restored,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceEditJournal {
    authorization: String,
    phase: JournalPhase,
    operations: Vec<JournalOperation>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum JournalOperation {
    Text {
        #[serde(default)]
        workspace_id: u64,
        #[serde(default)]
        document: usize,
        root: PathBuf,
        path: String,
        original: Option<SecurePathSnapshot>,
        #[serde(default)]
        prepared_name: Option<String>,
        prepared: Option<FileIdentity>,
        installed: Option<FileIdentity>,
        installed_hash: String,
        #[serde(default)]
        phase: JournalOperationPhase,
        #[serde(default)]
        cleanup: JournalCleanupPhase,
        #[serde(default)]
        rollback: JournalRollbackPhase,
    },
    Create {
        #[serde(default)]
        workspace_id: u64,
        root: PathBuf,
        path: String,
        previous: Option<SecurePathSnapshot>,
        #[serde(default)]
        prepared_name: Option<String>,
        prepared: Option<FileIdentity>,
        installed: Option<FileIdentity>,
        installed_hash: String,
        #[serde(default)]
        phase: JournalOperationPhase,
        #[serde(default)]
        cleanup: JournalCleanupPhase,
        #[serde(default)]
        rollback: JournalRollbackPhase,
    },
    Rename {
        #[serde(default)]
        workspace_id: u64,
        root: PathBuf,
        old_path: String,
        new_path: String,
        source: SecurePathSnapshot,
        previous: Option<SecurePathSnapshot>,
        moved: bool,
        #[serde(default)]
        phase: JournalOperationPhase,
        #[serde(default)]
        cleanup: JournalCleanupPhase,
        #[serde(default)]
        rollback: JournalRollbackPhase,
    },
    Delete {
        #[serde(default)]
        workspace_id: u64,
        root: PathBuf,
        path: String,
        snapshot: SecurePathSnapshot,
        recursive: bool,
        removed: bool,
        #[serde(default)]
        phase: JournalOperationPhase,
        #[serde(default)]
        cleanup: JournalCleanupPhase,
        #[serde(default)]
        rollback: JournalRollbackPhase,
    },
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
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(0),
            transactions: Mutex::new(HashMap::new()),
            outcomes: Mutex::new(HashMap::new()),
            acknowledgements: Mutex::new(HashMap::new()),
            recovery_authorizations: Mutex::new(HashMap::new()),
            store: None,
        }
    }

    pub fn open(store: crate::persistence::StateStore) -> Result<Self, WorkspaceEditError> {
        recover_workspace_edit_journals(&store)?;
        let mut outcomes = load_workspace_edit_outcomes(&store)?;
        let recovery_authorizations =
            prepare_persisted_workspace_edit_outcomes(&store, &mut outcomes)?;
        let acknowledgements = load_workspace_edit_acknowledgements(&store)?;
        let next_id = outcomes
            .keys()
            .chain(acknowledgements.keys())
            .copied()
            .max()
            .unwrap_or(0);
        Ok(Self {
            next_id: AtomicU64::new(next_id),
            transactions: Mutex::new(HashMap::new()),
            outcomes: Mutex::new(outcomes),
            acknowledgements: Mutex::new(acknowledgements),
            recovery_authorizations: Mutex::new(recovery_authorizations),
            store: Some(store),
        })
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
        let outcomes = self
            .outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if transactions
            .values()
            .filter(|transaction| !transaction.phase.is_finished())
            .count()
            + outcomes
                .values()
                .filter(|outcome| {
                    outcome.phase.is_recovery() && !outcome_authorization_expired(outcome)
                })
                .count()
            >= MAX_WORKSPACE_EDIT_TRANSACTIONS
        {
            return Err(WorkspaceEditError::Limit(
                "too many workspace edit transactions are active".to_owned(),
            ));
        }
        let prepared = prepare_transaction(edit, roots, open_documents)?;
        let (staged_documents, staged_operations) =
            staged_renderer_edit(&prepared.documents, &prepared.staged_operations);
        let id = allocate_transaction_id(&self.next_id, &transactions, &outcomes)?;
        drop(outcomes);
        let authorization = random_token().map_err(WorkspaceEditError::Io)?;
        let staged = StagedWorkspaceEdit {
            transaction_id: id,
            authorization: authorization.clone(),
            documents: staged_documents,
            operations: staged_operations,
        };
        let model_directives = plan_open_model_lineages(&staged, open_documents)?;
        transactions.insert(
            id,
            WorkspaceEditTransaction {
                created_at: Instant::now(),
                authorization,
                documents: prepared.documents,
                operations: prepared.operations,
                staged: staged.clone(),
                model_directives,
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
        let store = self.store.clone();
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        if !transactions.contains_key(&transaction_id) {
            drop(transactions);
            let status = self.status(transaction_id, authorization)?;
            return match status.phase {
                WorkspaceEditTransactionPhase::FinishingCommitted
                | WorkspaceEditTransactionPhase::CommittedCleanupRequired
                | WorkspaceEditTransactionPhase::FinishedCommitted => Ok(()),
                WorkspaceEditTransactionPhase::RecoveryRequired => {
                    Err(recovery_required(transaction_id))
                }
                _ => Err(WorkspaceEditError::Invalid(
                    "workspace edit transaction can no longer be committed".to_owned(),
                )),
            };
        }
        let transaction = transactions
            .get_mut(&transaction_id)
            .expect("workspace edit transaction existence was checked");
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
        save_journal(
            store.as_ref(),
            transaction_id,
            transaction,
            JournalPhase::Applying,
        )?;

        for index in 0..transaction.operations.len() {
            if let TransactionOperation::Resource(operation) = &mut transaction.operations[index]
                && let Err(error) = validate_and_refresh_resource_snapshot(operation)
            {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    workspace_edit_operation_error(index, error),
                ));
            }
            if let Err(error) = validate_future_ancestor_snapshots(
                &transaction.operations,
                &transaction.documents,
                index,
            )
            .map_err(|error| workspace_edit_operation_error(index, error))
            {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    error,
                ));
            }
            set_operation_phase(transaction, index, JournalOperationPhase::Applying);
            if let Err(error) = save_journal(
                store.as_ref(),
                transaction_id,
                transaction,
                JournalPhase::Applying,
            ) {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    error,
                ));
            }
            let document_index = match &transaction.operations[index] {
                TransactionOperation::TextDocument(document) => Some(*document),
                TransactionOperation::Resource(_) => None,
            };
            let prepared_operation = if let Some(document) = document_index {
                prepare_closed_document(
                    &mut transaction.documents[document],
                    &transaction.authorization,
                    index,
                )
            } else {
                match &mut transaction.operations[index] {
                    TransactionOperation::Resource(operation) => {
                        prepare_create_operation(operation, &transaction.authorization)
                    }
                    TransactionOperation::TextDocument(_) => unreachable!(),
                }
            };
            let prepared_operation = match prepared_operation {
                Ok(prepared) => prepared,
                Err(error) => {
                    return Err(fail_commit(
                        store.as_ref(),
                        transaction_id,
                        transaction,
                        workspace_edit_operation_error(index, error),
                    ));
                }
            };
            let prepared_save_error = if prepared_operation {
                save_journal(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    JournalPhase::Applying,
                )
                .err()
            } else {
                None
            };
            if let Some(error) = prepared_save_error {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    error,
                ));
            }
            let result = match &mut transaction.operations[index] {
                TransactionOperation::TextDocument(document) => {
                    if transaction.documents[*document].closed.is_some() {
                        apply_closed_document(
                            &mut transaction.documents[*document],
                            &transaction.authorization,
                            index,
                        )
                    } else {
                        Ok(())
                    }
                }
                TransactionOperation::Resource(operation) => {
                    apply_resource_operation(operation, &transaction.authorization, index)
                }
            };
            if let Err(error) = result.map_err(|error| workspace_edit_operation_error(index, error))
            {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    error,
                ));
            }
            set_operation_phase(transaction, index, JournalOperationPhase::Applied);
            if let Err(error) = save_journal(
                store.as_ref(),
                transaction_id,
                transaction,
                JournalPhase::Applying,
            ) {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    error,
                ));
            }
            if let Err(error) = refresh_transformed_resource_snapshots(
                &mut transaction.operations,
                &transaction.documents,
                index,
            )
            .map_err(|error| workspace_edit_operation_error(index, error))
            {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    error,
                ));
            }
            if let Err(error) = save_journal(
                store.as_ref(),
                transaction_id,
                transaction,
                JournalPhase::Applying,
            ) {
                return Err(fail_commit(
                    store.as_ref(),
                    transaction_id,
                    transaction,
                    error,
                ));
            }
        }
        if let Err(error) = save_journal(
            store.as_ref(),
            transaction_id,
            transaction,
            JournalPhase::Applied,
        ) {
            return Err(fail_commit(
                store.as_ref(),
                transaction_id,
                transaction,
                error,
            ));
        }
        transaction.phase = WorkspaceEditTransactionPhase::Committed;
        Ok(())
    }

    pub fn staged_operations(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<Vec<StagedWorkspaceEditOperation>, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        if let Some(transaction) = transactions.get(&transaction_id) {
            validate_authorization(transaction, authorization)?;
            return Ok(staged_operations(&transaction.operations));
        }
        drop(transactions);
        let outcomes = self
            .outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let outcome = outcomes
            .get(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_outcome_authorization(outcome, authorization)?;
        Ok(outcome_operations(outcome))
    }

    pub fn staged(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<StagedWorkspaceEdit, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        Ok(transaction.staged.clone())
    }

    pub fn model_directives(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<Vec<WorkspaceEditModelDirective>, WorkspaceEditError> {
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        let transaction = transactions
            .get(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_authorization(transaction, authorization)?;
        Ok(transaction.model_directives.clone())
    }

    pub fn rollback(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        let store = self.store.clone();
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        if !transactions.contains_key(&transaction_id) {
            drop(transactions);
            return self.rollback_recovered_outcome(transaction_id, authorization);
        }
        let transaction = transactions
            .get_mut(&transaction_id)
            .expect("workspace edit transaction existence was checked");
        validate_authorization(transaction, authorization)?;
        match transaction.phase {
            WorkspaceEditTransactionPhase::RolledBack
            | WorkspaceEditTransactionPhase::FinishedRolledBack
            | WorkspaceEditTransactionPhase::FinishedUncommitted => return Ok(()),
            WorkspaceEditTransactionPhase::FinishingCommitted
            | WorkspaceEditTransactionPhase::CommittedCleanupRequired
            | WorkspaceEditTransactionPhase::FinishedCommitted => {
                return Err(WorkspaceEditError::Invalid(
                    "a workspace edit with a durable commit decision cannot be rolled back"
                        .to_owned(),
                ));
            }
            _ => {}
        }
        rollback_transaction_durably(store.as_ref(), transaction, transaction_id)?;
        remove_recovery_directories(transaction)?;
        Ok(())
    }

    pub fn finish(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        let store = self.store.clone();
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        if !transactions.contains_key(&transaction_id) {
            drop(transactions);
            let outcomes = self
                .outcomes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let outcome = outcomes
                .get(&transaction_id)
                .ok_or(WorkspaceEditError::Expired)?;
            validate_outcome_authorization(outcome, authorization)?;
            if matches!(
                outcome.phase,
                WorkspaceEditTransactionPhase::FinishingCommitted
                    | WorkspaceEditTransactionPhase::CommittedCleanupRequired
            ) {
                drop(outcomes);
                self.finalize_recovered_outcome(transaction_id, authorization)?;
                return Ok(true);
            }
            return Ok(true);
        }
        let transaction = transactions
            .get_mut(&transaction_id)
            .expect("workspace edit transaction existence was checked");
        validate_authorization(transaction, authorization)?;
        let terminal_count = self
            .outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .filter(|outcome| outcome.phase.is_finished())
            .count();
        if terminal_count >= MAX_WORKSPACE_EDIT_OUTCOMES {
            return Err(WorkspaceEditError::Limit(format!(
                "too many unacknowledged workspace edit completions are retained ({MAX_WORKSPACE_EDIT_OUTCOMES})"
            )));
        }
        let next_phase = match transaction.phase {
            WorkspaceEditTransactionPhase::Staged => {
                WorkspaceEditTransactionPhase::FinishedUncommitted
            }
            WorkspaceEditTransactionPhase::Committed => {
                WorkspaceEditTransactionPhase::FinishedCommitted
            }
            WorkspaceEditTransactionPhase::FinishingCommitted => {
                WorkspaceEditTransactionPhase::FinishedCommitted
            }
            WorkspaceEditTransactionPhase::CommittedCleanupRequired => {
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
        if next_phase == WorkspaceEditTransactionPhase::FinishedCommitted {
            if let Some(store) = store.as_ref() {
                let mut journal = if transaction.phase.is_commit_decided() {
                    load_workspace_edit_journal(store, transaction_id)?
                } else {
                    let journal =
                        workspace_edit_journal(transaction, JournalPhase::FinishingCommitted);
                    persist_journal(store, transaction_id, &journal)?;
                    transaction.phase = WorkspaceEditTransactionPhase::FinishingCommitted;
                    let finishing = workspace_edit_outcome(
                        &journal,
                        WorkspaceEditTransactionPhase::FinishingCommitted,
                    );
                    persist_outcome(store, transaction_id, &finishing)?;
                    self.outcomes
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .insert(transaction_id, finishing);
                    journal
                };
                if let Err(error) = cleanup_committed_journal(store, transaction_id, &mut journal) {
                    mark_committed_cleanup_required(
                        store,
                        transaction_id,
                        transaction,
                        &journal,
                        &self.outcomes,
                    )?;
                    return Err(error);
                }
                clear_transaction_tombstones(transaction);
            } else {
                transaction.phase = WorkspaceEditTransactionPhase::FinishingCommitted;
                if let Err(error) =
                    finish_resources(&mut transaction.operations, &mut transaction.documents)
                {
                    transaction.phase = WorkspaceEditTransactionPhase::CommittedCleanupRequired;
                    return Err(error);
                }
            }
        }
        if next_phase != WorkspaceEditTransactionPhase::FinishedCommitted || store.is_none() {
            remove_recovery_directories(transaction)?;
        }
        if let Some(store) = store.as_ref()
            && let Err(error) = store
                .delete_workspace_edit_editor_recovery(transaction_id)
                .map_err(|error| {
                    WorkspaceEditError::Recovery(format!(
                        "could not remove completed editor recovery state: {error}"
                    ))
                })
        {
            if next_phase == WorkspaceEditTransactionPhase::FinishedCommitted {
                mark_active_committed_cleanup_required(
                    store,
                    transaction_id,
                    transaction,
                    &self.outcomes,
                )?;
            }
            return Err(error);
        }
        transaction.phase = next_phase;
        let journal = workspace_edit_journal(
            transaction,
            if next_phase == WorkspaceEditTransactionPhase::FinishedCommitted {
                JournalPhase::FinishingCommitted
            } else {
                JournalPhase::RolledBack
            },
        );
        let outcome = workspace_edit_outcome(&journal, next_phase);
        if let Some(store) = store.as_ref() {
            persist_outcome(store, transaction_id, &outcome)?;
        }
        delete_journal(store.as_ref(), transaction_id)?;
        self.outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(transaction_id, outcome);
        self.recovery_authorizations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(transaction_id, authorization.to_owned());
        transactions.remove(&transaction_id);
        Ok(true)
    }

    pub fn acknowledge_completion(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<bool, WorkspaceEditError> {
        let mut outcomes = self
            .outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(outcome) = outcomes.get(&transaction_id) {
            validate_outcome_authorization(outcome, authorization)?;
            if !outcome.phase.is_finished() {
                return Err(recovery_required(transaction_id));
            }
            let acknowledgement = WorkspaceEditAcknowledgement {
                authorization_hash: outcome.authorization_hash.clone(),
                created_at: unix_timestamp(),
            };
            if let Some(store) = self.store.as_ref() {
                store
                    .acknowledge_workspace_edit_completion(
                        transaction_id,
                        &acknowledgement.authorization_hash,
                        acknowledgement.created_at,
                    )
                    .map_err(|error| {
                        WorkspaceEditError::Recovery(format!(
                            "could not acknowledge workspace edit outcome: {error}"
                        ))
                    })?;
            }
            outcomes.remove(&transaction_id);
            self.recovery_authorizations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&transaction_id);
            let mut acknowledgements = self
                .acknowledgements
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            acknowledgements.insert(transaction_id, acknowledgement);
            trim_workspace_edit_acknowledgements(self.store.as_ref(), &mut acknowledgements)?;
            return Ok(true);
        }
        drop(outcomes);
        let acknowledgements = self
            .acknowledgements
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let acknowledgement = acknowledgements
            .get(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        if acknowledgement.authorization_hash != content_hash(authorization) {
            return Err(WorkspaceEditError::Invalid(
                "workspace edit transaction authorization is invalid".to_owned(),
            ));
        }
        Ok(true)
    }

    pub fn finalize(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        let store = self.store.clone();
        let mut transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        remove_expired(&mut transactions);
        if !transactions.contains_key(&transaction_id) {
            drop(transactions);
            return self.finalize_recovered_outcome(transaction_id, authorization);
        }
        let transaction = transactions
            .get_mut(&transaction_id)
            .expect("workspace edit transaction existence was checked");
        validate_authorization(transaction, authorization)?;
        if transaction.phase != WorkspaceEditTransactionPhase::RecoveryRequired {
            drop(transactions);
            self.finish(transaction_id, authorization)?;
            return self.status(transaction_id, authorization);
        }
        let next_phase = if transaction_has_applied_files(transaction) {
            WorkspaceEditTransactionPhase::FinishedCommitted
        } else {
            WorkspaceEditTransactionPhase::FinishedRolledBack
        };
        if next_phase == WorkspaceEditTransactionPhase::FinishedCommitted {
            if let Some(store) = store.as_ref() {
                let mut journal =
                    workspace_edit_journal(transaction, JournalPhase::FinishingCommitted);
                persist_journal(store, transaction_id, &journal)?;
                transaction.phase = WorkspaceEditTransactionPhase::FinishingCommitted;
                let finishing = workspace_edit_outcome(
                    &journal,
                    WorkspaceEditTransactionPhase::FinishingCommitted,
                );
                persist_outcome(store, transaction_id, &finishing)?;
                self.outcomes
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(transaction_id, finishing);
                if let Err(error) = cleanup_committed_journal(store, transaction_id, &mut journal) {
                    mark_committed_cleanup_required(
                        store,
                        transaction_id,
                        transaction,
                        &journal,
                        &self.outcomes,
                    )?;
                    return Err(error);
                }
                clear_transaction_tombstones(transaction);
            } else {
                transaction.phase = WorkspaceEditTransactionPhase::FinishingCommitted;
                if let Err(error) =
                    finish_resources(&mut transaction.operations, &mut transaction.documents)
                {
                    transaction.phase = WorkspaceEditTransactionPhase::CommittedCleanupRequired;
                    return Err(error);
                }
            }
        }
        if next_phase != WorkspaceEditTransactionPhase::FinishedCommitted || store.is_none() {
            remove_recovery_directories(transaction)?;
        }
        if let Some(store) = store.as_ref() {
            let result = if next_phase == WorkspaceEditTransactionPhase::FinishedRolledBack {
                store.restore_workspace_edit_editor_recovery(transaction_id)
            } else {
                store.delete_workspace_edit_editor_recovery(transaction_id)
            };
            if let Err(error) = result.map_err(|error| {
                WorkspaceEditError::Recovery(format!(
                    "could not reconcile finalized editor recovery state: {error}"
                ))
            }) {
                if next_phase == WorkspaceEditTransactionPhase::FinishedCommitted {
                    mark_active_committed_cleanup_required(
                        store,
                        transaction_id,
                        transaction,
                        &self.outcomes,
                    )?;
                }
                return Err(error);
            }
        }
        transaction.phase = next_phase;
        let journal = workspace_edit_journal(
            transaction,
            if next_phase == WorkspaceEditTransactionPhase::FinishedCommitted {
                JournalPhase::FinishingCommitted
            } else {
                JournalPhase::RolledBack
            },
        );
        let outcome = workspace_edit_outcome(&journal, next_phase);
        if let Some(store) = store.as_ref() {
            persist_outcome(store, transaction_id, &outcome)?;
        }
        delete_journal(store.as_ref(), transaction_id)?;
        let status = outcome_status(transaction_id, &outcome);
        self.outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(transaction_id, outcome);
        self.recovery_authorizations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(transaction_id, authorization.to_owned());
        transactions.remove(&transaction_id);
        Ok(status)
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
        if let Some(transaction) = transactions.get(&transaction_id) {
            validate_authorization(transaction, authorization)?;
            return Ok(transaction_status(transaction_id, transaction));
        }
        drop(transactions);
        let outcomes = self
            .outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let outcome = outcomes
            .get(&transaction_id)
            .ok_or(WorkspaceEditError::Expired)?;
        validate_outcome_authorization(outcome, authorization)?;
        Ok(outcome_status(transaction_id, outcome))
    }

    pub fn recoveries(&self) -> Vec<WorkspaceEditRecovery> {
        let outcomes = self
            .outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let authorizations = self
            .recovery_authorizations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut recoveries = authorizations
            .iter()
            .filter_map(|(transaction_id, authorization)| {
                let outcome = outcomes.get(transaction_id)?;
                ((outcome.phase.is_recovery()
                    || outcome.phase == WorkspaceEditTransactionPhase::FinishingCommitted
                    || outcome.phase.is_finished())
                    && !outcome_authorization_expired(outcome))
                .then(|| WorkspaceEditRecovery {
                    transaction_id: *transaction_id,
                    authorization: authorization.clone(),
                    status: outcome_status(*transaction_id, outcome),
                })
            })
            .collect::<Vec<_>>();
        recoveries.sort_unstable_by_key(|recovery| recovery.transaction_id);
        recoveries.truncate(MAX_WORKSPACE_EDIT_RECOVERIES);
        recoveries
    }

    pub(super) fn watch_disposition(
        &self,
        path: &std::path::Path,
    ) -> WorkspaceEditWatchDisposition {
        let transactions = self
            .transactions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut disposition = WorkspaceEditWatchDisposition::Unrelated;
        for transaction in transactions.values() {
            if !transaction
                .operations
                .iter()
                .any(|operation| match operation {
                    TransactionOperation::TextDocument(_) => false,
                    TransactionOperation::Resource(operation) => {
                        let overlaps = |relative: &str| {
                            let target = operation.workspace_root.join(relative);
                            path.starts_with(&target) || target.starts_with(path)
                        };
                        match &operation.kind {
                            ResourceOperationKind::Create { path, .. }
                            | ResourceOperationKind::Delete { path, .. } => overlaps(path),
                            ResourceOperationKind::Rename {
                                old_path, new_path, ..
                            } => overlaps(old_path) || overlaps(new_path),
                        }
                    }
                })
            {
                continue;
            }
            let current = match transaction.phase {
                WorkspaceEditTransactionPhase::FinishingCommitted
                | WorkspaceEditTransactionPhase::CommittedCleanupRequired
                | WorkspaceEditTransactionPhase::FinishedCommitted => {
                    WorkspaceEditWatchDisposition::Committed
                }
                WorkspaceEditTransactionPhase::RolledBack
                | WorkspaceEditTransactionPhase::FinishedRolledBack
                | WorkspaceEditTransactionPhase::FinishedUncommitted => {
                    WorkspaceEditWatchDisposition::RolledBack
                }
                _ => WorkspaceEditWatchDisposition::Active,
            };
            if current == WorkspaceEditWatchDisposition::Active {
                return current;
            }
            disposition = current;
        }
        disposition
    }

    fn rollback_recovered_outcome(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<(), WorkspaceEditError> {
        let outcome = {
            let outcomes = self
                .outcomes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let outcome = outcomes
                .get(&transaction_id)
                .ok_or(WorkspaceEditError::Expired)?;
            validate_outcome_authorization(outcome, authorization)?;
            outcome.clone()
        };
        match outcome.phase {
            WorkspaceEditTransactionPhase::FinishedRolledBack => return Ok(()),
            WorkspaceEditTransactionPhase::FinishingCommitted
            | WorkspaceEditTransactionPhase::CommittedCleanupRequired
            | WorkspaceEditTransactionPhase::FinishedCommitted => {
                return Err(WorkspaceEditError::Invalid(
                    "a workspace edit with a durable commit decision cannot be rolled back"
                        .to_owned(),
                ));
            }
            WorkspaceEditTransactionPhase::RecoveryRequired => {}
            _ => return Err(WorkspaceEditError::Expired),
        }
        let store = self.store.as_ref().ok_or(WorkspaceEditError::Expired)?;
        let mut journal = load_workspace_edit_journal(store, transaction_id)?;
        rollback_recovered_journal(store, transaction_id, &mut journal)?;
        store
            .restore_workspace_edit_editor_recovery(transaction_id)
            .map_err(|error| {
                WorkspaceEditError::Recovery(format!(
                    "could not restore recovered editor state: {error}"
                ))
            })?;
        cleanup_recovery_directories(&journal)?;
        let finished = WorkspaceEditOutcome {
            phase: WorkspaceEditTransactionPhase::FinishedRolledBack,
            created_at: unix_timestamp(),
            ..outcome
        };
        persist_outcome(store, transaction_id, &finished)?;
        delete_journal(Some(store), transaction_id)?;
        self.outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(transaction_id, finished);
        Ok(())
    }

    fn finalize_recovered_outcome(
        &self,
        transaction_id: u64,
        authorization: &str,
    ) -> Result<WorkspaceEditTransactionStatus, WorkspaceEditError> {
        let outcome = {
            let outcomes = self
                .outcomes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let outcome = outcomes
                .get(&transaction_id)
                .ok_or(WorkspaceEditError::Expired)?;
            validate_outcome_authorization(outcome, authorization)?;
            outcome.clone()
        };
        if outcome.phase.is_finished() {
            return Ok(outcome_status(transaction_id, &outcome));
        }
        if matches!(
            outcome.phase,
            WorkspaceEditTransactionPhase::FinishingCommitted
                | WorkspaceEditTransactionPhase::CommittedCleanupRequired
        ) {
            let store = self.store.as_ref().ok_or(WorkspaceEditError::Expired)?;
            let mut journal = load_workspace_edit_journal(store, transaction_id)?;
            if let Err(error) = cleanup_committed_journal(store, transaction_id, &mut journal) {
                mark_recovered_committed_cleanup_required(
                    store,
                    transaction_id,
                    &outcome,
                    &self.outcomes,
                )?;
                return Err(error);
            }
            if let Err(error) = store
                .delete_workspace_edit_editor_recovery(transaction_id)
                .map_err(|error| {
                    WorkspaceEditError::Recovery(format!(
                        "could not finalize recovered editor state: {error}"
                    ))
                })
            {
                mark_recovered_committed_cleanup_required(
                    store,
                    transaction_id,
                    &outcome,
                    &self.outcomes,
                )?;
                return Err(error);
            }
            let finished = WorkspaceEditOutcome {
                phase: WorkspaceEditTransactionPhase::FinishedCommitted,
                created_at: unix_timestamp(),
                ..outcome
            };
            persist_outcome(store, transaction_id, &finished)?;
            delete_journal(Some(store), transaction_id)?;
            let status = outcome_status(transaction_id, &finished);
            self.outcomes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(transaction_id, finished);
            return Ok(status);
        }
        if outcome.phase != WorkspaceEditTransactionPhase::RecoveryRequired {
            return Err(WorkspaceEditError::Expired);
        }
        let store = self.store.as_ref().ok_or(WorkspaceEditError::Expired)?;
        let mut journal = load_workspace_edit_journal(store, transaction_id)?;
        let phase = if recovered_journal_has_applied_files(&journal)? {
            journal.phase = JournalPhase::FinishingCommitted;
            persist_journal(store, transaction_id, &journal)?;
            let finishing = WorkspaceEditOutcome {
                phase: WorkspaceEditTransactionPhase::FinishingCommitted,
                created_at: unix_timestamp(),
                ..outcome.clone()
            };
            persist_outcome(store, transaction_id, &finishing)?;
            self.outcomes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(transaction_id, finishing);
            if let Err(error) = cleanup_committed_journal(store, transaction_id, &mut journal) {
                mark_recovered_committed_cleanup_required(
                    store,
                    transaction_id,
                    &outcome,
                    &self.outcomes,
                )?;
                return Err(error);
            }
            if let Err(error) = store
                .delete_workspace_edit_editor_recovery(transaction_id)
                .map_err(|error| {
                    WorkspaceEditError::Recovery(format!(
                        "could not finalize recovered editor state: {error}"
                    ))
                })
            {
                mark_recovered_committed_cleanup_required(
                    store,
                    transaction_id,
                    &outcome,
                    &self.outcomes,
                )?;
                return Err(error);
            }
            WorkspaceEditTransactionPhase::FinishedCommitted
        } else {
            rollback_recovered_journal(store, transaction_id, &mut journal)?;
            store
                .restore_workspace_edit_editor_recovery(transaction_id)
                .map_err(|error| {
                    WorkspaceEditError::Recovery(format!(
                        "could not restore finalized editor state: {error}"
                    ))
                })?;
            cleanup_recovery_directories(&journal)?;
            WorkspaceEditTransactionPhase::FinishedRolledBack
        };
        let finished = WorkspaceEditOutcome {
            phase,
            created_at: unix_timestamp(),
            ..outcome
        };
        persist_outcome(store, transaction_id, &finished)?;
        delete_journal(Some(store), transaction_id)?;
        let status = outcome_status(transaction_id, &finished);
        self.outcomes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(transaction_id, finished);
        Ok(status)
    }
}

fn staged_renderer_edit(
    documents: &[TransactionDocument],
    operations: &[StagedWorkspaceEditOperation],
) -> (
    Vec<StagedWorkspaceEditDocument>,
    Vec<StagedWorkspaceEditOperation>,
) {
    let mut staged_documents = documents
        .iter()
        .map(|document| StagedWorkspaceEditDocument {
            workspace_id: document.workspace_id,
            path: document.path.clone(),
            original_path: document
                .open_path
                .clone()
                .unwrap_or_else(|| document.path.clone()),
            original_text: document.original_text.clone(),
            new_text: document.new_text.clone(),
            generation: document.open.map(|open| open.generation),
            version: document.open.map(|open| open.version),
        })
        .collect::<Vec<_>>();
    let mut lineages = HashMap::<(WorkspaceId, String, u64, i64), Vec<usize>>::new();
    for (index, document) in documents.iter().enumerate() {
        let (Some(open), Some(open_path)) = (document.open, document.open_path.as_ref()) else {
            continue;
        };
        lineages
            .entry((
                document.workspace_id,
                open_path.clone(),
                open.generation,
                open.version,
            ))
            .or_default()
            .push(index);
    }
    let mut coalesced = HashMap::<usize, (usize, usize)>::new();
    for lineage in lineages.values().filter(|lineage| lineage.len() > 1) {
        let first = lineage[0];
        let last = *lineage.last().expect("non-empty open document lineage");
        staged_documents[first].path = documents[last].path.clone();
        staged_documents[first].new_text = documents[last].new_text.clone();
        for document in lineage {
            coalesced.insert(*document, (first, last));
        }
    }
    let staged_operations = operations
        .iter()
        .filter_map(|operation| match operation {
            StagedWorkspaceEditOperation::TextDocument { document } => {
                match coalesced.get(document) {
                    Some((_, last)) if document != last => None,
                    Some((first, _)) => {
                        Some(StagedWorkspaceEditOperation::TextDocument { document: *first })
                    }
                    None => Some(operation.clone()),
                }
            }
            _ => Some(operation.clone()),
        })
        .collect();
    (staged_documents, staged_operations)
}

#[derive(Clone)]
struct VirtualOpenModel {
    workspace_id: WorkspaceId,
    original_path: String,
    path: Option<String>,
    generation: u64,
    version: i64,
    original_text: String,
    text: String,
    expected_text: String,
    touched: bool,
}

pub(super) fn plan_open_model_lineages(
    edit: &StagedWorkspaceEdit,
    observations: &[WorkspaceEditOpenDocument],
) -> Result<Vec<WorkspaceEditModelDirective>, WorkspaceEditError> {
    let mut models = observations
        .iter()
        .map(|document| VirtualOpenModel {
            workspace_id: document.workspace_id,
            original_path: document.path.clone(),
            path: Some(document.path.clone()),
            generation: document.generation,
            version: document.version,
            original_text: document.text.clone(),
            text: document.text.clone(),
            expected_text: document.saved_text.clone(),
            touched: false,
        })
        .collect::<Vec<_>>();

    for operation in &edit.operations {
        match operation {
            StagedWorkspaceEditOperation::TextDocument { document } => {
                let document = edit.documents.get(*document).ok_or_else(|| {
                    WorkspaceEditError::Invalid(format!(
                        "workspace edit document {document} is missing"
                    ))
                })?;
                let targets = models_at(&models, document.workspace_id, &document.path, false);
                if document.generation.is_none() || document.version.is_none() {
                    if !targets.is_empty() {
                        return Err(WorkspaceEditError::Stale(format!(
                            "workspace edit target {} opened after validation",
                            document.path
                        )));
                    }
                    continue;
                }
                if targets.is_empty() {
                    return Err(WorkspaceEditError::Stale(format!(
                        "workspace edit target {} is not available",
                        document.path
                    )));
                }
                for target in targets {
                    let target = &mut models[target];
                    if target.text != document.original_text {
                        return Err(WorkspaceEditError::Stale(format!(
                            "workspace edit target {} has conflicting ordered edits",
                            document.path
                        )));
                    }
                    target.text = document.new_text.clone();
                    target.expected_text = document.new_text.clone();
                    target.touched = true;
                }
            }
            StagedWorkspaceEditOperation::RenameFile {
                workspace_id,
                old_path,
                new_path,
            } => {
                let sources = models_at(&models, *workspace_id, old_path, true);
                let destinations = models_at(&models, *workspace_id, new_path, true);
                assert_clean_open_models(&models, &destinations, "overwrite")?;
                for target in destinations {
                    let target = &mut models[target];
                    target.path = None;
                    target.touched = true;
                }
                for source in sources {
                    let source = &mut models[source];
                    let path = source.path.as_deref().expect("source models have a path");
                    let suffix = path_suffix(path, old_path).expect("source is below rename path");
                    source.path = Some(join_prefix(new_path, suffix));
                    source.touched = true;
                }
            }
            StagedWorkspaceEditOperation::CreateFile { workspace_id, path }
            | StagedWorkspaceEditOperation::DeleteFile {
                workspace_id, path, ..
            } => {
                let affected = models_at(&models, *workspace_id, path, true);
                let action = match operation {
                    StagedWorkspaceEditOperation::CreateFile { .. } => "overwrite",
                    StagedWorkspaceEditOperation::DeleteFile { .. } => "delete",
                    _ => unreachable!(),
                };
                assert_clean_open_models(&models, &affected, action)?;
                for target in affected {
                    let target = &mut models[target];
                    target.path = None;
                    target.touched = true;
                }
            }
        }
    }

    Ok(models
        .into_iter()
        .filter(|model| {
            model.touched
                && (model.path.as_deref() != Some(&model.original_path)
                    || model.text != model.original_text)
        })
        .map(|model| WorkspaceEditModelDirective {
            workspace_id: model.workspace_id,
            original_path: model.original_path,
            path: model.path,
            generation: model.generation,
            version: model.version,
            original_text: model.original_text,
            text: model.text,
        })
        .collect())
}

fn models_at(
    models: &[VirtualOpenModel],
    workspace_id: WorkspaceId,
    path: &str,
    descendants: bool,
) -> Vec<usize> {
    models
        .iter()
        .enumerate()
        .filter_map(|(index, model)| {
            (model.workspace_id == workspace_id
                && model.path.as_deref().is_some_and(|model_path| {
                    model_path == path || (descendants && path_is_within(model_path, path))
                }))
            .then_some(index)
        })
        .collect()
}

fn assert_clean_open_models(
    models: &[VirtualOpenModel],
    targets: &[usize],
    action: &str,
) -> Result<(), WorkspaceEditError> {
    for target in targets {
        let target = &models[*target];
        if target.text != target.expected_text {
            return Err(WorkspaceEditError::Stale(format!(
                "cannot {action} dirty open document {}",
                target.path.as_deref().unwrap_or(&target.original_path)
            )));
        }
    }
    Ok(())
}

fn workspace_edit_operation_error(index: usize, error: WorkspaceEditError) -> WorkspaceEditError {
    let annotate = |message: String| format!("workspace edit operation {index}: {message}");
    match error {
        WorkspaceEditError::Invalid(message) => WorkspaceEditError::Invalid(annotate(message)),
        WorkspaceEditError::Unsupported(message) => {
            WorkspaceEditError::Unsupported(annotate(message))
        }
        WorkspaceEditError::Stale(message) => WorkspaceEditError::Stale(annotate(message)),
        WorkspaceEditError::Limit(message) => WorkspaceEditError::Limit(annotate(message)),
        WorkspaceEditError::Io(message) => WorkspaceEditError::Io(annotate(message)),
        WorkspaceEditError::Recovery(message) => WorkspaceEditError::Recovery(annotate(message)),
        WorkspaceEditError::Expired => WorkspaceEditError::Expired,
    }
}

fn set_operation_phase(
    transaction: &mut WorkspaceEditTransaction,
    operation_index: usize,
    phase: JournalOperationPhase,
) {
    match &mut transaction.operations[operation_index] {
        TransactionOperation::TextDocument(document) => {
            if let Some(closed) = transaction.documents[*document].closed.as_mut() {
                closed.operation_phase = phase;
            }
        }
        TransactionOperation::Resource(operation) => operation.phase = phase,
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

fn prepare_transaction(
    edit: &Value,
    roots: &[WorkspaceEditRoot],
    open_documents: &[WorkspaceEditOpenDocument],
) -> Result<PreparedTransaction, WorkspaceEditError> {
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
            requested.push(RequestedChange::Text {
                uri: uri.clone(),
                version: None,
                edits: parse_edits(edits)?,
            });
        }
    } else if let Some(changes) = object.get("documentChanges") {
        let changes = changes.as_array().ok_or_else(|| {
            WorkspaceEditError::Invalid(
                "workspace edit documentChanges must be an array".to_owned(),
            )
        })?;
        for change in changes {
            if let Some(kind) = change.get("kind").and_then(Value::as_str) {
                requested.push(parse_resource_change(kind, change)?);
                continue;
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
            requested.push(RequestedChange::Text {
                uri: uri.to_owned(),
                version,
                edits: parse_edits(change.get("edits").unwrap_or(&Value::Null))?,
            });
        }
    } else {
        return Ok(PreparedTransaction {
            documents: Vec::new(),
            operations: Vec::new(),
            staged_operations: Vec::new(),
        });
    }

    if requested.len() > MAX_WORKSPACE_EDIT_EDITS {
        return Err(WorkspaceEditError::Limit(format!(
            "workspace edit exceeds the {MAX_WORKSPACE_EDIT_EDITS}-operation limit"
        )));
    }
    let edit_count = requested
        .iter()
        .map(|change| match change {
            RequestedChange::Text { edits, .. } => edits.len(),
            _ => 0,
        })
        .sum::<usize>();
    if edit_count > MAX_WORKSPACE_EDIT_EDITS {
        return Err(WorkspaceEditError::Limit(format!(
            "workspace edit exceeds the {MAX_WORKSPACE_EDIT_EDITS}-edit limit"
        )));
    }
    let replacement_bytes = requested
        .iter()
        .filter_map(|change| match change {
            RequestedChange::Text { edits, .. } => Some(edits.as_slice()),
            _ => None,
        })
        .flatten()
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
    let mut workspaces = HashMap::new();
    let mut workspace_roots = HashMap::new();
    for root in roots {
        let canonical = fs::canonicalize(&root.path)
            .map_err(|error| WorkspaceEditError::Io(error.to_string()))?;
        let workspace =
            Arc::new(SecureWorkspace::open(&canonical).map_err(WorkspaceEditError::Io)?);
        workspaces.insert(root.workspace_id, workspace);
        workspace_roots.insert(root.workspace_id, canonical);
    }
    let mut virtual_events: HashMap<WorkspaceId, Vec<VirtualEvent>> = HashMap::new();
    let mut logical_texts: HashMap<(WorkspaceId, String), String> = HashMap::new();
    let mut persisted_texts: HashMap<(WorkspaceId, String), String> = HashMap::new();
    let mut documents = Vec::new();
    let mut operations = Vec::with_capacity(requested.len());
    let mut staged_operations = Vec::with_capacity(requested.len());
    let mut staged_bytes = 0_usize;
    for change in requested {
        match change {
            RequestedChange::Text {
                uri,
                version,
                edits,
            } => {
                let resolved = resolve_uri_lexical(&uri, roots)?;
                let workspace =
                    workspaces
                        .get(&resolved.workspace_id)
                        .cloned()
                        .ok_or_else(|| {
                            WorkspaceEditError::Invalid("workspace root disappeared".to_owned())
                        })?;
                let backing = resolve_virtual_path(
                    &resolved.path,
                    virtual_events.entry(resolved.workspace_id).or_default(),
                );
                let (backing_path, snapshot) = match backing {
                    VirtualPath::Physical(path) => {
                        let snapshot = workspace
                            .snapshot(&path)
                            .map_err(WorkspaceEditError::Io)?
                            .ok_or_else(|| {
                            WorkspaceEditError::Invalid(format!(
                                "workspace edit target {path} does not exist"
                            ))
                        })?;
                        if snapshot.kind != SecurePathKind::File {
                            return Err(WorkspaceEditError::Invalid(
                                "workspace text edit target must be a regular file".to_owned(),
                            ));
                        }
                        (Some(path), Some(snapshot))
                    }
                    VirtualPath::Created => (None, None),
                    VirtualPath::Missing => {
                        return Err(WorkspaceEditError::Invalid(format!(
                            "workspace edit target {} does not exist",
                            resolved.path
                        )));
                    }
                };
                let open = backing_path
                    .as_deref()
                    .and_then(|path| open_by_path.get(&(resolved.workspace_id, path)).copied());
                if let (Some(version), Some(open)) = (version, open)
                    && version != open.version
                {
                    return Err(WorkspaceEditError::Stale(format!(
                        "open document {} has version {}, not {version}",
                        resolved.path, open.version
                    )));
                }
                let logical = logical_texts
                    .get(&(resolved.workspace_id, resolved.path.clone()))
                    .cloned();
                let (original_text, closed) = match open {
                    Some(open) => (logical.unwrap_or_else(|| open.text.clone()), None),
                    None => {
                        let (content, mode, state) = match snapshot.as_ref() {
                            Some(snapshot) => (
                                snapshot.content.clone().unwrap_or_default(),
                                snapshot.mode,
                                ClosedDocumentState::Original(snapshot.identity),
                            ),
                            None => (String::new(), 0o644, ClosedDocumentState::Pending),
                        };
                        (
                            logical.unwrap_or(content),
                            Some(ClosedDocument {
                                workspace,
                                workspace_root: workspace_roots[&resolved.workspace_id].clone(),
                                original: snapshot,
                                mode,
                                state,
                                tombstone: None,
                                prepared_name: Some(prepared_file_name(operations.len())),
                                prepared: None,
                                operation_phase: JournalOperationPhase::NotStarted,
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
                logical_texts.insert(
                    (resolved.workspace_id, resolved.path.clone()),
                    new_text.clone(),
                );
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
                        WorkspaceEditError::Limit(
                            "workspace edit staged size overflowed".to_owned(),
                        )
                    })?;
                if staged_bytes > MAX_WORKSPACE_EDIT_STAGED_BYTES {
                    return Err(WorkspaceEditError::Limit(format!(
                        "workspace edit exceeds the {MAX_WORKSPACE_EDIT_STAGED_BYTES}-byte staged output limit"
                    )));
                }
                let document = documents.len();
                if closed.is_some() {
                    persisted_texts.insert(
                        (resolved.workspace_id, resolved.path.clone()),
                        new_text.clone(),
                    );
                }
                documents.push(TransactionDocument {
                    workspace_id: resolved.workspace_id,
                    path: resolved.path,
                    original_hash: content_hash(&original_text),
                    original_text,
                    new_text,
                    open: open.map(|open| OpenDocumentIdentity {
                        generation: open.generation,
                        version: open.version,
                    }),
                    open_path: backing_path,
                    closed,
                });
                operations.push(TransactionOperation::TextDocument(document));
                staged_operations.push(StagedWorkspaceEditOperation::TextDocument { document });
            }
            RequestedChange::Create {
                uri,
                overwrite,
                ignore_if_exists,
            } => {
                let resolved = resolve_uri_lexical(&uri, roots)?;
                let workspace =
                    workspaces
                        .get(&resolved.workspace_id)
                        .cloned()
                        .ok_or_else(|| {
                            WorkspaceEditError::Invalid("workspace root disappeared".to_owned())
                        })?;
                let events = virtual_events.entry(resolved.workspace_id).or_default();
                let mut previous = virtual_snapshot(&workspace, &resolved.path, events)?;
                apply_virtual_content(
                    previous.as_mut(),
                    persisted_texts.get(&(resolved.workspace_id, resolved.path.clone())),
                );
                if previous.is_some() && ignore_if_exists && !overwrite {
                    continue;
                }
                if previous.is_some() && !overwrite {
                    return Err(WorkspaceEditError::Invalid(format!(
                        "create destination {} already exists",
                        resolved.path
                    )));
                }
                if previous.is_none()
                    && has_case_collision(roots, resolved.workspace_id, &resolved.path)?
                {
                    return Err(WorkspaceEditError::Invalid(
                        "CreateFile has an unsafe case collision".to_owned(),
                    ));
                }
                if previous
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.kind != SecurePathKind::File)
                {
                    return Err(WorkspaceEditError::Invalid(
                        "CreateFile cannot overwrite a directory".to_owned(),
                    ));
                }
                if previous.is_some()
                    && let VirtualPath::Physical(path) =
                        resolve_virtual_path(&resolved.path, events)
                {
                    reject_dirty_open_documents(
                        resolved.workspace_id,
                        &path,
                        &workspace,
                        open_documents,
                    )?;
                }
                events.push(VirtualEvent::Create(resolved.path.clone()));
                logical_texts.insert(
                    (resolved.workspace_id, resolved.path.clone()),
                    String::new(),
                );
                persisted_texts.insert(
                    (resolved.workspace_id, resolved.path.clone()),
                    String::new(),
                );
                operations.push(TransactionOperation::Resource(Box::new(
                    ResourceOperation {
                        workspace_id: resolved.workspace_id,
                        workspace,
                        workspace_root: workspace_roots[&resolved.workspace_id].clone(),
                        operation_index: operations.len(),
                        phase: JournalOperationPhase::NotStarted,
                        kind: ResourceOperationKind::Create {
                            path: resolved.path.clone(),
                            previous,
                            tombstone: None,
                            prepared_name: Some(prepared_file_name(operations.len())),
                            prepared: None,
                            created: None,
                        },
                    },
                )));
                staged_operations.push(StagedWorkspaceEditOperation::CreateFile {
                    workspace_id: resolved.workspace_id,
                    path: resolved.path,
                });
            }
            RequestedChange::Rename {
                old_uri,
                new_uri,
                overwrite,
                ignore_if_exists,
            } => {
                let old = resolve_uri_lexical(&old_uri, roots)?;
                let new = resolve_uri_lexical(&new_uri, roots)?;
                if old.workspace_id != new.workspace_id {
                    return Err(WorkspaceEditError::Unsupported(
                        "workspace edit renames cannot cross workspaces".to_owned(),
                    ));
                }
                if old.path == new.path || old.path.eq_ignore_ascii_case(&new.path) {
                    return Err(WorkspaceEditError::Invalid(
                        "workspace edit rename has an unsafe case collision".to_owned(),
                    ));
                }
                let workspace = workspaces.get(&old.workspace_id).cloned().ok_or_else(|| {
                    WorkspaceEditError::Invalid("workspace root disappeared".to_owned())
                })?;
                let events = virtual_events.entry(old.workspace_id).or_default();
                let mut source =
                    virtual_snapshot(&workspace, &old.path, events)?.ok_or_else(|| {
                        WorkspaceEditError::Invalid(format!(
                            "rename source {} does not exist",
                            old.path
                        ))
                    })?;
                apply_virtual_content(
                    Some(&mut source),
                    persisted_texts.get(&(old.workspace_id, old.path.clone())),
                );
                if source.kind == SecurePathKind::Directory && path_is_within(&new.path, &old.path)
                {
                    return Err(WorkspaceEditError::Invalid(
                        "workspace edit rename would create a directory cycle".to_owned(),
                    ));
                }
                let mut previous = virtual_snapshot(&workspace, &new.path, events)?;
                apply_virtual_content(
                    previous.as_mut(),
                    persisted_texts.get(&(new.workspace_id, new.path.clone())),
                );
                if previous.is_some() && ignore_if_exists && !overwrite {
                    continue;
                }
                if previous.is_some() && !overwrite {
                    return Err(WorkspaceEditError::Invalid(format!(
                        "rename destination {} already exists",
                        new.path
                    )));
                }
                if previous.is_none() && has_case_collision(roots, new.workspace_id, &new.path)? {
                    return Err(WorkspaceEditError::Invalid(
                        "RenameFile has an unsafe case collision".to_owned(),
                    ));
                }
                if previous.is_some()
                    && let VirtualPath::Physical(path) = resolve_virtual_path(&new.path, events)
                {
                    reject_dirty_open_documents(
                        new.workspace_id,
                        &path,
                        &workspace,
                        open_documents,
                    )?;
                }
                events.push(VirtualEvent::Rename(old.path.clone(), new.path.clone()));
                remap_logical_texts(&mut logical_texts, old.workspace_id, &old.path, &new.path);
                remap_logical_texts(&mut persisted_texts, old.workspace_id, &old.path, &new.path);
                operations.push(TransactionOperation::Resource(Box::new(
                    ResourceOperation {
                        workspace_id: old.workspace_id,
                        workspace,
                        workspace_root: workspace_roots[&old.workspace_id].clone(),
                        operation_index: operations.len(),
                        phase: JournalOperationPhase::NotStarted,
                        kind: ResourceOperationKind::Rename {
                            old_path: old.path.clone(),
                            new_path: new.path.clone(),
                            source,
                            previous,
                            tombstone: None,
                            moved: false,
                        },
                    },
                )));
                staged_operations.push(StagedWorkspaceEditOperation::RenameFile {
                    workspace_id: old.workspace_id,
                    old_path: old.path,
                    new_path: new.path,
                });
            }
            RequestedChange::Delete {
                uri,
                recursive,
                ignore_if_not_exists,
            } => {
                let resolved = resolve_uri_lexical(&uri, roots)?;
                let workspace =
                    workspaces
                        .get(&resolved.workspace_id)
                        .cloned()
                        .ok_or_else(|| {
                            WorkspaceEditError::Invalid("workspace root disappeared".to_owned())
                        })?;
                let events = virtual_events.entry(resolved.workspace_id).or_default();
                let Some(mut snapshot) = virtual_snapshot(&workspace, &resolved.path, events)?
                else {
                    if ignore_if_not_exists {
                        continue;
                    }
                    return Err(WorkspaceEditError::Invalid(format!(
                        "delete target {} does not exist",
                        resolved.path
                    )));
                };
                apply_virtual_content(
                    Some(&mut snapshot),
                    persisted_texts.get(&(resolved.workspace_id, resolved.path.clone())),
                );
                if let VirtualPath::Physical(path) = resolve_virtual_path(&resolved.path, events) {
                    reject_dirty_open_documents(
                        resolved.workspace_id,
                        &path,
                        &workspace,
                        open_documents,
                    )?;
                }
                if snapshot.kind == SecurePathKind::Directory
                    && !recursive
                    && !workspace
                        .directory_empty(&resolved.path)
                        .map_err(WorkspaceEditError::Io)?
                {
                    return Err(WorkspaceEditError::Invalid(
                        "deleting a non-empty directory requires recursive: true".to_owned(),
                    ));
                }
                events.push(VirtualEvent::Delete(resolved.path.clone()));
                logical_texts.retain(|(workspace_id, path), _| {
                    *workspace_id != resolved.workspace_id || !path_is_within(path, &resolved.path)
                });
                persisted_texts.retain(|(workspace_id, path), _| {
                    *workspace_id != resolved.workspace_id || !path_is_within(path, &resolved.path)
                });
                operations.push(TransactionOperation::Resource(Box::new(
                    ResourceOperation {
                        workspace_id: resolved.workspace_id,
                        workspace,
                        workspace_root: workspace_roots[&resolved.workspace_id].clone(),
                        operation_index: operations.len(),
                        phase: JournalOperationPhase::NotStarted,
                        kind: ResourceOperationKind::Delete {
                            path: resolved.path.clone(),
                            snapshot,
                            recursive,
                            tombstone: None,
                        },
                    },
                )));
                staged_operations.push(StagedWorkspaceEditOperation::DeleteFile {
                    workspace_id: resolved.workspace_id,
                    path: resolved.path,
                    recursive,
                });
            }
        }
    }
    if documents.len() > MAX_WORKSPACE_EDIT_DOCUMENTS {
        return Err(WorkspaceEditError::Limit(format!(
            "workspace edit exceeds the {MAX_WORKSPACE_EDIT_DOCUMENTS}-document limit"
        )));
    }
    Ok(PreparedTransaction {
        documents,
        operations,
        staged_operations,
    })
}

struct ResolvedDocument {
    workspace_id: WorkspaceId,
    path: String,
}

fn resolve_uri_lexical(
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
    if requested.components().any(|component| {
        !matches!(
            component,
            std::path::Component::RootDir | std::path::Component::Normal(_)
        )
    }) {
        return Err(WorkspaceEditError::Invalid(
            "workspace edit URI contains traversal or invalid path components".to_owned(),
        ));
    }
    let mut matched = None;
    for root in roots {
        let canonical_root = fs::canonicalize(&root.path)
            .map_err(|error| WorkspaceEditError::Io(error.to_string()))?;
        let Ok(relative) = requested.strip_prefix(&canonical_root) else {
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
        let path = crate::tabs::editor::normalize_path(relative)
            .map_err(|error| WorkspaceEditError::Invalid(error.to_string()))?;
        matched = Some((root.workspace_id, canonical_root.as_os_str().len(), path));
    }
    let (workspace_id, _, path) = matched.ok_or_else(|| {
        WorkspaceEditError::Unsupported("workspace edit path is outside the workspace".to_owned())
    })?;
    Ok(ResolvedDocument { workspace_id, path })
}

fn parse_resource_change(
    kind: &str,
    change: &Value,
) -> Result<RequestedChange, WorkspaceEditError> {
    let options = change.get("options").filter(|value| !value.is_null());
    match kind {
        "create" => {
            let uri = required_string(change, "uri", "CreateFile")?;
            let overwrite = option_bool(options, "overwrite")?;
            let ignore_if_exists = option_bool(options, "ignoreIfExists")?;
            Ok(RequestedChange::Create {
                uri,
                overwrite,
                ignore_if_exists,
            })
        }
        "rename" => {
            let old_uri = required_string(change, "oldUri", "RenameFile")?;
            let new_uri = required_string(change, "newUri", "RenameFile")?;
            let overwrite = option_bool(options, "overwrite")?;
            let ignore_if_exists = option_bool(options, "ignoreIfExists")?;
            Ok(RequestedChange::Rename {
                old_uri,
                new_uri,
                overwrite,
                ignore_if_exists,
            })
        }
        "delete" => Ok(RequestedChange::Delete {
            uri: required_string(change, "uri", "DeleteFile")?,
            recursive: option_bool(options, "recursive")?,
            ignore_if_not_exists: option_bool(options, "ignoreIfNotExists")?,
        }),
        _ => Err(WorkspaceEditError::Unsupported(format!(
            "workspace edit resource operation {kind:?} is not supported"
        ))),
    }
}

fn required_string(
    change: &Value,
    key: &str,
    operation: &str,
) -> Result<String, WorkspaceEditError> {
    change
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| WorkspaceEditError::Invalid(format!("{operation} requires {key}")))
}

fn option_bool(options: Option<&Value>, key: &str) -> Result<bool, WorkspaceEditError> {
    let Some(options) = options else {
        return Ok(false);
    };
    let object = options.as_object().ok_or_else(|| {
        WorkspaceEditError::Invalid("workspace edit resource options must be an object".to_owned())
    })?;
    match object.get(key) {
        None => Ok(false),
        Some(value) => value.as_bool().ok_or_else(|| {
            WorkspaceEditError::Invalid(format!("workspace edit option {key} must be boolean"))
        }),
    }
}

fn resolve_virtual_path(path: &str, events: &[VirtualEvent]) -> VirtualPath {
    let mut path = path.to_owned();
    for event in events.iter().rev() {
        match event {
            VirtualEvent::Create(created) if path == *created => return VirtualPath::Created,
            VirtualEvent::Delete(deleted) if path_is_within(&path, deleted) => {
                return VirtualPath::Missing;
            }
            VirtualEvent::Rename(source, destination) => {
                if let Some(suffix) = path_suffix(&path, destination) {
                    path = join_prefix(source, suffix);
                } else if path_is_within(&path, source) {
                    return VirtualPath::Missing;
                }
            }
            _ => {}
        }
    }
    VirtualPath::Physical(path)
}

fn virtual_snapshot(
    workspace: &SecureWorkspace,
    path: &str,
    events: &[VirtualEvent],
) -> Result<Option<SecurePathSnapshot>, WorkspaceEditError> {
    match resolve_virtual_path(path, events) {
        VirtualPath::Physical(path) => workspace.snapshot(&path).map_err(WorkspaceEditError::Io),
        VirtualPath::Created => Ok(Some(SecurePathSnapshot {
            identity: FileIdentity {
                device: 0,
                inode: 0,
            },
            kind: SecurePathKind::File,
            mode: 0o644,
            content: Some(String::new()),
            fingerprint: content_hash(""),
        })),
        VirtualPath::Missing => Ok(None),
    }
}

fn remap_logical_texts(
    texts: &mut HashMap<(WorkspaceId, String), String>,
    workspace_id: WorkspaceId,
    source: &str,
    destination: &str,
) {
    let moved = texts
        .iter()
        .filter(|((current_workspace, path), _)| {
            *current_workspace == workspace_id && path_is_within(path, source)
        })
        .map(|((_, path), text)| {
            let suffix = path_suffix(path, source).unwrap_or_default();
            (path.clone(), join_prefix(destination, suffix), text.clone())
        })
        .collect::<Vec<_>>();
    for (source, destination, text) in moved {
        texts.remove(&(workspace_id, source));
        texts.insert((workspace_id, destination), text);
    }
}

fn apply_virtual_content(snapshot: Option<&mut SecurePathSnapshot>, content: Option<&String>) {
    let (Some(snapshot), Some(content)) = (snapshot, content) else {
        return;
    };
    if snapshot.kind == SecurePathKind::File {
        snapshot.identity = FileIdentity {
            device: 0,
            inode: 0,
        };
        snapshot.content = Some(content.clone());
        snapshot.fingerprint = content_hash(content);
    }
}

fn reject_dirty_open_documents(
    workspace_id: WorkspaceId,
    path: &str,
    workspace: &SecureWorkspace,
    open_documents: &[WorkspaceEditOpenDocument],
) -> Result<(), WorkspaceEditError> {
    for document in open_documents.iter().filter(|document| {
        document.workspace_id == workspace_id && path_is_within(&document.path, path)
    }) {
        let snapshot = workspace
            .snapshot(&document.path)
            .map_err(WorkspaceEditError::Io)?
            .ok_or_else(|| {
                WorkspaceEditError::Stale(format!(
                    "open document {} disappeared before resource deletion",
                    document.path
                ))
            })?;
        if snapshot.kind != SecurePathKind::File
            || snapshot.content.as_deref() != Some(document.text.as_str())
        {
            return Err(WorkspaceEditError::Stale(format!(
                "cannot replace or delete dirty open document {}",
                document.path
            )));
        }
    }
    Ok(())
}

fn path_is_within(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn path_suffix<'a>(path: &'a str, parent: &str) -> Option<&'a str> {
    if path == parent {
        Some("")
    } else {
        path.strip_prefix(parent)?.strip_prefix('/')
    }
}

fn join_prefix(prefix: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}/{suffix}")
    }
}

fn has_case_collision(
    roots: &[WorkspaceEditRoot],
    workspace_id: WorkspaceId,
    path: &str,
) -> Result<bool, WorkspaceEditError> {
    let root = roots
        .iter()
        .find(|root| root.workspace_id == workspace_id)
        .ok_or_else(|| WorkspaceEditError::Invalid("workspace root disappeared".to_owned()))?;
    let path = std::path::Path::new(path);
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(WorkspaceEditError::Invalid(
            "workspace edit path is not valid UTF-8".to_owned(),
        ));
    };
    let parent = root
        .path
        .join(path.parent().unwrap_or_else(|| std::path::Path::new("")));
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(WorkspaceEditError::Io(error.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| WorkspaceEditError::Io(error.to_string()))?;
        let entry = entry.file_name();
        let Some(entry) = entry.to_str() else {
            continue;
        };
        if entry != name && entry.eq_ignore_ascii_case(name) {
            return Ok(true);
        }
    }
    Ok(false)
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

pub(crate) fn apply_document_text_edits(
    text: &str,
    edits: &[LanguageServerTextEdit],
) -> Result<String, WorkspaceEditError> {
    let spans = validate_and_span_edits(text, edits)?;
    Ok(apply_edits(text, edits, spans))
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
    let mut validated = HashSet::new();
    for document in documents {
        let Some(expected) = document.open else {
            continue;
        };
        let key = (document.workspace_id, document.open_path.as_deref());
        if !validated.insert(key) {
            continue;
        }
        let current = open_documents.iter().find(|open| {
            open.workspace_id == document.workspace_id
                && document.open_path.as_deref() == Some(open.path.as_str())
        });
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

fn apply_closed_document(
    document: &mut TransactionDocument,
    token: &str,
    operation_index: usize,
) -> Result<(), WorkspaceEditError> {
    let closed = document.closed.as_mut().ok_or_else(|| {
        WorkspaceEditError::Invalid("workspace edit target is not a closed file".to_owned())
    })?;
    let snapshot = match closed.state {
        ClosedDocumentState::Pending => {
            let snapshot = closed
                .workspace
                .snapshot(&document.path)
                .map_err(WorkspaceEditError::Io)?
                .ok_or_else(|| {
                    WorkspaceEditError::Stale(format!("{} was not created", document.path))
                })?;
            if snapshot.kind != SecurePathKind::File
                || snapshot.content.as_deref() != Some(document.original_text.as_str())
            {
                return Err(WorkspaceEditError::Stale(format!(
                    "{} changed after creation",
                    document.path
                )));
            }
            closed.original = Some(snapshot.clone());
            closed.mode = snapshot.mode;
            closed.state = ClosedDocumentState::Original(snapshot.identity);
            snapshot
        }
        ClosedDocumentState::Original(_) => {
            let snapshot = closed
                .workspace
                .snapshot(&document.path)
                .map_err(WorkspaceEditError::Io)?
                .ok_or_else(|| {
                    WorkspaceEditError::Stale(format!("{} disappeared", document.path))
                })?;
            if !closed
                .original
                .as_ref()
                .is_some_and(|original| snapshot_matches(original, &snapshot))
            {
                return Err(WorkspaceEditError::Stale(format!(
                    "{} changed before commit",
                    document.path
                )));
            }
            closed.mode = snapshot.mode;
            snapshot
        }
        ClosedDocumentState::Applied(_) => {
            return Err(WorkspaceEditError::Invalid(
                "workspace edit target was already applied".to_owned(),
            ));
        }
    };
    match closed
        .workspace
        .stage_remove(&document.path, &snapshot, token, operation_index)
    {
        Ok(tombstone) => closed.tombstone = Some(tombstone),
        Err(error) => {
            if let SecureMutationOutcome::Applied(tombstone) = error.outcome {
                closed.tombstone = Some(*tombstone);
            }
            return Err(WorkspaceEditError::Io(error.message));
        }
    }
    let prepared = closed.prepared.ok_or_else(|| {
        WorkspaceEditError::Recovery("prepared text edit data disappeared".to_owned())
    })?;
    match closed
        .workspace
        .install_prepared_file(&document.path, token, operation_index, prepared)
    {
        Ok(applied) => {
            closed.state = ClosedDocumentState::Applied(applied);
            Ok(())
        }
        Err(error) => {
            if let SecureMutationOutcome::Applied(applied) = error.outcome {
                closed.state = ClosedDocumentState::Applied(*applied);
            }
            if let Some(tombstone) = closed.tombstone.as_ref()
                && closed.workspace.restore(&document.path, tombstone).is_ok()
            {
                closed.tombstone = None;
            }
            Err(WorkspaceEditError::Io(error.message))
        }
    }
}

fn prepare_closed_document(
    document: &mut TransactionDocument,
    token: &str,
    operation_index: usize,
) -> Result<bool, WorkspaceEditError> {
    let Some(closed) = document.closed.as_mut() else {
        return Ok(false);
    };
    if closed.prepared.is_some() || matches!(closed.state, ClosedDocumentState::Applied(_)) {
        return Ok(false);
    }
    match closed
        .workspace
        .prepare_file(token, operation_index, closed.mode, &document.new_text)
    {
        Ok(identity) => {
            closed.prepared = Some(identity);
            Ok(true)
        }
        Err(error) => {
            if let SecureMutationOutcome::Applied(identity) = error.outcome {
                closed.prepared = Some(*identity);
            }
            Err(WorkspaceEditError::Io(error.message))
        }
    }
}

fn rollback_applied(
    operations: &mut [TransactionOperation],
    documents: &mut [TransactionDocument],
    token: &str,
) -> Result<(), WorkspaceEditError> {
    let mut errors = Vec::new();
    for (operation_index, operation) in operations.iter_mut().enumerate().rev() {
        if let TransactionOperation::Resource(operation) = operation {
            if let Err(error) = rollback_resource_operation(operation, token) {
                errors.push(error.to_string());
            }
            continue;
        }
        let TransactionOperation::TextDocument(index) = operation else {
            unreachable!()
        };
        let document = &mut documents[*index];
        let Some(closed) = document.closed.as_mut() else {
            continue;
        };
        let rollback = (|| {
            if let ClosedDocumentState::Applied(identity) = closed.state {
                let snapshot = closed
                    .workspace
                    .snapshot(&document.path)?
                    .ok_or_else(|| "workspace edit target disappeared".to_owned())?;
                if snapshot.identity != identity
                    || snapshot.content.as_deref() != Some(document.new_text.as_str())
                {
                    return Err("workspace edit target changed before rollback".to_owned());
                }
                closed.workspace.remove_created(&document.path, identity)?;
                let tombstone = closed
                    .tombstone
                    .as_ref()
                    .ok_or_else(|| "workspace edit text backup disappeared".to_owned())?;
                closed.workspace.restore(&document.path, tombstone)?;
            }
            if let Some(prepared) = closed.prepared {
                closed.workspace.remove_prepared_file_if_matches(
                    token,
                    operation_index,
                    prepared,
                )?;
            }
            Ok(())
        })();
        match rollback {
            Ok(()) => {
                closed.tombstone = None;
                closed.prepared = None;
                closed.state = closed
                    .original
                    .as_ref()
                    .map(|snapshot| ClosedDocumentState::Original(snapshot.identity))
                    .unwrap_or(ClosedDocumentState::Pending);
            }
            Err(error) => {
                errors.push(format!("{}: {error}", document.path));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(WorkspaceEditError::Io(errors.join("; ")))
    }
}

fn apply_resource_operation(
    operation: &mut ResourceOperation,
    token: &str,
    operation_index: usize,
) -> Result<(), WorkspaceEditError> {
    match &mut operation.kind {
        ResourceOperationKind::Create {
            path,
            previous,
            tombstone,
            prepared,
            created,
            ..
        } => {
            if created.is_some() {
                return Ok(());
            }
            if let Some(previous) = previous {
                let current_previous = operation
                    .workspace
                    .snapshot(path)
                    .map_err(WorkspaceEditError::Io)?
                    .ok_or_else(|| {
                        WorkspaceEditError::Stale(format!(
                            "create overwrite target {path} disappeared"
                        ))
                    })?;
                if !snapshot_matches(previous, &current_previous) {
                    return Err(WorkspaceEditError::Stale(format!(
                        "create overwrite target {path} changed before commit"
                    )));
                }
                match operation.workspace.stage_remove(
                    path,
                    &current_previous,
                    token,
                    operation_index,
                ) {
                    Ok(staged) => *tombstone = Some(staged),
                    Err(error) => {
                        if let SecureMutationOutcome::Applied(staged) = error.outcome {
                            *tombstone = Some(*staged);
                        }
                        return Err(WorkspaceEditError::Io(error.message));
                    }
                }
            }
            let prepared_identity = prepared.ok_or_else(|| {
                WorkspaceEditError::Recovery("prepared CreateFile data disappeared".to_owned())
            })?;
            match operation.workspace.install_prepared_file(
                path,
                token,
                operation_index,
                prepared_identity,
            ) {
                Ok(identity) => {
                    *created = Some(identity);
                    Ok(())
                }
                Err(error) => {
                    if let SecureMutationOutcome::Applied(identity) = error.outcome {
                        *created = Some(*identity);
                    }
                    if let Some(staged) = tombstone.as_ref()
                        && operation.workspace.restore(path, staged).is_ok()
                    {
                        *tombstone = None;
                    }
                    Err(WorkspaceEditError::Io(error.message))
                }
            }
        }
        ResourceOperationKind::Rename {
            old_path,
            new_path,
            source,
            previous,
            tombstone,
            moved,
        } => {
            if *moved {
                return Ok(());
            }
            let current = operation
                .workspace
                .snapshot(old_path)
                .map_err(WorkspaceEditError::Io)?
                .ok_or_else(|| {
                    WorkspaceEditError::Stale(format!("rename source {old_path} disappeared"))
                })?;
            if !snapshot_matches(source, &current) {
                return Err(WorkspaceEditError::Stale(format!(
                    "rename source {old_path} changed before commit"
                )));
            }
            if let Some(previous) = previous {
                let current_previous = operation
                    .workspace
                    .snapshot(new_path)
                    .map_err(WorkspaceEditError::Io)?
                    .ok_or_else(|| {
                        WorkspaceEditError::Stale(format!(
                            "rename destination {new_path} disappeared"
                        ))
                    })?;
                if !snapshot_matches(previous, &current_previous) {
                    return Err(WorkspaceEditError::Stale(format!(
                        "rename destination {new_path} changed before commit"
                    )));
                }
                match operation.workspace.stage_remove(
                    new_path,
                    &current_previous,
                    token,
                    operation_index,
                ) {
                    Ok(staged) => *tombstone = Some(staged),
                    Err(error) => {
                        if let SecureMutationOutcome::Applied(staged) = error.outcome {
                            *tombstone = Some(*staged);
                        }
                        return Err(WorkspaceEditError::Io(error.message));
                    }
                }
            }
            match operation
                .workspace
                .rename(old_path, new_path, current.identity)
            {
                Ok(_) => {
                    *moved = true;
                    Ok(())
                }
                Err(error) => {
                    if matches!(error.outcome, SecureMutationOutcome::Applied(_)) {
                        *moved = true;
                    }
                    if let Some(staged) = tombstone.as_ref()
                        && operation.workspace.restore(new_path, staged).is_ok()
                    {
                        *tombstone = None;
                    }
                    Err(WorkspaceEditError::Io(error.message))
                }
            }
        }
        ResourceOperationKind::Delete {
            path,
            snapshot,
            tombstone,
            recursive,
        } => {
            if tombstone.is_none() {
                let current = operation
                    .workspace
                    .snapshot(path)
                    .map_err(WorkspaceEditError::Io)?
                    .ok_or_else(|| {
                        WorkspaceEditError::Stale(format!("delete target {path} disappeared"))
                    })?;
                if !snapshot_matches(snapshot, &current) {
                    return Err(WorkspaceEditError::Stale(format!(
                        "delete target {path} changed before commit"
                    )));
                }
                if snapshot.kind == SecurePathKind::Directory
                    && !*recursive
                    && !operation
                        .workspace
                        .directory_empty(path)
                        .map_err(WorkspaceEditError::Io)?
                {
                    return Err(WorkspaceEditError::Stale(
                        "delete directory became non-empty before commit".to_owned(),
                    ));
                }
                match operation
                    .workspace
                    .stage_remove(path, &current, token, operation_index)
                {
                    Ok(staged) => *tombstone = Some(staged),
                    Err(error) => {
                        if let SecureMutationOutcome::Applied(staged) = error.outcome {
                            *tombstone = Some(*staged);
                        }
                        return Err(WorkspaceEditError::Io(error.message));
                    }
                }
            }
            Ok(())
        }
    }
}

fn prepare_create_operation(
    operation: &mut ResourceOperation,
    token: &str,
) -> Result<bool, WorkspaceEditError> {
    let ResourceOperationKind::Create {
        prepared, created, ..
    } = &mut operation.kind
    else {
        return Ok(false);
    };
    if prepared.is_some() || created.is_some() {
        return Ok(false);
    }
    match operation
        .workspace
        .prepare_file(token, operation.operation_index, 0o644, "")
    {
        Ok(identity) => {
            *prepared = Some(identity);
            Ok(true)
        }
        Err(error) => {
            if let SecureMutationOutcome::Applied(identity) = error.outcome {
                *prepared = Some(*identity);
            }
            Err(WorkspaceEditError::Io(error.message))
        }
    }
}

fn rollback_resource_operation(
    operation: &mut ResourceOperation,
    token: &str,
) -> Result<(), WorkspaceEditError> {
    match &mut operation.kind {
        ResourceOperationKind::Create {
            path,
            tombstone,
            prepared,
            created,
            ..
        } => {
            if let Some(created_identity) = *created {
                let current = operation
                    .workspace
                    .snapshot(path)
                    .map_err(WorkspaceEditError::Io)?
                    .ok_or_else(|| {
                        WorkspaceEditError::Stale(format!("created file {path} disappeared"))
                    })?;
                if current.identity != created_identity
                    || current.kind != SecurePathKind::File
                    || current.content.as_deref() != Some("")
                {
                    return Err(WorkspaceEditError::Stale(format!(
                        "created file {path} changed before rollback"
                    )));
                }
                operation
                    .workspace
                    .remove_created(path, created_identity)
                    .map_err(WorkspaceEditError::Io)?;
                *created = None;
            }
            if let Some(identity) = prepared.as_ref() {
                operation
                    .workspace
                    .remove_prepared_file_if_matches(token, operation.operation_index, *identity)
                    .map_err(WorkspaceEditError::Io)?;
                *prepared = None;
            }
            if let Some(staged) = tombstone.as_ref() {
                operation
                    .workspace
                    .restore(path, staged)
                    .map_err(WorkspaceEditError::Io)?;
                *tombstone = None;
            }
        }
        ResourceOperationKind::Rename {
            old_path,
            new_path,
            source,
            tombstone,
            moved,
            ..
        } => {
            if *moved {
                let current = operation
                    .workspace
                    .snapshot(new_path)
                    .map_err(WorkspaceEditError::Io)?
                    .ok_or_else(|| {
                        WorkspaceEditError::Stale(format!("renamed file {new_path} disappeared"))
                    })?;
                if !snapshot_matches(source, &current) {
                    return Err(WorkspaceEditError::Stale(format!(
                        "renamed target {new_path} changed before rollback"
                    )));
                }
                operation
                    .workspace
                    .rename(new_path, old_path, current.identity)
                    .map_err(|error| WorkspaceEditError::Io(error.message))?;
                *moved = false;
            }
            if let Some(staged) = tombstone.as_ref() {
                operation
                    .workspace
                    .restore(new_path, staged)
                    .map_err(WorkspaceEditError::Io)?;
                *tombstone = None;
            }
        }
        ResourceOperationKind::Delete {
            path, tombstone, ..
        } => {
            if let Some(staged) = tombstone.as_ref() {
                operation
                    .workspace
                    .restore(path, staged)
                    .map_err(WorkspaceEditError::Io)?;
                *tombstone = None;
            }
        }
    }
    Ok(())
}

fn finish_resources(
    operations: &mut [TransactionOperation],
    documents: &mut [TransactionDocument],
) -> Result<(), WorkspaceEditError> {
    for operation in operations.iter_mut().rev() {
        if let TransactionOperation::TextDocument(index) = operation {
            let Some(closed) = documents[*index].closed.as_mut() else {
                continue;
            };
            if let Some(tombstone) = closed.tombstone.as_ref() {
                closed
                    .workspace
                    .discard(tombstone, false)
                    .map_err(WorkspaceEditError::Io)?;
                closed.tombstone = None;
            }
            continue;
        }
        let TransactionOperation::Resource(operation) = operation else {
            unreachable!()
        };
        match &mut operation.kind {
            ResourceOperationKind::Create { tombstone, .. }
            | ResourceOperationKind::Rename { tombstone, .. } => {
                if let Some(staged) = tombstone.as_ref() {
                    operation
                        .workspace
                        .discard(staged, true)
                        .map_err(WorkspaceEditError::Io)?;
                }
                *tombstone = None;
            }
            ResourceOperationKind::Delete {
                recursive,
                tombstone,
                ..
            } => {
                if let Some(staged) = tombstone.as_ref() {
                    operation
                        .workspace
                        .discard(staged, *recursive)
                        .map_err(WorkspaceEditError::Io)?;
                }
                *tombstone = None;
            }
        }
    }
    Ok(())
}

fn snapshot_matches(expected: &SecurePathSnapshot, current: &SecurePathSnapshot) -> bool {
    if expected.kind != current.kind {
        return false;
    }
    if expected.identity.device == 0 && expected.identity.inode == 0 {
        return expected.content == current.content;
    }
    expected.identity == current.identity && expected.fingerprint == current.fingerprint
}

fn validate_and_refresh_resource_snapshot(
    operation: &mut ResourceOperation,
) -> Result<(), WorkspaceEditError> {
    let workspace = Arc::clone(&operation.workspace);
    let snapshot = |path: &str| workspace.snapshot(path).map_err(WorkspaceEditError::Io);
    let validate = |path: &str,
                    expected: &Option<SecurePathSnapshot>,
                    current: &Option<SecurePathSnapshot>| {
        let matches = match (expected, current) {
            (None, None) => true,
            (Some(expected), Some(current)) => snapshot_matches(expected, current),
            _ => false,
        };
        if matches {
            Ok(())
        } else {
            Err(WorkspaceEditError::Stale(format!(
                "ordered resource target {path} changed before its commit turn"
            )))
        }
    };
    match &mut operation.kind {
        ResourceOperationKind::Create { path, previous, .. } => {
            let current = snapshot(path)?;
            validate(path, previous, &current)?;
            *previous = current;
        }
        ResourceOperationKind::Rename {
            old_path,
            new_path,
            source,
            previous,
            ..
        } => {
            let current_source = snapshot(old_path)?;
            validate(old_path, &Some(source.clone()), &current_source)?;
            *source = current_source.expect("validated rename source should exist");
            let current_destination = snapshot(new_path)?;
            validate(new_path, previous, &current_destination)?;
            *previous = current_destination;
        }
        ResourceOperationKind::Delete { path, snapshot, .. } => {
            let current = operation
                .workspace
                .snapshot(path)
                .map_err(WorkspaceEditError::Io)?;
            validate(path, &Some(snapshot.clone()), &current)?;
            *snapshot = current.expect("validated delete target should exist");
        }
    }
    Ok(())
}

fn refresh_transformed_resource_snapshots(
    operations: &mut [TransactionOperation],
    documents: &[TransactionDocument],
    applied: usize,
) -> Result<(), WorkspaceEditError> {
    #[cfg(test)]
    if FAIL_NEXT_RESOURCE_REFRESH.replace(false) {
        return Err(WorkspaceEditError::Io(
            "injected ordered resource refresh failure".to_owned(),
        ));
    }
    let changed = match &operations[applied] {
        TransactionOperation::TextDocument(document) => vec![documents[*document].path.clone()],
        TransactionOperation::Resource(operation) => match &operation.kind {
            ResourceOperationKind::Create { path, .. }
            | ResourceOperationKind::Delete { path, .. } => vec![path.clone()],
            ResourceOperationKind::Rename {
                old_path, new_path, ..
            } => vec![old_path.clone(), new_path.clone()],
        },
    };
    for operation in &mut operations[applied + 1..] {
        let TransactionOperation::Resource(operation) = operation else {
            continue;
        };
        let affected = |path: &str| {
            changed
                .iter()
                .any(|changed| changed != path && path_is_within(changed, path))
        };
        let workspace = Arc::clone(&operation.workspace);
        match &mut operation.kind {
            ResourceOperationKind::Create { path, previous, .. } if affected(path) => {
                *previous = workspace.snapshot(path).map_err(WorkspaceEditError::Io)?;
            }
            ResourceOperationKind::Rename {
                old_path,
                new_path,
                source,
                previous,
                ..
            } => {
                if affected(old_path)
                    && let Some(current) = workspace
                        .snapshot(old_path)
                        .map_err(WorkspaceEditError::Io)?
                {
                    *source = current;
                }
                if affected(new_path) {
                    *previous = workspace
                        .snapshot(new_path)
                        .map_err(WorkspaceEditError::Io)?;
                }
            }
            ResourceOperationKind::Delete { path, snapshot, .. } if affected(path) => {
                if let Some(current) = workspace.snapshot(path).map_err(WorkspaceEditError::Io)? {
                    *snapshot = current;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_future_ancestor_snapshots(
    operations: &[TransactionOperation],
    documents: &[TransactionDocument],
    applied: usize,
) -> Result<(), WorkspaceEditError> {
    let changed = match &operations[applied] {
        TransactionOperation::TextDocument(document) => vec![documents[*document].path.as_str()],
        TransactionOperation::Resource(operation) => match &operation.kind {
            ResourceOperationKind::Create { path, .. }
            | ResourceOperationKind::Delete { path, .. } => vec![path.as_str()],
            ResourceOperationKind::Rename {
                old_path, new_path, ..
            } => vec![old_path.as_str(), new_path.as_str()],
        },
    };
    for operation in &operations[applied + 1..] {
        let TransactionOperation::Resource(operation) = operation else {
            continue;
        };
        let ancestor = |path: &str| {
            changed
                .iter()
                .any(|changed| *changed != path && path_is_within(changed, path))
        };
        let validate = |path: &str, expected: Option<&SecurePathSnapshot>| {
            if !ancestor(path) {
                return Ok(());
            }
            let current = operation
                .workspace
                .snapshot(path)
                .map_err(WorkspaceEditError::Io)?;
            let matches = match (expected, current.as_ref()) {
                (None, None) => true,
                (Some(expected), Some(current)) => snapshot_matches(expected, current),
                _ => false,
            };
            if matches {
                Ok(())
            } else {
                Err(WorkspaceEditError::Stale(format!(
                    "ordered ancestor {path} changed before a child operation"
                )))
            }
        };
        match &operation.kind {
            ResourceOperationKind::Create { path, previous, .. } => {
                validate(path, previous.as_ref())?
            }
            ResourceOperationKind::Rename {
                old_path,
                new_path,
                source,
                previous,
                ..
            } => {
                validate(old_path, Some(source))?;
                validate(new_path, previous.as_ref())?;
            }
            ResourceOperationKind::Delete { path, snapshot, .. } => validate(path, Some(snapshot))?,
        }
    }
    Ok(())
}

fn staged_operations(operations: &[TransactionOperation]) -> Vec<StagedWorkspaceEditOperation> {
    operations
        .iter()
        .map(|operation| match operation {
            TransactionOperation::TextDocument(document) => {
                StagedWorkspaceEditOperation::TextDocument {
                    document: *document,
                }
            }
            TransactionOperation::Resource(operation) => match &operation.kind {
                ResourceOperationKind::Create { path, .. } => {
                    StagedWorkspaceEditOperation::CreateFile {
                        workspace_id: operation.workspace_id,
                        path: path.clone(),
                    }
                }
                ResourceOperationKind::Rename {
                    old_path, new_path, ..
                } => StagedWorkspaceEditOperation::RenameFile {
                    workspace_id: operation.workspace_id,
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                },
                ResourceOperationKind::Delete {
                    path, recursive, ..
                } => StagedWorkspaceEditOperation::DeleteFile {
                    workspace_id: operation.workspace_id,
                    path: path.clone(),
                    recursive: *recursive,
                },
            },
        })
        .collect()
}

fn save_journal(
    store: Option<&crate::persistence::StateStore>,
    transaction_id: u64,
    transaction: &WorkspaceEditTransaction,
    phase: JournalPhase,
) -> Result<(), WorkspaceEditError> {
    let Some(store) = store else {
        return Ok(());
    };
    let journal = workspace_edit_journal(transaction, phase);
    persist_journal(store, transaction_id, &journal)
}

fn workspace_edit_journal(
    transaction: &WorkspaceEditTransaction,
    phase: JournalPhase,
) -> WorkspaceEditJournal {
    WorkspaceEditJournal {
        authorization: transaction.authorization.clone(),
        phase,
        operations: transaction
            .operations
            .iter()
            .map(|operation| match operation {
                TransactionOperation::TextDocument(index) => {
                    let document = &transaction.documents[*index];
                    let closed = document.closed.as_ref();
                    JournalOperation::Text {
                        workspace_id: document.workspace_id.value(),
                        document: *index,
                        root: closed
                            .map(|closed| closed.workspace_root.clone())
                            .unwrap_or_default(),
                        path: document.path.clone(),
                        original: closed.and_then(|closed| closed.original.clone()),
                        prepared_name: closed.and_then(|closed| closed.prepared_name.clone()),
                        prepared: closed.and_then(|closed| closed.prepared),
                        installed: closed.and_then(|closed| match closed.state {
                            ClosedDocumentState::Applied(identity) => Some(identity),
                            _ => None,
                        }),
                        installed_hash: content_hash(&document.new_text),
                        phase: closed
                            .map(|closed| closed.operation_phase)
                            .unwrap_or(JournalOperationPhase::Applied),
                        cleanup: JournalCleanupPhase::Pending,
                        rollback: JournalRollbackPhase::Pending,
                    }
                }
                TransactionOperation::Resource(operation) => match &operation.kind {
                    ResourceOperationKind::Create {
                        path,
                        previous,
                        prepared_name,
                        prepared,
                        created,
                        ..
                    } => JournalOperation::Create {
                        workspace_id: operation.workspace_id.value(),
                        root: operation.workspace_root.clone(),
                        path: path.clone(),
                        previous: previous.clone(),
                        prepared_name: prepared_name.clone(),
                        prepared: *prepared,
                        installed: *created,
                        installed_hash: content_hash(""),
                        phase: operation.phase,
                        cleanup: JournalCleanupPhase::Pending,
                        rollback: JournalRollbackPhase::Pending,
                    },
                    ResourceOperationKind::Rename {
                        old_path,
                        new_path,
                        source,
                        previous,
                        moved,
                        ..
                    } => JournalOperation::Rename {
                        workspace_id: operation.workspace_id.value(),
                        root: operation.workspace_root.clone(),
                        old_path: old_path.clone(),
                        new_path: new_path.clone(),
                        source: source.clone(),
                        previous: previous.clone(),
                        moved: *moved,
                        phase: operation.phase,
                        cleanup: JournalCleanupPhase::Pending,
                        rollback: JournalRollbackPhase::Pending,
                    },
                    ResourceOperationKind::Delete {
                        path,
                        snapshot,
                        recursive,
                        tombstone,
                    } => JournalOperation::Delete {
                        workspace_id: operation.workspace_id.value(),
                        root: operation.workspace_root.clone(),
                        path: path.clone(),
                        snapshot: snapshot.clone(),
                        recursive: *recursive,
                        removed: tombstone.is_some(),
                        phase: operation.phase,
                        cleanup: JournalCleanupPhase::Pending,
                        rollback: JournalRollbackPhase::Pending,
                    },
                },
            })
            .collect(),
    }
}

fn delete_journal(
    store: Option<&crate::persistence::StateStore>,
    transaction_id: u64,
) -> Result<(), WorkspaceEditError> {
    let Some(store) = store else {
        return Ok(());
    };
    store
        .delete_workspace_edit_journal(transaction_id)
        .map_err(|error| {
            WorkspaceEditError::Recovery(format!(
                "could not remove completed workspace edit journal: {error}"
            ))
        })
}

fn remove_recovery_directories(
    transaction: &WorkspaceEditTransaction,
) -> Result<(), WorkspaceEditError> {
    let mut workspaces = HashMap::<PathBuf, Arc<SecureWorkspace>>::new();
    for operation in &transaction.operations {
        match operation {
            TransactionOperation::TextDocument(index) => {
                if let Some(closed) = transaction.documents[*index].closed.as_ref() {
                    workspaces
                        .entry(closed.workspace_root.clone())
                        .or_insert_with(|| Arc::clone(&closed.workspace));
                }
            }
            TransactionOperation::Resource(operation) => {
                workspaces
                    .entry(operation.workspace_root.clone())
                    .or_insert_with(|| Arc::clone(&operation.workspace));
            }
        }
    }
    for workspace in workspaces.values() {
        workspace
            .remove_recovery_directory(&transaction.authorization)
            .map_err(WorkspaceEditError::Io)?;
    }
    Ok(())
}

fn recover_workspace_edit_journals(
    store: &crate::persistence::StateStore,
) -> Result<(), WorkspaceEditError> {
    let existing_outcomes = load_workspace_edit_outcomes(store)?;
    let journals = store.workspace_edit_journals().map_err(|error| {
        WorkspaceEditError::Recovery(format!("could not load workspace edit journals: {error}"))
    })?;
    for (transaction_id, encoded) in journals {
        let mut journal: WorkspaceEditJournal =
            serde_json::from_str(&encoded).map_err(|error| {
                WorkspaceEditError::Recovery(format!(
                    "workspace edit journal {transaction_id} is invalid: {error}"
                ))
            })?;
        let commit_decided = journal.phase == JournalPhase::FinishingCommitted;
        let resolution = match journal.phase {
            JournalPhase::FinishingCommitted => {
                cleanup_committed_journal(store, transaction_id, &mut journal)
                    .and_then(|()| {
                        store
                            .delete_workspace_edit_editor_recovery(transaction_id)
                            .map_err(|error| {
                                WorkspaceEditError::Recovery(format!(
                                    "could not finalize recovered editor state: {error}"
                                ))
                            })
                    })
                    .map(|()| WorkspaceEditTransactionPhase::FinishedCommitted)
            }
            JournalPhase::Applying
            | JournalPhase::Applied
            | JournalPhase::RecoveryRequired
            | JournalPhase::RollingBack => {
                rollback_recovered_journal(store, transaction_id, &mut journal)
                    .and_then(|()| {
                        store
                            .restore_workspace_edit_editor_recovery(transaction_id)
                            .map_err(|error| {
                                WorkspaceEditError::Recovery(format!(
                                    "could not restore recovered editor state: {error}"
                                ))
                            })
                    })
                    .and_then(|()| cleanup_recovery_directories(&journal))
                    .map(|()| WorkspaceEditTransactionPhase::FinishedRolledBack)
            }
            JournalPhase::RolledBack => store
                .restore_workspace_edit_editor_recovery(transaction_id)
                .map_err(|error| {
                    WorkspaceEditError::Recovery(format!(
                        "could not restore rolled-back editor state: {error}"
                    ))
                })
                .and_then(|()| cleanup_recovery_directories(&journal))
                .map(|()| WorkspaceEditTransactionPhase::FinishedRolledBack),
        };
        let resolved_phase = match resolution {
            Ok(phase) => phase,
            Err(_) => {
                let recovery_phase = if commit_decided {
                    WorkspaceEditTransactionPhase::CommittedCleanupRequired
                } else {
                    WorkspaceEditTransactionPhase::RecoveryRequired
                };
                let mut outcome = workspace_edit_outcome(&journal, recovery_phase);
                if let Some(existing) = existing_outcomes.get(&transaction_id)
                    && existing.phase == recovery_phase
                {
                    outcome.created_at = existing.created_at;
                }
                persist_outcome(store, transaction_id, &outcome)?;
                continue;
            }
        };
        let outcome = workspace_edit_outcome(&journal, resolved_phase);
        persist_outcome(store, transaction_id, &outcome)?;
        store
            .delete_workspace_edit_journal(transaction_id)
            .map_err(|error| {
                WorkspaceEditError::Recovery(format!(
                    "could not remove recovered workspace edit journal {transaction_id}: {error}"
                ))
            })?;
    }
    Ok(())
}

fn rollback_recovered_journal(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
    journal: &mut WorkspaceEditJournal,
) -> Result<(), WorkspaceEditError> {
    journal.phase = JournalPhase::RollingBack;
    persist_journal(store, transaction_id, journal)?;
    for index in (0..journal.operations.len()).rev() {
        loop {
            let phase = journal.operations[index].rollback_phase();
            match phase {
                JournalRollbackPhase::Pending => {
                    journal.operations[index]
                        .set_rollback_phase(JournalRollbackPhase::RemovingApplied);
                    persist_journal(store, transaction_id, journal)?;
                }
                JournalRollbackPhase::RemovingApplied => {
                    remove_recovered_applied(
                        &journal.authorization,
                        index,
                        &journal.operations[index],
                    )?;
                    journal.operations[index]
                        .set_rollback_phase(JournalRollbackPhase::AppliedRemoved);
                    persist_journal(store, transaction_id, journal)?;
                }
                JournalRollbackPhase::AppliedRemoved => {
                    journal.operations[index]
                        .set_rollback_phase(JournalRollbackPhase::RestoringBackup);
                    persist_journal(store, transaction_id, journal)?;
                }
                JournalRollbackPhase::RestoringBackup => {
                    restore_recovered_backup(
                        &journal.authorization,
                        index,
                        &journal.operations[index],
                    )?;
                    journal.operations[index].set_rollback_phase(JournalRollbackPhase::Restored);
                    persist_journal(store, transaction_id, journal)?;
                }
                JournalRollbackPhase::Restored => break,
            }
        }
    }
    journal.phase = JournalPhase::RolledBack;
    persist_journal(store, transaction_id, journal)
}

fn remove_recovered_applied(
    token: &str,
    index: usize,
    operation: &JournalOperation,
) -> Result<(), WorkspaceEditError> {
    if operation.operation_phase() == JournalOperationPhase::NotStarted {
        return Ok(());
    }
    match operation {
        JournalOperation::Text {
            root,
            path,
            prepared_name,
            prepared,
            installed,
            installed_hash,
            ..
        }
        | JournalOperation::Create {
            root,
            path,
            prepared_name,
            prepared,
            installed,
            installed_hash,
            ..
        } => {
            if root.as_os_str().is_empty() {
                return Ok(());
            }
            let workspace = recovery_workspace(root)?;
            if let Some(expected) = (*installed).or(*prepared) {
                let restored = match operation {
                    JournalOperation::Text { original, .. } => original.as_ref(),
                    JournalOperation::Create { previous, .. } => previous.as_ref(),
                    _ => unreachable!(),
                };
                let current = workspace
                    .snapshot(path)
                    .map_err(WorkspaceEditError::Recovery)?;
                if !current
                    .as_ref()
                    .zip(restored)
                    .is_some_and(|(current, restored)| snapshot_matches(restored, current))
                {
                    remove_exact_recovery_file(&workspace, path, expected, installed_hash)?;
                }
            }
            if let Some(prepared) = prepared {
                workspace
                    .remove_prepared_file_if_matches(token, index, *prepared)
                    .map_err(WorkspaceEditError::Recovery)?;
            } else if let Some(prepared_name) = prepared_name {
                workspace
                    .remove_prepared_file_without_identity(
                        token,
                        index,
                        prepared_name,
                        installed_hash,
                    )
                    .map_err(WorkspaceEditError::Recovery)?;
            }
            Ok(())
        }
        JournalOperation::Rename {
            root,
            old_path,
            new_path,
            source,
            ..
        } => {
            if root.as_os_str().is_empty() {
                return Ok(());
            }
            let workspace = recovery_workspace(root)?;
            let old = workspace
                .snapshot(old_path)
                .map_err(WorkspaceEditError::Recovery)?;
            let new = workspace
                .snapshot(new_path)
                .map_err(WorkspaceEditError::Recovery)?;
            if new
                .as_ref()
                .is_some_and(|current| snapshot_matches(source, current))
            {
                if old.is_some() {
                    return Err(WorkspaceEditError::Recovery(format!(
                        "rename recovery source {old_path} is occupied"
                    )));
                }
                workspace
                    .rename(new_path, old_path, source.identity)
                    .map_err(|error| WorkspaceEditError::Recovery(error.message))?;
            } else if !old
                .as_ref()
                .is_some_and(|current| snapshot_matches(source, current))
            {
                return Err(WorkspaceEditError::Recovery(format!(
                    "rename recovery identity for {old_path} is missing"
                )));
            }
            Ok(())
        }
        JournalOperation::Delete { .. } => Ok(()),
    }
}

fn restore_recovered_backup(
    token: &str,
    index: usize,
    operation: &JournalOperation,
) -> Result<(), WorkspaceEditError> {
    if operation.operation_phase() == JournalOperationPhase::NotStarted {
        return Ok(());
    }
    let (root, path, backup, recursive) = match operation {
        JournalOperation::Text {
            root,
            path,
            original,
            ..
        } => (root, path, original.as_ref(), false),
        JournalOperation::Create {
            root,
            path,
            previous,
            ..
        } => (root, path, previous.as_ref(), true),
        JournalOperation::Rename {
            root,
            new_path,
            previous,
            ..
        } => (root, new_path, previous.as_ref(), true),
        JournalOperation::Delete {
            root,
            path,
            snapshot,
            recursive,
            ..
        } => (root, path, Some(snapshot), *recursive),
    };
    if root.as_os_str().is_empty() {
        return Ok(());
    }
    let workspace = recovery_workspace(root)?;
    if let Some(expected) = backup {
        workspace
            .restore_recovery_backup(path, token, index, expected)
            .map_err(WorkspaceEditError::Recovery)?;
    }
    if let JournalOperation::Text {
        prepared_name,
        prepared,
        installed_hash,
        ..
    }
    | JournalOperation::Create {
        prepared_name,
        prepared,
        installed_hash,
        ..
    } = operation
    {
        if let Some(prepared) = prepared {
            workspace
                .remove_prepared_file_if_matches(token, index, *prepared)
                .map_err(WorkspaceEditError::Recovery)?;
        } else if let Some(prepared_name) = prepared_name {
            workspace
                .remove_prepared_file_without_identity(token, index, prepared_name, installed_hash)
                .map_err(WorkspaceEditError::Recovery)?;
        }
    }
    let _ = recursive;
    Ok(())
}

fn cleanup_committed_journal(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
    journal: &mut WorkspaceEditJournal,
) -> Result<(), WorkspaceEditError> {
    for index in (0..journal.operations.len()).rev() {
        if journal.operations[index].operation_phase() == JournalOperationPhase::NotStarted {
            journal.operations[index].set_cleanup_phase(JournalCleanupPhase::Discarded);
            persist_journal(store, transaction_id, journal)?;
            continue;
        }
        if journal.operations[index].cleanup_phase() == JournalCleanupPhase::Pending {
            validate_committed_backup(&journal.authorization, index, &journal.operations[index])?;
            journal.operations[index].set_cleanup_phase(JournalCleanupPhase::Discarding);
            persist_journal(store, transaction_id, journal)?;
            inject_cleanup_fault(CleanupFault::AfterMarkDiscarding)?;
        }
        if journal.operations[index].cleanup_phase() == JournalCleanupPhase::Discarded {
            continue;
        }
        discard_committed_backup(&journal.authorization, index, &journal.operations[index])?;
        inject_cleanup_fault(CleanupFault::BeforeMarkDiscarded)?;
        journal.operations[index].set_cleanup_phase(JournalCleanupPhase::Discarded);
        persist_journal(store, transaction_id, journal)?;
        inject_cleanup_fault(CleanupFault::AfterMarkDiscarded)?;
    }
    cleanup_recovery_directories(journal)
}

fn committed_backup(
    operation: &JournalOperation,
) -> Option<(&PathBuf, &str, &SecurePathSnapshot, bool)> {
    match operation {
        JournalOperation::Text {
            root,
            path,
            original: Some(backup),
            ..
        } => Some((root, path, backup, false)),
        JournalOperation::Create {
            root,
            path,
            previous: Some(backup),
            ..
        } => Some((root, path, backup, true)),
        JournalOperation::Rename {
            root,
            new_path,
            previous: Some(backup),
            ..
        } => Some((root, new_path, backup, true)),
        JournalOperation::Delete {
            root,
            path,
            snapshot,
            recursive,
            ..
        } => Some((root, path, snapshot, *recursive)),
        _ => None,
    }
}

fn validate_committed_backup(
    token: &str,
    index: usize,
    operation: &JournalOperation,
) -> Result<(), WorkspaceEditError> {
    let Some((root, path, backup, _)) = committed_backup(operation) else {
        return Ok(());
    };
    if root.as_os_str().is_empty() {
        return Ok(());
    }
    recovery_workspace(root)?
        .validate_recovery_backup(path, token, index, backup)
        .map_err(WorkspaceEditError::Recovery)
}

fn discard_committed_backup(
    token: &str,
    index: usize,
    operation: &JournalOperation,
) -> Result<(), WorkspaceEditError> {
    let Some((root, path, backup, recursive)) = committed_backup(operation) else {
        return Ok(());
    };
    if root.as_os_str().is_empty() {
        return Ok(());
    }
    recovery_workspace(root)?
        .discard_recovery_backup(path, token, index, backup, recursive)
        .map_err(WorkspaceEditError::Recovery)
}

fn clear_transaction_tombstones(transaction: &mut WorkspaceEditTransaction) {
    for operation in &mut transaction.operations {
        match operation {
            TransactionOperation::TextDocument(index) => {
                if let Some(closed) = transaction.documents[*index].closed.as_mut() {
                    closed.tombstone = None;
                }
            }
            TransactionOperation::Resource(operation) => match &mut operation.kind {
                ResourceOperationKind::Create { tombstone, .. }
                | ResourceOperationKind::Rename { tombstone, .. }
                | ResourceOperationKind::Delete { tombstone, .. } => *tombstone = None,
            },
        }
    }
}

fn cleanup_recovery_directories(journal: &WorkspaceEditJournal) -> Result<(), WorkspaceEditError> {
    let roots = journal
        .operations
        .iter()
        .filter_map(|operation| match operation {
            JournalOperation::Text { root, .. } if root.as_os_str().is_empty() => None,
            JournalOperation::Text { root, .. }
            | JournalOperation::Create { root, .. }
            | JournalOperation::Rename { root, .. }
            | JournalOperation::Delete { root, .. } => Some(root),
        })
        .collect::<std::collections::HashSet<_>>();
    for root in roots {
        recovery_workspace(root)?
            .remove_recovery_directory(&journal.authorization)
            .map_err(WorkspaceEditError::Recovery)?;
    }
    Ok(())
}

fn recovery_workspace(root: &std::path::Path) -> Result<SecureWorkspace, WorkspaceEditError> {
    SecureWorkspace::open(root).map_err(WorkspaceEditError::Recovery)
}

fn remove_exact_recovery_file(
    workspace: &SecureWorkspace,
    path: &str,
    expected: FileIdentity,
    expected_hash: &str,
) -> Result<(), WorkspaceEditError> {
    let Some(current) = workspace
        .snapshot(path)
        .map_err(WorkspaceEditError::Recovery)?
    else {
        return Ok(());
    };
    if current.identity != expected
        || current.kind != SecurePathKind::File
        || current
            .content
            .as_deref()
            .is_none_or(|content| content_hash(content) != expected_hash)
    {
        return Err(WorkspaceEditError::Recovery(format!(
            "recovery target {path} no longer has its recorded identity and hash"
        )));
    }
    workspace
        .remove_created(path, expected)
        .map_err(WorkspaceEditError::Recovery)
}

fn persist_journal(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
    journal: &WorkspaceEditJournal,
) -> Result<(), WorkspaceEditError> {
    let encoded = serde_json::to_string(journal).map_err(|error| {
        WorkspaceEditError::Recovery(format!("could not encode workspace edit journal: {error}"))
    })?;
    store
        .save_workspace_edit_journal(transaction_id, &encoded)
        .map_err(|error| {
            WorkspaceEditError::Recovery(format!(
                "could not durably update workspace edit journal: {error}"
            ))
        })
}

fn load_workspace_edit_journal(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
) -> Result<WorkspaceEditJournal, WorkspaceEditError> {
    let encoded = store
        .workspace_edit_journals()
        .map_err(|error| {
            WorkspaceEditError::Recovery(format!(
                "could not load workspace edit recovery journal: {error}"
            ))
        })?
        .into_iter()
        .find_map(|(id, journal)| (id == transaction_id).then_some(journal))
        .ok_or_else(|| {
            WorkspaceEditError::Recovery(format!(
                "transaction {transaction_id} no longer has actionable recovery data"
            ))
        })?;
    serde_json::from_str(&encoded).map_err(|error| {
        WorkspaceEditError::Recovery(format!(
            "workspace edit journal {transaction_id} is invalid: {error}"
        ))
    })
}

fn workspace_edit_outcome(
    journal: &WorkspaceEditJournal,
    phase: WorkspaceEditTransactionPhase,
) -> WorkspaceEditOutcome {
    WorkspaceEditOutcome {
        authorization_hash: content_hash(&journal.authorization),
        authorization_expires_at: None,
        phase,
        created_at: unix_timestamp(),
        operations: journal
            .operations
            .iter()
            .map(|operation| match operation {
                JournalOperation::Text { document, .. } => {
                    WorkspaceEditOutcomeOperation::TextDocument {
                        document: *document,
                    }
                }
                JournalOperation::Create {
                    workspace_id, path, ..
                } => WorkspaceEditOutcomeOperation::CreateFile {
                    workspace_id: *workspace_id,
                    path: path.clone(),
                },
                JournalOperation::Rename {
                    workspace_id,
                    old_path,
                    new_path,
                    ..
                } => WorkspaceEditOutcomeOperation::RenameFile {
                    workspace_id: *workspace_id,
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                },
                JournalOperation::Delete {
                    workspace_id,
                    path,
                    recursive,
                    ..
                } => WorkspaceEditOutcomeOperation::DeleteFile {
                    workspace_id: *workspace_id,
                    path: path.clone(),
                    recursive: *recursive,
                },
            })
            .collect(),
    }
}

fn persist_outcome(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
    outcome: &WorkspaceEditOutcome,
) -> Result<(), WorkspaceEditError> {
    let encoded = serde_json::to_string(outcome).map_err(|error| {
        WorkspaceEditError::Recovery(format!("could not encode workspace edit outcome: {error}"))
    })?;
    store
        .save_workspace_edit_outcome(transaction_id, &encoded)
        .map_err(|error| {
            WorkspaceEditError::Recovery(format!(
                "could not durably save workspace edit outcome: {error}"
            ))
        })
}

fn mark_committed_cleanup_required(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
    transaction: &mut WorkspaceEditTransaction,
    journal: &WorkspaceEditJournal,
    outcomes: &Mutex<HashMap<u64, WorkspaceEditOutcome>>,
) -> Result<(), WorkspaceEditError> {
    transaction.phase = WorkspaceEditTransactionPhase::CommittedCleanupRequired;
    let outcome = workspace_edit_outcome(
        journal,
        WorkspaceEditTransactionPhase::CommittedCleanupRequired,
    );
    persist_outcome(store, transaction_id, &outcome)?;
    outcomes
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(transaction_id, outcome);
    Ok(())
}

fn mark_active_committed_cleanup_required(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
    transaction: &mut WorkspaceEditTransaction,
    outcomes: &Mutex<HashMap<u64, WorkspaceEditOutcome>>,
) -> Result<(), WorkspaceEditError> {
    let journal = load_workspace_edit_journal(store, transaction_id)?;
    mark_committed_cleanup_required(store, transaction_id, transaction, &journal, outcomes)
}

fn mark_recovered_committed_cleanup_required(
    store: &crate::persistence::StateStore,
    transaction_id: u64,
    outcome: &WorkspaceEditOutcome,
    outcomes: &Mutex<HashMap<u64, WorkspaceEditOutcome>>,
) -> Result<(), WorkspaceEditError> {
    let cleanup_required = WorkspaceEditOutcome {
        phase: WorkspaceEditTransactionPhase::CommittedCleanupRequired,
        created_at: outcome.created_at,
        ..outcome.clone()
    };
    persist_outcome(store, transaction_id, &cleanup_required)?;
    outcomes
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(transaction_id, cleanup_required);
    Ok(())
}

fn load_workspace_edit_outcomes(
    store: &crate::persistence::StateStore,
) -> Result<HashMap<u64, WorkspaceEditOutcome>, WorkspaceEditError> {
    let records = store.workspace_edit_outcomes().map_err(|error| {
        WorkspaceEditError::Recovery(format!("could not load workspace edit outcomes: {error}"))
    })?;
    let mut outcomes = HashMap::new();
    for (transaction_id, encoded) in records {
        let outcome: WorkspaceEditOutcome = serde_json::from_str(&encoded).map_err(|error| {
            WorkspaceEditError::Recovery(format!(
                "workspace edit outcome {transaction_id} is invalid: {error}"
            ))
        })?;
        outcomes.insert(transaction_id, outcome);
    }
    let mut terminal = outcomes
        .iter()
        .filter(|(_, outcome)| outcome.phase.is_finished())
        .map(|(id, outcome)| (*id, outcome.created_at))
        .collect::<Vec<_>>();
    terminal.sort_unstable_by_key(|(_, created_at)| std::cmp::Reverse(*created_at));
    if terminal.len() > MAX_WORKSPACE_EDIT_OUTCOMES {
        return Err(WorkspaceEditError::Recovery(format!(
            "workspace edit terminal outcome limit of {MAX_WORKSPACE_EDIT_OUTCOMES} was exceeded"
        )));
    }
    Ok(outcomes)
}

fn prepare_persisted_workspace_edit_outcomes(
    store: &crate::persistence::StateStore,
    outcomes: &mut HashMap<u64, WorkspaceEditOutcome>,
) -> Result<HashMap<u64, String>, WorkspaceEditError> {
    let mut authorizations = HashMap::new();
    let transaction_ids = outcomes.keys().copied().collect::<Vec<_>>();
    for transaction_id in transaction_ids {
        let Some(outcome) = outcomes.get_mut(&transaction_id) else {
            continue;
        };
        if !outcome.phase.is_recovery()
            && outcome.phase != WorkspaceEditTransactionPhase::FinishingCommitted
            && !outcome.phase.is_finished()
        {
            continue;
        }
        let now = unix_timestamp();
        if outcome.phase == WorkspaceEditTransactionPhase::RecoveryRequired
            && now.saturating_sub(outcome.created_at) > WORKSPACE_EDIT_OUTCOME_TTL.as_secs()
        {
            outcome.authorization_expires_at = Some(now.saturating_sub(1));
            persist_outcome(store, transaction_id, outcome)?;
            continue;
        }
        let authorization = random_token().map_err(WorkspaceEditError::Io)?;
        outcome.authorization_hash = content_hash(&authorization);
        outcome.authorization_expires_at = if outcome.phase.is_finished()
            || matches!(
                outcome.phase,
                WorkspaceEditTransactionPhase::FinishingCommitted
                    | WorkspaceEditTransactionPhase::CommittedCleanupRequired
            ) {
            None
        } else {
            Some(now.saturating_add(WORKSPACE_EDIT_OUTCOME_TTL.as_secs()))
        };
        persist_outcome(store, transaction_id, outcome)?;
        authorizations.insert(transaction_id, authorization);
    }
    Ok(authorizations)
}

fn outcome_authorization_expired(outcome: &WorkspaceEditOutcome) -> bool {
    outcome
        .authorization_expires_at
        .is_some_and(|expires_at| unix_timestamp() > expires_at)
}

fn validate_outcome_authorization(
    outcome: &WorkspaceEditOutcome,
    authorization: &str,
) -> Result<(), WorkspaceEditError> {
    if outcome_authorization_expired(outcome) {
        return Err(WorkspaceEditError::Expired);
    }
    if outcome.authorization_hash == content_hash(authorization) {
        Ok(())
    } else {
        Err(WorkspaceEditError::Invalid(
            "workspace edit transaction authorization is invalid".to_owned(),
        ))
    }
}

fn outcome_status(
    transaction_id: u64,
    outcome: &WorkspaceEditOutcome,
) -> WorkspaceEditTransactionStatus {
    WorkspaceEditTransactionStatus {
        transaction_id,
        phase: outcome.phase,
        retry_rollback: outcome.phase == WorkspaceEditTransactionPhase::RecoveryRequired,
        can_finalize: matches!(
            outcome.phase,
            WorkspaceEditTransactionPhase::RecoveryRequired
                | WorkspaceEditTransactionPhase::FinishingCommitted
                | WorkspaceEditTransactionPhase::CommittedCleanupRequired
        ),
        requires_acknowledgement: outcome.phase.is_finished(),
    }
}

fn load_workspace_edit_acknowledgements(
    store: &crate::persistence::StateStore,
) -> Result<HashMap<u64, WorkspaceEditAcknowledgement>, WorkspaceEditError> {
    let now = unix_timestamp();
    let records = store.workspace_edit_acknowledgements().map_err(|error| {
        WorkspaceEditError::Recovery(format!(
            "could not load workspace edit acknowledgements: {error}"
        ))
    })?;
    let mut acknowledgements = HashMap::new();
    for (index, (transaction_id, authorization_hash, created_at)) in records.into_iter().enumerate()
    {
        if index >= MAX_WORKSPACE_EDIT_OUTCOMES
            || now.saturating_sub(created_at) > WORKSPACE_EDIT_OUTCOME_TTL.as_secs()
        {
            store
                .delete_workspace_edit_acknowledgement(transaction_id)
                .map_err(|error| WorkspaceEditError::Recovery(error.to_string()))?;
            continue;
        }
        acknowledgements.insert(
            transaction_id,
            WorkspaceEditAcknowledgement {
                authorization_hash,
                created_at,
            },
        );
    }
    Ok(acknowledgements)
}

fn trim_workspace_edit_acknowledgements(
    store: Option<&crate::persistence::StateStore>,
    acknowledgements: &mut HashMap<u64, WorkspaceEditAcknowledgement>,
) -> Result<(), WorkspaceEditError> {
    let mut ordered = acknowledgements
        .iter()
        .map(|(id, acknowledgement)| (*id, acknowledgement.created_at))
        .collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|(id, created_at)| (std::cmp::Reverse(*created_at), *id));
    for (transaction_id, _) in ordered.into_iter().skip(MAX_WORKSPACE_EDIT_OUTCOMES) {
        if let Some(store) = store {
            store
                .delete_workspace_edit_acknowledgement(transaction_id)
                .map_err(|error| WorkspaceEditError::Recovery(error.to_string()))?;
        }
        acknowledgements.remove(&transaction_id);
    }
    Ok(())
}

fn outcome_operations(outcome: &WorkspaceEditOutcome) -> Vec<StagedWorkspaceEditOperation> {
    outcome
        .operations
        .iter()
        .map(|operation| match operation {
            WorkspaceEditOutcomeOperation::TextDocument { document } => {
                StagedWorkspaceEditOperation::TextDocument {
                    document: *document,
                }
            }
            WorkspaceEditOutcomeOperation::CreateFile { workspace_id, path } => {
                StagedWorkspaceEditOperation::CreateFile {
                    workspace_id: WorkspaceId::new(*workspace_id),
                    path: path.clone(),
                }
            }
            WorkspaceEditOutcomeOperation::RenameFile {
                workspace_id,
                old_path,
                new_path,
            } => StagedWorkspaceEditOperation::RenameFile {
                workspace_id: WorkspaceId::new(*workspace_id),
                old_path: old_path.clone(),
                new_path: new_path.clone(),
            },
            WorkspaceEditOutcomeOperation::DeleteFile {
                workspace_id,
                path,
                recursive,
            } => StagedWorkspaceEditOperation::DeleteFile {
                workspace_id: WorkspaceId::new(*workspace_id),
                path: path.clone(),
                recursive: *recursive,
            },
        })
        .collect()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn recovered_journal_has_applied_files(
    journal: &WorkspaceEditJournal,
) -> Result<bool, WorkspaceEditError> {
    for operation in &journal.operations {
        if operation.operation_phase() == JournalOperationPhase::NotStarted {
            continue;
        }
        let applied = match operation {
            JournalOperation::Text {
                root,
                path,
                prepared,
                installed,
                installed_hash,
                ..
            }
            | JournalOperation::Create {
                root,
                path,
                prepared,
                installed,
                installed_hash,
                ..
            } => {
                if root.as_os_str().is_empty() {
                    false
                } else if let Some(expected) = (*installed).or(*prepared) {
                    recovery_workspace(root)?
                        .snapshot(path)
                        .map_err(WorkspaceEditError::Recovery)?
                        .is_some_and(|current| {
                            current.identity == expected
                                && current.kind == SecurePathKind::File
                                && current
                                    .content
                                    .as_deref()
                                    .is_some_and(|content| content_hash(content) == *installed_hash)
                        })
                } else {
                    false
                }
            }
            JournalOperation::Rename {
                root,
                new_path,
                source,
                moved,
                ..
            } => {
                *moved
                    && recovery_workspace(root)?
                        .snapshot(new_path)
                        .map_err(WorkspaceEditError::Recovery)?
                        .is_some_and(|current| snapshot_matches(source, &current))
            }
            JournalOperation::Delete {
                root,
                path,
                removed,
                ..
            } => {
                *removed
                    && recovery_workspace(root)?
                        .snapshot(path)
                        .map_err(WorkspaceEditError::Recovery)?
                        .is_none()
            }
        };
        if applied {
            return Ok(true);
        }
    }
    Ok(false)
}

impl JournalOperation {
    fn operation_phase(&self) -> JournalOperationPhase {
        match self {
            Self::Text { phase, .. }
            | Self::Create { phase, .. }
            | Self::Rename { phase, .. }
            | Self::Delete { phase, .. } => *phase,
        }
    }

    fn cleanup_phase(&self) -> JournalCleanupPhase {
        match self {
            Self::Text { cleanup, .. }
            | Self::Create { cleanup, .. }
            | Self::Rename { cleanup, .. }
            | Self::Delete { cleanup, .. } => *cleanup,
        }
    }

    fn set_cleanup_phase(&mut self, phase: JournalCleanupPhase) {
        match self {
            Self::Text { cleanup, .. }
            | Self::Create { cleanup, .. }
            | Self::Rename { cleanup, .. }
            | Self::Delete { cleanup, .. } => *cleanup = phase,
        }
    }

    fn rollback_phase(&self) -> JournalRollbackPhase {
        match self {
            Self::Text { rollback, .. }
            | Self::Create { rollback, .. }
            | Self::Rename { rollback, .. }
            | Self::Delete { rollback, .. } => *rollback,
        }
    }

    fn set_rollback_phase(&mut self, phase: JournalRollbackPhase) {
        match self {
            Self::Text { rollback, .. }
            | Self::Create { rollback, .. }
            | Self::Rename { rollback, .. }
            | Self::Delete { rollback, .. } => *rollback = phase,
        }
    }
}

fn rollback_transaction(
    transaction: &mut WorkspaceEditTransaction,
    transaction_id: u64,
) -> Result<(), WorkspaceEditError> {
    rollback_applied(
        &mut transaction.operations,
        &mut transaction.documents,
        &transaction.authorization,
    )
    .map_err(|error| {
        transaction.phase = WorkspaceEditTransactionPhase::RecoveryRequired;
        WorkspaceEditError::Recovery(format!(
            "closed-file rollback is incomplete: {error}; retry rollback or explicitly finalize transaction {transaction_id}"
        ))
    })
}

fn rollback_transaction_durably(
    store: Option<&crate::persistence::StateStore>,
    transaction: &mut WorkspaceEditTransaction,
    transaction_id: u64,
) -> Result<(), WorkspaceEditError> {
    let Some(store) = store else {
        rollback_transaction(transaction, transaction_id)?;
        mark_transaction_rolled_back(transaction);
        return Ok(());
    };
    transaction.phase = WorkspaceEditTransactionPhase::RecoveryRequired;
    let mut journal = workspace_edit_journal(transaction, JournalPhase::RecoveryRequired);
    persist_journal(store, transaction_id, &journal)?;
    rollback_recovered_journal(store, transaction_id, &mut journal).map_err(|error| {
        WorkspaceEditError::Recovery(format!(
            "filesystem rollback is incomplete: {error}; retry rollback or explicitly finalize transaction {transaction_id}"
        ))
    })?;
    mark_transaction_rolled_back(transaction);
    Ok(())
}

fn mark_transaction_rolled_back(transaction: &mut WorkspaceEditTransaction) {
    for operation in &mut transaction.operations {
        match operation {
            TransactionOperation::TextDocument(index) => {
                let Some(closed) = transaction.documents[*index].closed.as_mut() else {
                    continue;
                };
                closed.tombstone = None;
                closed.prepared = None;
                closed.state = closed
                    .original
                    .as_ref()
                    .map(|snapshot| ClosedDocumentState::Original(snapshot.identity))
                    .unwrap_or(ClosedDocumentState::Pending);
            }
            TransactionOperation::Resource(operation) => match &mut operation.kind {
                ResourceOperationKind::Create {
                    tombstone,
                    prepared,
                    created,
                    ..
                } => {
                    *tombstone = None;
                    *prepared = None;
                    *created = None;
                }
                ResourceOperationKind::Rename {
                    tombstone, moved, ..
                } => {
                    *tombstone = None;
                    *moved = false;
                }
                ResourceOperationKind::Delete { tombstone, .. } => *tombstone = None,
            },
        }
    }
    transaction.phase = WorkspaceEditTransactionPhase::RolledBack;
}

fn fail_commit(
    store: Option<&crate::persistence::StateStore>,
    transaction_id: u64,
    transaction: &mut WorkspaceEditTransaction,
    error: WorkspaceEditError,
) -> WorkspaceEditError {
    let mutation_possible = transaction_has_possible_mutations(transaction);
    let durable_recovery_error = if mutation_possible {
        transaction.phase = WorkspaceEditTransactionPhase::RecoveryRequired;
        save_journal(
            store,
            transaction_id,
            transaction,
            JournalPhase::RecoveryRequired,
        )
        .err()
    } else {
        None
    };
    let rollback_result = if mutation_possible && durable_recovery_error.is_none() {
        rollback_transaction_durably(store, transaction, transaction_id)
    } else {
        rollback_applied(
            &mut transaction.operations,
            &mut transaction.documents,
            &transaction.authorization,
        )
    };
    match rollback_result {
        Ok(()) if mutation_possible => {
            if let Err(cleanup_error) = remove_recovery_directories(transaction) {
                transaction.phase = WorkspaceEditTransactionPhase::RecoveryRequired;
                return WorkspaceEditError::Recovery(format!(
                    "{error}; rollback completed but recovery cleanup failed: {cleanup_error}"
                ));
            }
            if let Some(durable_error) = durable_recovery_error {
                WorkspaceEditError::Recovery(format!(
                    "{error}; could not durably record the recovery-required transition: {durable_error}"
                ))
            } else {
                error
            }
        }
        Ok(()) => error,
        Err(rollback) => {
            transaction.phase = WorkspaceEditTransactionPhase::RecoveryRequired;
            let _ = save_journal(
                store,
                transaction_id,
                transaction,
                JournalPhase::RecoveryRequired,
            );
            WorkspaceEditError::Recovery(format!(
                "{error}; reverse rollback failed: {rollback}; retry rollback or explicitly finalize transaction {transaction_id}"
            ))
        }
    }
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
        can_finalize: matches!(
            transaction.phase,
            WorkspaceEditTransactionPhase::RecoveryRequired
                | WorkspaceEditTransactionPhase::FinishingCommitted
                | WorkspaceEditTransactionPhase::CommittedCleanupRequired
        ),
        requires_acknowledgement: false,
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
    }) || transaction
        .operations
        .iter()
        .any(|operation| match operation {
            TransactionOperation::TextDocument(_) => false,
            TransactionOperation::Resource(operation) => match &operation.kind {
                ResourceOperationKind::Create { created, .. } => created.is_some(),
                ResourceOperationKind::Rename { moved, .. } => *moved,
                ResourceOperationKind::Delete { tombstone, .. } => tombstone.is_some(),
            },
        })
}

fn transaction_has_possible_mutations(transaction: &WorkspaceEditTransaction) -> bool {
    transaction.documents.iter().any(|document| {
        document
            .closed
            .as_ref()
            .is_some_and(|closed| closed.operation_phase != JournalOperationPhase::NotStarted)
    }) || transaction
        .operations
        .iter()
        .any(|operation| match operation {
            TransactionOperation::TextDocument(_) => false,
            TransactionOperation::Resource(operation) => {
                operation.phase != JournalOperationPhase::NotStarted
            }
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
    outcomes: &HashMap<u64, WorkspaceEditOutcome>,
) -> Result<u64, WorkspaceEditError> {
    for _ in 0..=transactions.len() + outcomes.len() {
        let id = next_id.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        if id != 0 && !transactions.contains_key(&id) && !outcomes.contains_key(&id) {
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
    fn stages_changes_document_changes_and_resource_operations() {
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

        let repeated = manager
            .stage(
                &serde_json::json!({
                    "documentChanges": [
                        { "textDocument": { "uri": uri, "version": null }, "edits": [] },
                        { "textDocument": { "uri": file_uri(&path), "version": null }, "edits": [] }
                    ]
                }),
                &roots,
                &[],
            )
            .unwrap();
        assert_eq!(repeated.operations.len(), 2);
        manager
            .finish(repeated.transaction_id, &repeated.authorization)
            .unwrap();
        let resource = manager
            .stage(
                &serde_json::json!({
                    "documentChanges": [{ "kind": "create", "uri": file_uri(&root.join("new")) }]
                }),
                &roots,
                &[],
            )
            .unwrap();
        assert!(matches!(
            resource.operations[0],
            StagedWorkspaceEditOperation::CreateFile { .. }
        ));
        manager
            .finish(resource.transaction_id, &resource.authorization)
            .unwrap();
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
            assert!(
                manager
                    .stage(
                        &serde_json::json!({ "changes": { file_uri(&root.join("link.txt")): [] } }),
                        &roots,
                        &[]
                    )
                    .is_err()
            );
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
            saved_text: "open".to_owned(),
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
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&root.join("source/file.txt")): [{ "range": range(0, 6), "newText": "edited" }]
                }}),
                &roots,
                &[],
            )
            .unwrap();
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
    fn unacknowledged_closed_files_can_be_rolled_back() {
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
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "after");

        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(
            manager
                .status(staged.transaction_id, &staged.authorization)
                .unwrap()
                .phase,
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
            .commit_closed(completed.transaction_id, &completed.authorization, &[])
            .unwrap();
        manager
            .finish(completed.transaction_id, &completed.authorization)
            .unwrap();
        assert!(matches!(
            manager.rollback(completed.transaction_id, &completed.authorization),
            Err(WorkspaceEditError::Invalid(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "after");

        fs::write(&path, "before").unwrap();
        let cancelled = make_edit();
        manager
            .commit_closed(cancelled.transaction_id, &cancelled.authorization, &[])
            .unwrap();
        manager
            .rollback(cancelled.transaction_id, &cancelled.authorization)
            .unwrap();
        manager
            .finish(cancelled.transaction_id, &cancelled.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "before");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn coalesces_open_text_lineage_around_resource_operations_and_rolls_back_later_failure() {
        use crate::language_servers::secure_edit::{SecureEditFault, fail_next_secure_edit_at};

        let root = test_directory("coalesced-open-resource-failure");
        let old_path = root.join("old.txt");
        fs::write(&old_path, "before").unwrap();
        let open = vec![WorkspaceEditOpenDocument {
            workspace_id: WorkspaceId::new(1),
            path: "old.txt".to_owned(),
            generation: 4,
            version: 9,
            text: "before".to_owned(),
            saved_text: "before".to_owned(),
        }];
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "textDocument": { "uri": file_uri(&old_path), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "middle" }
                    ]},
                    { "kind": "rename", "oldUri": file_uri(&old_path), "newUri": file_uri(&root.join("renamed.txt")) },
                    { "textDocument": { "uri": file_uri(&root.join("renamed.txt")), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "final" }
                    ]},
                    { "kind": "create", "uri": file_uri(&root.join("later.txt")) }
                ]}),
                &roots(&root),
                &open,
            )
            .unwrap();

        let text_operations = staged
            .operations
            .iter()
            .filter_map(|operation| match operation {
                StagedWorkspaceEditOperation::TextDocument { document } => Some(*document),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text_operations, [0]);
        assert!(matches!(
            staged.operations.as_slice(),
            [
                StagedWorkspaceEditOperation::RenameFile { .. },
                StagedWorkspaceEditOperation::TextDocument { document: 0 },
                StagedWorkspaceEditOperation::CreateFile { .. }
            ]
        ));
        assert_eq!(staged.documents[0].original_path, "old.txt");
        assert_eq!(staged.documents[0].path, "renamed.txt");
        assert_eq!(staged.documents[0].original_text, "before");
        assert_eq!(staged.documents[0].new_text, "final");
        assert_eq!(staged.documents[0].generation, Some(4));
        assert_eq!(staged.documents[0].version, Some(9));

        fail_next_secure_edit_at(SecureEditFault::Prepare);
        assert!(
            manager
                .commit_closed(staged.transaction_id, &staged.authorization, &open)
                .is_err()
        );
        assert_eq!(fs::read_to_string(&old_path).unwrap(), "before");
        assert!(!root.join("renamed.txt").exists());
        assert!(!root.join("later.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_text_then_delete_commits_and_restores_the_original_lineage() {
        let root = test_directory("open-text-delete-lineage");
        let path = root.join("open.txt");
        fs::write(&path, "before").unwrap();
        let open = open_document("open.txt", "before");
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "textDocument": { "uri": file_uri(&path), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "after" }
                    ]},
                    { "kind": "delete", "uri": file_uri(&path) }
                ]}),
                &roots(&root),
                std::slice::from_ref(&open),
            )
            .unwrap();

        assert!(matches!(
            staged.operations.as_slice(),
            [
                StagedWorkspaceEditOperation::TextDocument { .. },
                StagedWorkspaceEditOperation::DeleteFile { .. }
            ]
        ));
        manager
            .commit_closed(
                staged.transaction_id,
                &staged.authorization,
                std::slice::from_ref(&open),
            )
            .unwrap();
        assert!(!path.exists());
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "before");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_text_then_overwrite_create_retires_and_restores_the_original_lineage() {
        let root = test_directory("open-text-overwrite-create-lineage");
        let path = root.join("open.txt");
        fs::write(&path, "before").unwrap();
        let open = open_document("open.txt", "before");
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "textDocument": { "uri": file_uri(&path), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "obsolete" }
                    ]},
                    { "kind": "create", "uri": file_uri(&path), "options": { "overwrite": true } }
                ]}),
                &roots(&root),
                std::slice::from_ref(&open),
            )
            .unwrap();

        manager
            .commit_closed(
                staged.transaction_id,
                &staged.authorization,
                std::slice::from_ref(&open),
            )
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "before");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_text_then_rename_stages_one_final_destination_lineage() {
        let root = test_directory("open-text-rename-lineage");
        let old_path = root.join("old.txt");
        let new_path = root.join("new.txt");
        fs::write(&old_path, "before").unwrap();
        let open = open_document("old.txt", "before");
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "textDocument": { "uri": file_uri(&old_path), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "after" }
                    ]},
                    { "kind": "rename", "oldUri": file_uri(&old_path), "newUri": file_uri(&new_path) }
                ]}),
                &roots(&root),
                std::slice::from_ref(&open),
            )
            .unwrap();

        assert_eq!(staged.documents[0].original_path, "old.txt");
        assert_eq!(staged.documents[0].new_text, "after");
        manager
            .commit_closed(
                staged.transaction_id,
                &staged.authorization,
                std::slice::from_ref(&open),
            )
            .unwrap();
        assert_eq!(fs::read_to_string(&new_path).unwrap(), "before");
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&old_path).unwrap(), "before");
        assert!(!new_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_rename_then_text_stages_one_final_destination_lineage() {
        let root = test_directory("open-rename-text-lineage");
        let old_path = root.join("old.txt");
        let new_path = root.join("new.txt");
        fs::write(&old_path, "before").unwrap();
        let open = open_document("old.txt", "before");
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "rename", "oldUri": file_uri(&old_path), "newUri": file_uri(&new_path) },
                    { "textDocument": { "uri": file_uri(&new_path), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "after" }
                    ]}
                ]}),
                &roots(&root),
                std::slice::from_ref(&open),
            )
            .unwrap();

        assert_eq!(staged.documents[0].original_path, "old.txt");
        assert_eq!(staged.documents[0].path, "new.txt");
        assert_eq!(staged.documents[0].new_text, "after");
        manager
            .commit_closed(
                staged.transaction_id,
                &staged.authorization,
                std::slice::from_ref(&open),
            )
            .unwrap();
        assert_eq!(fs::read_to_string(&new_path).unwrap(), "before");
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&old_path).unwrap(), "before");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_multi_rename_text_chain_coalesces_and_rolls_back_exactly() {
        let root = test_directory("open-multi-rename-text-lineage");
        let old_path = root.join("old.txt");
        let middle_path = root.join("middle.txt");
        let final_path = root.join("final.txt");
        fs::write(&old_path, "before").unwrap();
        let open = open_document("old.txt", "before");
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "textDocument": { "uri": file_uri(&old_path), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "middle" }
                    ]},
                    { "kind": "rename", "oldUri": file_uri(&old_path), "newUri": file_uri(&middle_path) },
                    { "textDocument": { "uri": file_uri(&middle_path), "version": 9 }, "edits": [
                        { "range": range(0, 6), "newText": "final text" }
                    ]},
                    { "kind": "rename", "oldUri": file_uri(&middle_path), "newUri": file_uri(&final_path) }
                ]}),
                &roots(&root),
                std::slice::from_ref(&open),
            )
            .unwrap();

        assert_eq!(
            staged
                .operations
                .iter()
                .filter(|operation| matches!(
                    operation,
                    StagedWorkspaceEditOperation::TextDocument { .. }
                ))
                .count(),
            1
        );
        assert_eq!(staged.documents[0].original_path, "old.txt");
        assert_eq!(staged.documents[0].path, "middle.txt");
        assert_eq!(staged.documents[0].original_text, "before");
        assert_eq!(staged.documents[0].new_text, "final text");
        manager
            .commit_closed(
                staged.transaction_id,
                &staged.authorization,
                std::slice::from_ref(&open),
            )
            .unwrap();
        assert_eq!(fs::read_to_string(&final_path).unwrap(), "before");
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(&old_path).unwrap(), "before");
        assert!(!middle_path.exists());
        assert!(!final_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn applies_create_and_rename_interleaved_with_text_edits() {
        let root = test_directory("resource-interleaving");
        fs::write(root.join("old.txt"), "before").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "create", "uri": file_uri(&root.join("created.txt")) },
                    { "textDocument": { "uri": file_uri(&root.join("created.txt")), "version": null }, "edits": [
                        { "range": range(0, 0), "newText": "created" }
                    ]},
                    { "kind": "rename", "oldUri": file_uri(&root.join("old.txt")), "newUri": file_uri(&root.join("renamed.txt")) },
                    { "textDocument": { "uri": file_uri(&root.join("renamed.txt")), "version": null }, "edits": [
                        { "range": range(0, 6), "newText": "after" }
                    ]}
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();

        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("created.txt")).unwrap(),
            "created"
        );
        assert_eq!(
            fs::read_to_string(root.join("renamed.txt")).unwrap(),
            "after"
        );
        assert!(!root.join("old.txt").exists());
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert!(!root.join("created.txt").exists());
        assert_eq!(fs::read_to_string(root.join("old.txt")).unwrap(), "before");
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_and_overwrite_are_recoverable_and_finish_removes_tombstones() {
        let root = test_directory("resource-recovery");
        fs::create_dir(root.join("directory")).unwrap();
        fs::write(root.join("directory/file.txt"), "nested").unwrap();
        fs::write(root.join("source.txt"), "source").unwrap();
        fs::write(root.join("destination.txt"), "destination").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "rename", "oldUri": file_uri(&root.join("source.txt")), "newUri": file_uri(&root.join("destination.txt")), "options": { "overwrite": true } },
                    { "kind": "delete", "uri": file_uri(&root.join("directory")), "options": { "recursive": true } }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("destination.txt")).unwrap(),
            "source"
        );
        assert!(!root.join("directory").exists());
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("source.txt")).unwrap(),
            "source"
        );
        assert_eq!(
            fs::read_to_string(root.join("destination.txt")).unwrap(),
            "destination"
        );
        assert_eq!(
            fs::read_to_string(root.join("directory/file.txt")).unwrap(),
            "nested"
        );
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();

        let committed = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "delete", "uri": file_uri(&root.join("directory")), "options": { "recursive": true } }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(committed.transaction_id, &committed.authorization, &[])
            .unwrap();
        manager
            .finish(committed.transaction_id, &committed.authorization)
            .unwrap();
        assert!(fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("kosmos-workspace-edit")
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn enforces_resource_options_and_non_recursive_directory_delete() {
        let root = test_directory("resource-options");
        fs::create_dir(root.join("directory")).unwrap();
        fs::write(root.join("directory/file.txt"), "nested").unwrap();
        fs::write(root.join("existing.txt"), "existing").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let overwrite_wins = manager.stage(&serde_json::json!({ "documentChanges": [
            { "kind": "create", "uri": file_uri(&root.join("existing.txt")), "options": { "overwrite": true, "ignoreIfExists": true } }
        ]}), &roots(&root), &[]).unwrap();
        manager
            .commit_closed(
                overwrite_wins.transaction_id,
                &overwrite_wins.authorization,
                &[],
            )
            .unwrap();
        assert_eq!(fs::read_to_string(root.join("existing.txt")).unwrap(), "");
        manager
            .rollback(overwrite_wins.transaction_id, &overwrite_wins.authorization)
            .unwrap();
        manager
            .finish(overwrite_wins.transaction_id, &overwrite_wins.authorization)
            .unwrap();
        assert!(
            manager
                .stage(
                    &serde_json::json!({ "documentChanges": [
                        { "kind": "delete", "uri": file_uri(&root.join("directory")) }
                    ]}),
                    &roots(&root),
                    &[]
                )
                .is_err()
        );
        let ignored = manager.stage(&serde_json::json!({ "documentChanges": [
            { "kind": "delete", "uri": file_uri(&root.join("missing.txt")), "options": { "ignoreIfNotExists": true } }
        ]}), &roots(&root), &[]).unwrap();
        assert!(ignored.operations.is_empty());
        manager
            .finish(ignored.transaction_id, &ignored.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ordered_resource_paths_can_be_reused_and_chained() {
        let root = test_directory("ordered-resource-chains");
        fs::write(root.join("source.txt"), "source").unwrap();
        fs::create_dir(root.join("parent")).unwrap();
        fs::write(root.join("parent/child.txt"), "child").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "create", "uri": file_uri(&root.join("created.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("created.txt")), "newUri": file_uri(&root.join("created-renamed.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("source.txt")), "newUri": file_uri(&root.join("middle.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("middle.txt")), "newUri": file_uri(&root.join("final.txt")) },
                    { "kind": "create", "uri": file_uri(&root.join("source.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("parent")), "newUri": file_uri(&root.join("renamed-parent")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("renamed-parent/child.txt")), "newUri": file_uri(&root.join("renamed-parent/nested.txt")) },
                    { "kind": "delete", "uri": file_uri(&root.join("missing.txt")), "options": { "ignoreIfNotExists": true } }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        assert_eq!(staged.operations.len(), 7);
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert!(root.join("created-renamed.txt").is_file());
        assert_eq!(
            fs::read_to_string(root.join("final.txt")).unwrap(),
            "source"
        );
        assert_eq!(fs::read_to_string(root.join("source.txt")).unwrap(), "");
        assert_eq!(
            fs::read_to_string(root.join("renamed-parent/nested.txt")).unwrap(),
            "child"
        );
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("source.txt")).unwrap(),
            "source"
        );
        assert_eq!(
            fs::read_to_string(root.join("parent/child.txt")).unwrap(),
            "child"
        );
        assert!(!root.join("created-renamed.txt").exists());
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();

        let child_first = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "rename", "oldUri": file_uri(&root.join("parent/child.txt")), "newUri": file_uri(&root.join("parent/nested.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("parent")), "newUri": file_uri(&root.join("moved-parent")) }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(child_first.transaction_id, &child_first.authorization, &[])
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("moved-parent/nested.txt")).unwrap(),
            "child"
        );
        manager
            .rollback(child_first.transaction_id, &child_first.authorization)
            .unwrap();
        manager
            .finish(child_first.transaction_id, &child_first.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ordered_overwrite_delete_create_and_text_chains_validate_at_each_turn() {
        let root = test_directory("ordered-overwrite-text-chains");
        fs::write(root.join("a.txt"), "source").unwrap();
        fs::write(root.join("b.txt"), "overwritten").unwrap();
        fs::write(root.join("recycled.txt"), "removed").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "rename", "oldUri": file_uri(&root.join("a.txt")), "newUri": file_uri(&root.join("b.txt")), "options": { "overwrite": true } },
                    { "kind": "rename", "oldUri": file_uri(&root.join("b.txt")), "newUri": file_uri(&root.join("c.txt")) },
                    { "kind": "delete", "uri": file_uri(&root.join("recycled.txt")) },
                    { "kind": "create", "uri": file_uri(&root.join("recycled.txt")) },
                    { "textDocument": { "uri": file_uri(&root.join("recycled.txt")), "version": null }, "edits": [
                        { "range": range(0, 0), "newText": "recreated" }
                    ]},
                    { "kind": "rename", "oldUri": file_uri(&root.join("recycled.txt")), "newUri": file_uri(&root.join("final.txt")) }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();

        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert_eq!(fs::read_to_string(root.join("c.txt")).unwrap(), "source");
        assert_eq!(
            fs::read_to_string(root.join("final.txt")).unwrap(),
            "recreated"
        );
        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "source");
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "overwritten"
        );
        assert_eq!(
            fs::read_to_string(root.join("recycled.txt")).unwrap(),
            "removed"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn overwrite_create_replaces_open_clean_lineage_before_rename_and_text_edit() {
        let root = test_directory("ordered-overwrite-create-open-lineage");
        let database_root = test_directory("ordered-overwrite-create-open-lineage-store");
        fs::write(root.join("a.txt"), "old model content").unwrap();
        let store =
            crate::persistence::StateStore::open(database_root.join("state.sqlite3")).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let clean = WorkspaceEditOpenDocument {
            workspace_id: WorkspaceId::new(1),
            path: "a.txt".to_owned(),
            generation: 4,
            version: 7,
            text: "old model content".to_owned(),
            saved_text: "old model content".to_owned(),
        };
        let edit = serde_json::json!({ "documentChanges": [
            { "kind": "create", "uri": file_uri(&root.join("a.txt")), "options": { "overwrite": true } },
            { "kind": "rename", "oldUri": file_uri(&root.join("a.txt")), "newUri": file_uri(&root.join("b.txt")) },
            { "textDocument": { "uri": file_uri(&root.join("b.txt")), "version": null }, "edits": [
                { "range": range(0, 0), "newText": "new content" }
            ]}
        ]});
        let staged = manager
            .stage(&edit, &roots(&root), std::slice::from_ref(&clean))
            .unwrap();

        assert_eq!(staged.documents.len(), 1);
        assert_eq!(staged.documents[0].original_text, "");
        assert_eq!(staged.documents[0].new_text, "new content");
        assert_eq!(staged.documents[0].generation, None);
        assert_eq!(staged.documents[0].version, None);
        manager
            .commit_closed(
                staged.transaction_id,
                &staged.authorization,
                std::slice::from_ref(&clean),
            )
            .unwrap();
        assert!(!root.join("a.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "new content"
        );

        manager
            .rollback(staged.transaction_id, &staged.authorization)
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "old model content"
        );
        assert!(!root.join("b.txt").exists());
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        manager
            .acknowledge_completion(staged.transaction_id, &staged.authorization)
            .unwrap();

        let dirty = WorkspaceEditOpenDocument {
            text: "unsaved old model content".to_owned(),
            ..clean.clone()
        };
        assert!(matches!(
            manager.stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "create",
                    "uri": file_uri(&root.join("a.txt")),
                    "options": { "overwrite": true }
                }]}),
                &roots(&root),
                &[dirty],
            ),
            Err(WorkspaceEditError::Stale(_))
        ));

        let committed = manager
            .stage(&edit, &roots(&root), std::slice::from_ref(&clean))
            .unwrap();
        manager
            .commit_closed(
                committed.transaction_id,
                &committed.authorization,
                std::slice::from_ref(&clean),
            )
            .unwrap();
        manager
            .finish(committed.transaction_id, &committed.authorization)
            .unwrap();
        drop(manager);

        let reopened = WorkspaceEditTransactions::open(store).unwrap();
        let recovery = reopened
            .recoveries()
            .into_iter()
            .find(|recovery| recovery.transaction_id == committed.transaction_id)
            .unwrap();
        assert_eq!(
            reopened
                .status(committed.transaction_id, &recovery.authorization)
                .unwrap()
                .phase,
            WorkspaceEditTransactionPhase::FinishedCommitted
        );
        assert!(!root.join("a.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "new content"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn later_ordered_turn_rejects_concurrent_physical_change_and_rolls_back_prior_turns() {
        let root = test_directory("ordered-later-concurrency");
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::write(root.join("later.txt"), "staged").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "rename", "oldUri": file_uri(&root.join("a.txt")), "newUri": file_uri(&root.join("b.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("later.txt")), "newUri": file_uri(&root.join("final.txt")) }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        fs::write(root.join("later.txt"), "changed concurrently").unwrap();

        assert!(matches!(
            manager.commit_closed(staged.transaction_id, &staged.authorization, &[]),
            Err(WorkspaceEditError::Stale(_))
        ));
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a");
        assert!(!root.join("b.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("later.txt")).unwrap(),
            "changed concurrently"
        );
        assert!(!root.join("final.txt").exists());

        fs::create_dir(root.join("parent")).unwrap();
        fs::write(root.join("parent/child.txt"), "child").unwrap();
        let ancestor = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "rename", "oldUri": file_uri(&root.join("parent/child.txt")), "newUri": file_uri(&root.join("parent/renamed.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("parent")), "newUri": file_uri(&root.join("moved")) }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        fs::write(root.join("parent/concurrent.txt"), "concurrent").unwrap();
        assert!(matches!(
            manager.commit_closed(ancestor.transaction_id, &ancestor.authorization, &[]),
            Err(WorkspaceEditError::Stale(_))
        ));
        assert!(root.join("parent/child.txt").is_file());
        assert!(root.join("parent/concurrent.txt").is_file());
        assert!(!root.join("parent/renamed.txt").exists());
        assert!(!root.join("moved").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ordered_refresh_failure_durably_rolls_back_in_reverse() {
        let root = test_directory("ordered-refresh-fault-workspace");
        let database_root = test_directory("ordered-refresh-fault-store");
        let store =
            crate::persistence::StateStore::open(database_root.join("state.sqlite3")).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "create", "uri": file_uri(&root.join("created.txt")) },
                    { "kind": "rename", "oldUri": file_uri(&root.join("created.txt")), "newUri": file_uri(&root.join("renamed.txt")) }
                ]}),
                &roots(&root),
                &[],
            )
            .unwrap();

        FAIL_NEXT_RESOURCE_REFRESH.set(true);
        assert!(
            manager
                .commit_closed(staged.transaction_id, &staged.authorization, &[])
                .is_err()
        );
        assert_eq!(
            manager
                .status(staged.transaction_id, &staged.authorization)
                .unwrap()
                .phase,
            WorkspaceEditTransactionPhase::RolledBack
        );
        assert!(!root.join("created.txt").exists());
        assert!(!root.join("renamed.txt").exists());
        let encoded = &store.workspace_edit_journals().unwrap()[0].1;
        let journal: WorkspaceEditJournal = serde_json::from_str(encoded).unwrap();
        assert_eq!(journal.phase, JournalPhase::RolledBack);
        assert!(
            journal
                .operations
                .iter()
                .all(|operation| { operation.rollback_phase() == JournalRollbackPhase::Restored })
        );

        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn destructive_resources_reject_dirty_open_files_and_recursive_changes() {
        let root = test_directory("resource-concurrency");
        fs::create_dir(root.join("directory")).unwrap();
        fs::write(root.join("directory/file.txt"), "saved").unwrap();
        let manager = WorkspaceEditTransactions::new();
        let dirty = WorkspaceEditOpenDocument {
            workspace_id: WorkspaceId::new(1),
            path: "directory/file.txt".to_owned(),
            generation: 1,
            version: 2,
            text: "unsaved".to_owned(),
            saved_text: "saved".to_owned(),
        };
        assert!(matches!(
            manager.stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "delete",
                    "uri": file_uri(&root.join("directory")),
                    "options": { "recursive": true }
                }]}),
                &roots(&root),
                &[dirty],
            ),
            Err(WorkspaceEditError::Stale(_))
        ));

        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "delete",
                    "uri": file_uri(&root.join("directory")),
                    "options": { "recursive": true }
                }]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        fs::write(root.join("directory/file.txt"), "changed concurrently").unwrap();
        assert!(matches!(
            manager.commit_closed(staged.transaction_id, &staged.authorization, &[]),
            Err(WorkspaceEditError::Stale(_))
        ));
        assert_eq!(
            fs::read_to_string(root.join("directory/file.txt")).unwrap(),
            "changed concurrently"
        );
        manager
            .finish(staged.transaction_id, &staged.authorization)
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fresh_store_retains_rolled_back_crash_outcome_until_acknowledged() {
        let root = test_directory("crash-journal-workspace");
        let database_root = test_directory("crash-journal-store");
        let database = database_root.join("state.sqlite3");
        fs::write(root.join("source.txt"), "source").unwrap();
        fs::write(root.join("destination.txt"), "destination").unwrap();
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "rename",
                    "oldUri": file_uri(&root.join("source.txt")),
                    "newUri": file_uri(&root.join("destination.txt")),
                    "options": { "overwrite": true }
                }]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        assert_eq!(store.workspace_edit_journals().unwrap().len(), 1);
        drop(manager);

        let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
        let reopened = WorkspaceEditTransactions::open(reopened_store.clone()).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("source.txt")).unwrap(),
            "source"
        );
        assert_eq!(
            fs::read_to_string(root.join("destination.txt")).unwrap(),
            "destination"
        );
        assert!(reopened_store.workspace_edit_journals().unwrap().is_empty());
        assert!(matches!(
            reopened.status(staged.transaction_id, &staged.authorization),
            Err(WorkspaceEditError::Invalid(_))
        ));
        let recovery = reopened.recoveries().into_iter().next().unwrap();
        assert_eq!(
            recovery.status.phase,
            WorkspaceEditTransactionPhase::FinishedRolledBack
        );
        reopened
            .acknowledge_completion(staged.transaction_id, &recovery.authorization)
            .unwrap();
        assert!(reopened_store.workspace_edit_outcomes().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn fresh_store_retains_committed_crash_outcome_until_acknowledgement() {
        let root = test_directory("committed-crash-outcome-workspace");
        let database_root = test_directory("committed-crash-outcome-store");
        let database = database_root.join("state.sqlite3");
        fs::write(root.join("target.txt"), "before").unwrap();
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&root.join("target.txt")): [{
                        "range": range(0, 6),
                        "newText": "after"
                    }]
                }}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        fail_next_cleanup_at(CleanupFault::AfterMarkDiscarding);
        assert!(
            manager
                .finish(staged.transaction_id, &staged.authorization)
                .is_err()
        );
        drop(manager);

        let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
        let reopened = WorkspaceEditTransactions::open(reopened_store.clone()).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("target.txt")).unwrap(),
            "after"
        );
        assert!(matches!(
            reopened.status(staged.transaction_id, &staged.authorization),
            Err(WorkspaceEditError::Invalid(_))
        ));
        let recovery = reopened.recoveries().into_iter().next().unwrap();
        assert_eq!(
            recovery.status.phase,
            WorkspaceEditTransactionPhase::FinishedCommitted
        );
        reopened
            .acknowledge_completion(staged.transaction_id, &recovery.authorization)
            .unwrap();
        drop(reopened);
        let after_lost_ack = WorkspaceEditTransactions::open(reopened_store.clone()).unwrap();
        after_lost_ack
            .acknowledge_completion(staged.transaction_id, &recovery.authorization)
            .unwrap();
        assert!(reopened_store.workspace_edit_outcomes().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn successful_finish_is_queryable_and_acknowledgeable_after_response_loss_and_restart() {
        let root = test_directory("finish-response-loss-workspace");
        let database_root = test_directory("finish-response-loss-store");
        let database = database_root.join("state.sqlite3");
        fs::write(root.join("target.txt"), "before").unwrap();
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "changes": {
                    file_uri(&root.join("target.txt")): [{
                        "range": range(0, 6), "newText": "after"
                    }]
                }}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
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
        drop(manager);

        let reopened = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let recovery = reopened.recoveries().into_iter().next().unwrap();
        assert_eq!(
            recovery.status.phase,
            WorkspaceEditTransactionPhase::FinishedCommitted
        );
        reopened
            .finish(staged.transaction_id, &recovery.authorization)
            .unwrap();
        reopened
            .acknowledge_completion(staged.transaction_id, &recovery.authorization)
            .unwrap();
        drop(reopened);
        let after_ack_loss = WorkspaceEditTransactions::open(store).unwrap();
        after_ack_loss
            .acknowledge_completion(staged.transaction_id, &recovery.authorization)
            .unwrap();
        assert!(after_ack_loss.recoveries().is_empty());
        assert_eq!(
            fs::read_to_string(root.join("target.txt")).unwrap(),
            "after"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn expired_orphaned_recoveries_do_not_permanently_exhaust_transaction_capacity() {
        let root = test_directory("expired-recovery-capacity-workspace");
        let database_root = test_directory("expired-recovery-capacity-store");
        let database = database_root.join("state.sqlite3");
        let store = crate::persistence::StateStore::open(&database).unwrap();
        for transaction_id in 1..=MAX_WORKSPACE_EDIT_TRANSACTIONS as u64 {
            persist_outcome(
                &store,
                transaction_id,
                &WorkspaceEditOutcome {
                    authorization_hash: content_hash("lost authorization"),
                    authorization_expires_at: None,
                    phase: WorkspaceEditTransactionPhase::RecoveryRequired,
                    created_at: 0,
                    operations: Vec::new(),
                },
            )
            .unwrap();
        }

        let reopened = WorkspaceEditTransactions::open(store).unwrap();
        assert!(reopened.recoveries().is_empty());
        let staged = reopened
            .stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "create",
                    "uri": file_uri(&root.join("new.txt"))
                }]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        assert!(staged.transaction_id > MAX_WORKSPACE_EDIT_TRANSACTIONS as u64);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn crash_between_create_install_and_journal_update_removes_only_prepared_inode() {
        let root = test_directory("crash-create-workspace");
        let database_root = test_directory("crash-create-store");
        let database = database_root.join("state.sqlite3");
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "create",
                    "uri": file_uri(&root.join("created.txt"))
                }]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        {
            let mut transactions = manager.transactions.lock().unwrap();
            let transaction = transactions.get_mut(&staged.transaction_id).unwrap();
            let authorization = transaction.authorization.clone();
            set_operation_phase(transaction, 0, JournalOperationPhase::Applying);
            save_journal(
                Some(&store),
                staged.transaction_id,
                transaction,
                JournalPhase::Applying,
            )
            .unwrap();
            {
                let TransactionOperation::Resource(operation) = &mut transaction.operations[0]
                else {
                    unreachable!()
                };
                assert!(prepare_create_operation(operation, &authorization).unwrap());
            }
            save_journal(
                Some(&store),
                staged.transaction_id,
                transaction,
                JournalPhase::Applying,
            )
            .unwrap();
            let TransactionOperation::Resource(operation) = &mut transaction.operations[0] else {
                unreachable!()
            };
            apply_resource_operation(operation, &authorization, 0).unwrap();
        }
        assert!(root.join("created.txt").exists());
        drop(manager);

        let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
        let _reopened = WorkspaceEditTransactions::open(reopened_store).unwrap();
        assert!(!root.join("created.txt").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn create_rollback_never_deletes_an_empty_replacement() {
        let root = test_directory("create-replacement-race");
        let manager = WorkspaceEditTransactions::new();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "create",
                    "uri": file_uri(&root.join("created.txt"))
                }]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        let _created_file = fs::File::open(root.join("created.txt")).unwrap();
        fs::remove_file(root.join("created.txt")).unwrap();
        fs::write(root.join("created.txt"), "").unwrap();

        assert!(matches!(
            manager.rollback(staged.transaction_id, &staged.authorization),
            Err(WorkspaceEditError::Recovery(_))
        ));
        assert!(root.join("created.txt").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_crash_recovery_never_deletes_an_empty_replacement() {
        let root = test_directory("create-crash-replacement-workspace");
        let database_root = test_directory("create-crash-replacement-store");
        let database = database_root.join("state.sqlite3");
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "create",
                    "uri": file_uri(&root.join("created.txt"))
                }]}),
                &roots(&root),
                &[],
            )
            .unwrap();
        manager
            .commit_closed(staged.transaction_id, &staged.authorization, &[])
            .unwrap();
        let encoded = &store.workspace_edit_journals().unwrap()[0].1;
        let journal: WorkspaceEditJournal = serde_json::from_str(encoded).unwrap();
        assert!(matches!(
            journal.operations[0],
            JournalOperation::Create {
                installed: Some(_),
                ..
            }
        ));
        let _created_file = fs::File::open(root.join("created.txt")).unwrap();
        fs::remove_file(root.join("created.txt")).unwrap();
        fs::write(root.join("created.txt"), "").unwrap();
        drop(manager);

        let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
        let reopened = WorkspaceEditTransactions::open(reopened_store).unwrap();
        let recovery = reopened.recoveries().into_iter().next().unwrap();
        assert_ne!(recovery.authorization, staged.authorization);
        assert!(matches!(
            reopened.status(staged.transaction_id, &staged.authorization),
            Err(WorkspaceEditError::Invalid(_))
        ));
        let status = reopened
            .status(staged.transaction_id, &recovery.authorization)
            .unwrap();
        assert_eq!(
            status.phase,
            WorkspaceEditTransactionPhase::RecoveryRequired
        );
        assert!(status.retry_rollback);
        assert!(status.can_finalize);
        assert!(root.join("created.txt").is_file());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn recovery_replay_converges_at_each_restore_boundary() {
        for boundary in 0..3 {
            let root = test_directory(&format!("restore-boundary-{boundary}-workspace"));
            let database_root = test_directory(&format!("restore-boundary-{boundary}-store"));
            let database = database_root.join("state.sqlite3");
            fs::write(root.join("target.txt"), "original").unwrap();
            let store = crate::persistence::StateStore::open(&database).unwrap();
            let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
            let staged = manager
                .stage(
                    &serde_json::json!({ "documentChanges": [{
                        "kind": "create",
                        "uri": file_uri(&root.join("target.txt")),
                        "options": { "overwrite": true }
                    }]}),
                    &roots(&root),
                    &[],
                )
                .unwrap();
            manager
                .commit_closed(staged.transaction_id, &staged.authorization, &[])
                .unwrap();
            let encoded = &store.workspace_edit_journals().unwrap()[0].1;
            let mut journal: WorkspaceEditJournal = serde_json::from_str(encoded).unwrap();
            journal.phase = JournalPhase::RollingBack;
            journal.operations[0].set_rollback_phase(JournalRollbackPhase::RemovingApplied);
            persist_journal(&store, staged.transaction_id, &journal).unwrap();
            remove_recovered_applied(&journal.authorization, 0, &journal.operations[0]).unwrap();
            journal.operations[0].set_rollback_phase(JournalRollbackPhase::RestoringBackup);
            persist_journal(&store, staged.transaction_id, &journal).unwrap();
            if boundary >= 1 {
                restore_recovered_backup(&journal.authorization, 0, &journal.operations[0])
                    .unwrap();
            }
            if boundary == 2 {
                journal.operations[0].set_rollback_phase(JournalRollbackPhase::Restored);
                journal.phase = JournalPhase::RolledBack;
                persist_journal(&store, staged.transaction_id, &journal).unwrap();
            }
            drop(manager);

            let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
            let _reopened = WorkspaceEditTransactions::open(reopened_store.clone()).unwrap();
            assert_eq!(
                fs::read_to_string(root.join("target.txt")).unwrap(),
                "original"
            );
            assert!(reopened_store.workspace_edit_journals().unwrap().is_empty());
            let _ = fs::remove_dir_all(root);
            let _ = fs::remove_dir_all(database_root);
        }
    }

    #[test]
    fn post_syscall_helper_failures_are_rolled_back_and_restart_cleanly() {
        use crate::language_servers::secure_edit::{SecureEditFault, fail_next_secure_edit_at};

        for (name, fault) in [
            ("prepare", SecureEditFault::Prepare),
            ("install", SecureEditFault::InstallPrepared),
            ("rename", SecureEditFault::Rename),
            ("stage-remove", SecureEditFault::StageRemove),
        ] {
            let root = test_directory(&format!("post-syscall-{name}-workspace"));
            let database_root = test_directory(&format!("post-syscall-{name}-store"));
            let database = database_root.join("state.sqlite3");
            fs::write(root.join("source.txt"), "source").unwrap();
            let edit = match fault {
                SecureEditFault::Prepare | SecureEditFault::InstallPrepared => {
                    serde_json::json!({ "documentChanges": [{
                    "kind": "create", "uri": file_uri(&root.join("created.txt"))
                }] })
                }
                SecureEditFault::Rename => serde_json::json!({ "documentChanges": [{
                    "kind": "rename",
                    "oldUri": file_uri(&root.join("source.txt")),
                    "newUri": file_uri(&root.join("renamed.txt"))
                }] }),
                SecureEditFault::StageRemove => serde_json::json!({ "documentChanges": [{
                    "kind": "delete", "uri": file_uri(&root.join("source.txt"))
                }] }),
                SecureEditFault::PrepareBeforeIdentity | SecureEditFault::RecursiveDiscard => {
                    unreachable!()
                }
            };
            let store = crate::persistence::StateStore::open(&database).unwrap();
            let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
            let staged = manager.stage(&edit, &roots(&root), &[]).unwrap();
            fail_next_secure_edit_at(fault);
            assert!(
                manager
                    .commit_closed(staged.transaction_id, &staged.authorization, &[])
                    .is_err()
            );
            assert_eq!(
                fs::read_to_string(root.join("source.txt")).unwrap(),
                "source"
            );
            assert!(!root.join("created.txt").exists());
            assert!(!root.join("renamed.txt").exists());
            drop(manager);

            let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
            let _reopened = WorkspaceEditTransactions::open(reopened_store.clone()).unwrap();
            assert!(reopened_store.workspace_edit_journals().unwrap().is_empty());
            assert_eq!(
                fs::read_to_string(root.join("source.txt")).unwrap(),
                "source"
            );
            let _ = fs::remove_dir_all(root);
            let _ = fs::remove_dir_all(database_root);
        }
    }

    #[test]
    fn startup_recovers_prepared_file_created_before_identity_was_journaled() {
        use crate::language_servers::secure_edit::{
            SecureEditFault, TRANSACTION_PATH_PREFIX, fail_next_secure_edit_at,
        };

        let root = test_directory("prepared-identity-gap-workspace");
        let database_root = test_directory("prepared-identity-gap-store");
        let database = database_root.join("state.sqlite3");
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [{
                    "kind": "create", "uri": file_uri(&root.join("created.txt"))
                }] }),
                &roots(&root),
                &[],
            )
            .unwrap();
        {
            let mut transactions = manager.transactions.lock().unwrap();
            let transaction = transactions.get_mut(&staged.transaction_id).unwrap();
            set_operation_phase(transaction, 0, JournalOperationPhase::Applying);
            save_journal(
                Some(&store),
                staged.transaction_id,
                transaction,
                JournalPhase::Applying,
            )
            .unwrap();
            let TransactionOperation::Resource(operation) = &mut transaction.operations[0] else {
                panic!("expected create transaction operation");
            };
            fail_next_secure_edit_at(SecureEditFault::PrepareBeforeIdentity);
            assert!(prepare_create_operation(operation, &staged.authorization).is_err());
        }
        let recovery = root.join(format!("{TRANSACTION_PATH_PREFIX}{}", staged.authorization));
        assert!(recovery.join("prepared-0").is_file());
        assert!(!root.join("created.txt").exists());
        let encoded = &store.workspace_edit_journals().unwrap()[0].1;
        let journal: WorkspaceEditJournal = serde_json::from_str(encoded).unwrap();
        let JournalOperation::Create {
            prepared_name,
            prepared,
            ..
        } = &journal.operations[0]
        else {
            panic!("expected create journal operation");
        };
        assert_eq!(prepared_name.as_deref(), Some("prepared-0"));
        assert_eq!(*prepared, None);
        drop(manager);
        drop(store);

        let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
        let _reopened = WorkspaceEditTransactions::open(reopened_store.clone()).unwrap();

        assert!(!recovery.exists());
        assert!(!root.join("created.txt").exists());
        assert!(reopened_store.workspace_edit_journals().unwrap().is_empty());
        let outcomes = load_workspace_edit_outcomes(&reopened_store).unwrap();
        assert_eq!(
            outcomes[&staged.transaction_id].phase,
            WorkspaceEditTransactionPhase::FinishedRolledBack
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn not_started_empty_rename_is_skipped_after_created_source_crash() {
        let root = test_directory("not-started-empty-rename-workspace");
        let database_root = test_directory("not-started-empty-rename-store");
        let database = database_root.join("state.sqlite3");
        fs::write(root.join("destination.txt"), "").unwrap();
        let destination_before = fs::metadata(root.join("destination.txt")).unwrap();
        let store = crate::persistence::StateStore::open(&database).unwrap();
        let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
        let staged = manager
            .stage(
                &serde_json::json!({ "documentChanges": [
                    { "kind": "create", "uri": file_uri(&root.join("source.txt")) },
                    {
                        "kind": "rename",
                        "oldUri": file_uri(&root.join("source.txt")),
                        "newUri": file_uri(&root.join("destination.txt")),
                        "options": { "overwrite": true }
                    }
                ] }),
                &roots(&root),
                &[],
            )
            .unwrap();
        {
            let mut transactions = manager.transactions.lock().unwrap();
            let transaction = transactions.get_mut(&staged.transaction_id).unwrap();
            let authorization = transaction.authorization.clone();
            set_operation_phase(transaction, 0, JournalOperationPhase::Applying);
            save_journal(
                Some(&store),
                staged.transaction_id,
                transaction,
                JournalPhase::Applying,
            )
            .unwrap();
            let TransactionOperation::Resource(operation) = &mut transaction.operations[0] else {
                unreachable!()
            };
            prepare_create_operation(operation, &authorization).unwrap();
            apply_resource_operation(operation, &authorization, 0).unwrap();
            set_operation_phase(transaction, 0, JournalOperationPhase::Applied);
            let TransactionOperation::Resource(operation) = &mut transaction.operations[1] else {
                unreachable!()
            };
            validate_and_refresh_resource_snapshot(operation).unwrap();
            save_journal(
                Some(&store),
                staged.transaction_id,
                transaction,
                JournalPhase::Applying,
            )
            .unwrap();
        }
        assert!(root.join("source.txt").exists());
        drop(manager);

        let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
        let _reopened = WorkspaceEditTransactions::open(reopened_store).unwrap();
        assert!(!root.join("source.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("destination.txt")).unwrap(),
            ""
        );
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(root.join("destination.txt")).unwrap().ino(),
            destination_before.ino()
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(database_root);
    }

    #[test]
    fn recursive_cleanup_restarts_from_every_durable_phase() {
        use crate::language_servers::secure_edit::{SecureEditFault, fail_next_secure_edit_at};

        for boundary in 0..4 {
            let root = test_directory(&format!("cleanup-{boundary}-workspace"));
            let database_root = test_directory(&format!("cleanup-{boundary}-store"));
            let database = database_root.join("state.sqlite3");
            fs::create_dir(root.join("directory")).unwrap();
            fs::write(root.join("directory/first.txt"), "first").unwrap();
            fs::write(root.join("directory/second.txt"), "second").unwrap();
            let store = crate::persistence::StateStore::open(&database).unwrap();
            let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
            let staged = manager
                .stage(
                    &serde_json::json!({ "documentChanges": [{
                        "kind": "delete",
                        "uri": file_uri(&root.join("directory")),
                        "options": { "recursive": true }
                    }] }),
                    &roots(&root),
                    &[],
                )
                .unwrap();
            manager
                .commit_closed(staged.transaction_id, &staged.authorization, &[])
                .unwrap();
            match boundary {
                0 => fail_next_cleanup_at(CleanupFault::AfterMarkDiscarding),
                1 => fail_next_secure_edit_at(SecureEditFault::RecursiveDiscard),
                2 => fail_next_cleanup_at(CleanupFault::BeforeMarkDiscarded),
                3 => fail_next_cleanup_at(CleanupFault::AfterMarkDiscarded),
                _ => unreachable!(),
            }
            assert!(
                manager
                    .finish(staged.transaction_id, &staged.authorization)
                    .is_err()
            );
            drop(manager);

            let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
            let reopened = WorkspaceEditTransactions::open(reopened_store.clone()).unwrap();
            assert!(!root.join("directory").exists());
            assert!(reopened_store.workspace_edit_journals().unwrap().is_empty());
            let recovery = reopened.recoveries().into_iter().next().unwrap();
            assert_eq!(
                recovery.status.phase,
                WorkspaceEditTransactionPhase::FinishedCommitted
            );
            reopened
                .acknowledge_completion(staged.transaction_id, &recovery.authorization)
                .unwrap();
            let _ = fs::remove_dir_all(root);
            let _ = fs::remove_dir_all(database_root);
        }
    }

    #[test]
    fn discarded_overwrite_backup_makes_cleanup_retry_only_and_convergent() {
        for recovery in ["retry-finalize", "startup"] {
            let root = test_directory(&format!("committed-cleanup-{recovery}-workspace"));
            let database_root = test_directory(&format!("committed-cleanup-{recovery}-store"));
            let database = database_root.join("state.sqlite3");
            fs::write(root.join("first.txt"), "first original").unwrap();
            fs::write(root.join("second.txt"), "second original").unwrap();
            let store = crate::persistence::StateStore::open(&database).unwrap();
            let manager = WorkspaceEditTransactions::open(store.clone()).unwrap();
            let staged = manager
                .stage(
                    &serde_json::json!({ "documentChanges": [
                        { "textDocument": { "uri": file_uri(&root.join("first.txt")), "version": null }, "edits": [
                            { "range": range(0, 14), "newText": "first desired" }
                        ]},
                        { "textDocument": { "uri": file_uri(&root.join("second.txt")), "version": null }, "edits": [
                            { "range": range(0, 15), "newText": "second desired" }
                        ]}
                    ]}),
                    &roots(&root),
                    &[],
                )
                .unwrap();
            manager
                .commit_closed(staged.transaction_id, &staged.authorization, &[])
                .unwrap();

            fail_next_cleanup_at(CleanupFault::AfterMarkDiscarded);
            assert!(
                manager
                    .finish(staged.transaction_id, &staged.authorization)
                    .is_err()
            );
            let status = manager
                .status(staged.transaction_id, &staged.authorization)
                .unwrap();
            assert_eq!(
                status.phase,
                WorkspaceEditTransactionPhase::CommittedCleanupRequired
            );
            assert!(!status.retry_rollback);
            assert!(status.can_finalize);
            assert!(matches!(
                manager.rollback(staged.transaction_id, &staged.authorization),
                Err(WorkspaceEditError::Invalid(_))
            ));
            assert_eq!(
                fs::read_to_string(root.join("first.txt")).unwrap(),
                "first desired"
            );
            assert_eq!(
                fs::read_to_string(root.join("second.txt")).unwrap(),
                "second desired"
            );

            let encoded = &store.workspace_edit_journals().unwrap()[0].1;
            let journal: WorkspaceEditJournal = serde_json::from_str(encoded).unwrap();
            assert_eq!(
                journal
                    .operations
                    .iter()
                    .filter(|operation| {
                        operation.cleanup_phase() == JournalCleanupPhase::Discarded
                    })
                    .count(),
                1
            );
            let remaining = journal
                .operations
                .iter()
                .enumerate()
                .find(|(_, operation)| operation.cleanup_phase() != JournalCleanupPhase::Discarded)
                .unwrap();
            validate_committed_backup(&journal.authorization, remaining.0, remaining.1).unwrap();

            if recovery == "retry-finalize" {
                let finalized = manager
                    .finalize(staged.transaction_id, &staged.authorization)
                    .unwrap();
                assert_eq!(
                    finalized.phase,
                    WorkspaceEditTransactionPhase::FinishedCommitted
                );
            } else {
                drop(manager);
                let reopened = WorkspaceEditTransactions::open(store.clone()).unwrap();
                let recovered = reopened.recoveries().into_iter().next().unwrap();
                assert_eq!(
                    recovered.status.phase,
                    WorkspaceEditTransactionPhase::FinishedCommitted
                );
            }
            assert!(store.workspace_edit_journals().unwrap().is_empty());
            assert_eq!(
                fs::read_to_string(root.join("first.txt")).unwrap(),
                "first desired"
            );
            assert_eq!(
                fs::read_to_string(root.join("second.txt")).unwrap(),
                "second desired"
            );
            let _ = fs::remove_dir_all(root);
            let _ = fs::remove_dir_all(database_root);
        }
    }

    fn range(start: u32, end: u32) -> Value {
        serde_json::json!({ "start": { "line": 0, "character": start }, "end": { "line": 0, "character": end } })
    }

    fn open_document(path: &str, text: &str) -> WorkspaceEditOpenDocument {
        WorkspaceEditOpenDocument {
            workspace_id: WorkspaceId::new(1),
            path: path.to_owned(),
            generation: 4,
            version: 9,
            text: text.to_owned(),
            saved_text: text.to_owned(),
        }
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

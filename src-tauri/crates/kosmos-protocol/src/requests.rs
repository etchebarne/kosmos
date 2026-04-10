use serde::{Deserialize, Serialize};

/// All requests the host can send to the remote agent.
///
/// Wire format (with `RequestMessage`):
/// ```json
/// { "id": 1, "method": "ReadDir", "params": { "path": "..." } }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    // ── File tree ──
    ReadDir {
        path: String,
    },
    MoveFile {
        source: String,
        dest_dir: String,
    },
    CreateFile {
        path: String,
    },
    CreateDir {
        path: String,
    },
    RenameEntry {
        path: String,
        new_name: String,
    },
    CopyEntry {
        source: String,
        dest_dir: String,
    },
    TrashEntry {
        path: String,
    },
    DeleteEntry {
        path: String,
    },

    // ── Editor ──
    ReadFile {
        path: String,
    },
    WriteFile {
        path: String,
        content: String,
    },

    // ── Git ──
    GetGitBranch {
        path: String,
    },
    GetGitRemoteOwner {
        path: String,
    },
    GetGitStatus {
        path: String,
    },
    GitStage {
        path: String,
        files: Vec<String>,
    },
    GitUnstage {
        path: String,
        files: Vec<String>,
    },
    GitStageAll {
        path: String,
    },
    GitCommit {
        path: String,
        message: String,
    },
    GitListBranches {
        path: String,
    },
    GitCheckout {
        path: String,
        branch: String,
    },
    GitDeleteBranch {
        path: String,
        branch: String,
    },
    GitDiscard {
        path: String,
        files: Vec<String>,
    },
    GitTrashUntracked {
        path: String,
        files: Vec<String>,
    },
    GitStashAll {
        path: String,
    },
    GitStashFiles {
        path: String,
        files: Vec<String>,
    },
    GitStashList {
        path: String,
    },
    GitStashShow {
        path: String,
        index: usize,
    },
    GitStashPop {
        path: String,
        index: usize,
    },
    GitStashDrop {
        path: String,
        index: usize,
    },
    GitDiscardAllTracked {
        path: String,
    },
    GitTrashAllUntracked {
        path: String,
    },
    GitDiff {
        path: String,
        file: String,
        staged: bool,
    },
    GitDiffUntracked {
        path: String,
        file: String,
    },
    GitInit {
        path: String,
    },
    GitFetch {
        path: String,
    },
    GitPull {
        path: String,
    },
    GitPullRebase {
        path: String,
    },
    GitPush {
        path: String,
    },
    GitForcePush {
        path: String,
    },

    // ── Search ──
    ListWorkspaceFiles {
        path: String,
    },
    SearchInFiles {
        path: String,
        query: String,
        max_results: Option<usize>,
    },

    // ── Workspace watcher ──
    WatchWorkspace {
        path: String,
    },
    UnwatchWorkspace,

    // ── Terminal ──
    TerminalListShells,
    /// List running terminal IDs (used for reconnection to daemon).
    TerminalList,
    TerminalSpawn {
        id: String,
        program: String,
        args: Vec<String>,
        cwd: String,
        cols: u16,
        rows: u16,
    },
    TerminalWrite {
        id: String,
        data: String,
    },
    TerminalResize {
        id: String,
        cols: u16,
        rows: u16,
    },
    TerminalClose {
        id: String,
    },

    // ── LSP ──
    LspStart {
        workspace_path: String,
        language_id: String,
    },
    LspSend {
        server_id: String,
        message: String,
    },
    LspStop {
        server_id: String,
    },
    LspStopWorkspace {
        workspace_path: String,
    },
    LspCheckAvailability {
        workspace_path: String,
    },
    LspScanProjects {
        workspace_path: String,
    },
    LspResolveRoot {
        file_path: String,
        language_id: String,
        workspace_path: String,
    },
    LspLanguageGroups,
    LspInstalledList,
    LspInstallServer {
        name: String,
    },
    LspUninstallServer {
        name: String,
    },

    /// Keepalive ping — agent replies immediately.
    Ping,
}

/// Wire format for a request message.
///
/// ```json
/// { "id": 1, "request": { "method": "ReadDir", "params": { "path": "..." } } }
/// ```
///
/// Note: `request` is a nested field, NOT flattened. serde's `flatten` doesn't
/// work with adjacently tagged enums for deserialization.
#[derive(Serialize, Deserialize, Debug)]
pub struct RequestMessage {
    pub id: u64,
    pub request: Request,
}

/// Wire format for a response message.
#[derive(Serialize, Deserialize, Debug)]
pub struct ResponseMessage {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ResponseMessage {
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, error: String) -> Self {
        Self {
            id,
            result: None,
            error: Some(error),
        }
    }
}

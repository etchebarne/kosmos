use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Git ──

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChange {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub additions: i32,
    pub deletions: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusInfo {
    pub changes: Vec<GitFileChange>,
    pub branch: Option<String>,
    pub remote_branch: Option<String>,
    pub last_commit_message: Option<String>,
    pub has_remote: bool,
    pub is_repo: bool,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchInfo {
    pub name: String,
    pub is_remote: bool,
    pub is_current: bool,
    pub last_commit_date: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    pub index: usize,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitStashFile {
    pub path: String,
    pub status: String,
}

// ── File tree ──

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub extension: Option<String>,
}

// ── Terminal ──

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShellInfo {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

// ── LSP ──

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LspStartResult {
    pub server_id: String,
    pub server_name: String,
    pub server_language: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerAvailability {
    pub language_id: String,
    pub server_name: String,
    pub available: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProject {
    pub language_id: String,
    pub server_name: String,
    pub project_root: String,
    pub available: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstalledServer {
    pub name: String,
    pub version: String,
    pub source_type: String,
    pub bin_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegistryEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub languages: Vec<String>,
    pub source_id: String,
    #[serde(default)]
    pub source_type: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub bin: Option<String>,
    #[serde(default)]
    pub extra_packages: Option<Vec<String>>,
    #[serde(default)]
    pub assets: Option<HashMap<String, PlatformAsset>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlatformAsset {
    pub file: String,
    pub bin: Option<String>,
}

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::Deserialize;

use kosmos_protocol::types::{DetectedProject, ServerAvailability};

#[cfg(feature = "installer")]
use super::installer;

#[derive(Deserialize)]
struct ServerEntry {
    languages: Vec<String>,
    server: String,
    bin: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    markers: Vec<String>,
    #[serde(default, rename = "companionTo")]
    companion_to: Vec<String>,
}

const SERVERS_JSON: &str = include_str!("servers.json");

static SERVERS: LazyLock<Vec<ServerEntry>> = LazyLock::new(|| {
    serde_json::from_str(SERVERS_JSON).expect("Failed to parse servers.json")
});

pub struct ServerConfig {
    pub language_id: String,
    pub server_name: String,
    pub command: String,
    pub args: Vec<String>,
}

fn find_entry(language_id: &str) -> Option<&'static ServerEntry> {
    SERVERS
        .iter()
        .find(|e| e.languages.iter().any(|l| l == language_id))
}

pub fn resolve_server_for_language(language_id: &str) -> Option<ServerConfig> {
    let entry = find_entry(language_id)?;
    Some(ServerConfig {
        language_id: entry.languages[0].clone(),
        server_name: entry.server.clone(),
        command: entry.bin.clone(),
        args: entry.args.clone(),
    })
}

pub fn server_name_for_language(language_id: &str) -> Option<&'static str> {
    find_entry(language_id).map(|e| e.server.as_str())
}

pub fn server_language_group(language_id: &str) -> &str {
    match find_entry(language_id) {
        Some(entry) => &entry.languages[0],
        None => language_id,
    }
}

pub fn language_groups() -> HashMap<String, String> {
    let mut groups = HashMap::new();
    for entry in SERVERS.iter() {
        let group = &entry.languages[0];
        for lang in &entry.languages[1..] {
            groups.insert(lang.clone(), group.clone());
        }
    }
    groups
}

/// Returns companion server language → list of primary server language groups it serves.
/// E.g. { "tailwindcss": ["typescript", "css", "html", ...] }
pub fn companion_servers() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for entry in SERVERS.iter() {
        if entry.companion_to.is_empty() {
            continue;
        }
        map.insert(entry.languages[0].clone(), entry.companion_to.clone());
    }
    map
}

// ── Binary availability ──

pub fn is_server_available(servers_dir: &Path, command: &str) -> bool {
    #[cfg(feature = "installer")]
    if installer::find_installed_binary(servers_dir, command).is_some() {
        return true;
    }
    let _ = servers_dir; // suppress unused warning when installer disabled
    which::which(command).is_ok()
}

pub fn resolve_command(servers_dir: &Path, command: &str) -> String {
    #[cfg(feature = "installer")]
    if let Some(local_path) = installer::find_installed_binary(servers_dir, command) {
        return local_path.to_string_lossy().to_string();
    }
    let _ = servers_dir;
    command.to_string()
}

pub fn check_availability(
    servers_dir: &Path,
    workspace_path: &str,
) -> Vec<ServerAvailability> {
    detect_workspace_languages(workspace_path)
        .into_iter()
        .filter_map(|lang| {
            let config = resolve_server_for_language(&lang)?;
            Some(ServerAvailability {
                available: is_server_available(servers_dir, &config.command),
                server_name: config.server_name,
                language_id: config.language_id,
            })
        })
        .collect()
}

// ── Deep workspace scanning ──

const MAX_SCAN_DEPTH: usize = 5;
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "vendor",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    "out",
    ".output",
    ".nuxt",
    ".svelte-kit",
    "coverage",
    ".cache",
    "tmp",
    ".tmp",
];

pub fn scan_workspace_projects(
    servers_dir: &Path,
    workspace_path: &str,
) -> Vec<DetectedProject> {
    let mut marker_groups: Vec<(&[String], &str)> = Vec::new();
    let mut seen_groups = HashSet::new();
    for entry in SERVERS.iter() {
        if entry.markers.is_empty() {
            continue;
        }
        let group = entry.languages[0].as_str();
        if seen_groups.insert(group) {
            marker_groups.push((&entry.markers, group));
        }
    }

    let mut found: HashMap<String, String> = HashMap::new();
    let mut stack: Vec<(PathBuf, usize)> =
        vec![(Path::new(workspace_path).to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        for (markers, group) in &marker_groups {
            if markers.iter().any(|m| dir.join(m).exists()) {
                let group = group.to_string();
                let dir_str = dir.to_string_lossy().to_string();
                found
                    .entry(group)
                    .and_modify(|existing| {
                        if dir_str.len() < existing.len() {
                            *existing = dir_str.clone();
                        }
                    })
                    .or_insert(dir_str);
            }
        }

        if depth >= MAX_SCAN_DEPTH {
            continue;
        }

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if SKIP_DIRS.contains(&name_str.as_ref()) || name_str.starts_with('.') {
                continue;
            }
            stack.push((path, depth + 1));
        }
    }

    found
        .into_iter()
        .filter_map(|(group, project_root)| {
            let config = resolve_server_for_language(&group)?;
            Some(DetectedProject {
                available: is_server_available(servers_dir, &config.command),
                server_name: config.server_name,
                language_id: group,
                project_root,
            })
        })
        .collect()
}

pub fn find_project_root(file_path: &str, language_id: &str, workspace_root: &str) -> String {
    let markers = match find_entry(language_id) {
        Some(entry) if !entry.markers.is_empty() => &entry.markers,
        _ => return workspace_root.to_string(),
    };

    let root = Path::new(workspace_root);
    let mut dir = Path::new(file_path).parent().unwrap_or(root);

    loop {
        if markers.iter().any(|m| dir.join(m).exists()) {
            return dir.to_string_lossy().to_string();
        }
        if dir == root {
            break;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent,
            _ => break,
        }
    }

    workspace_root.to_string()
}

fn detect_workspace_languages(workspace_path: &str) -> Vec<String> {
    let root = Path::new(workspace_path);
    let mut languages = Vec::new();
    let mut seen = HashSet::new();

    for entry in SERVERS.iter() {
        if entry.markers.is_empty() {
            continue;
        }
        let group = &entry.languages[0];
        if entry.markers.iter().any(|m| root.join(m).exists()) && seen.insert(group) {
            languages.push(group.clone());
        }
    }

    languages
}

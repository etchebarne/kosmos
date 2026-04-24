use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::process::Command;

/// (agent id exposed to the frontend, CLI binary name on PATH).
const AGENTS: &[(&str, &str)] = &[("claude-code", "claude"), ("codex", "codex")];

pub fn is_agent_installed(agent_id: &str) -> bool {
    AGENTS
        .iter()
        .find(|(id, _)| *id == agent_id)
        .is_some_and(|(_, bin)| is_on_path(bin))
}

#[tauri::command]
pub fn ai_installed_agents() -> Vec<String> {
    AGENTS
        .iter()
        .filter(|(_, bin)| is_on_path(bin))
        .map(|(id, _)| (*id).to_string())
        .collect()
}

/// Returns true if `bin` is findable on PATH. Simple scan — no subprocess spawn.
/// On macOS, Tauri apps launched from Finder inherit the GUI session's PATH, which
/// typically excludes user-level install dirs; users in that case can launch Kosmos
/// from a terminal to pick up their shell PATH.
fn is_on_path(bin: &str) -> bool {
    let Some(path_os) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_os) {
        if dir.join(bin).is_file() {
            return true;
        }
        #[cfg(windows)]
        for ext in ["exe", "cmd", "bat"] {
            if dir.join(format!("{bin}.{ext}")).is_file() {
                return true;
            }
        }
    }
    false
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerateResult {
    /// Code the agent wrote to the sideband temp file.
    pub text: String,
    /// stderr captured from the subprocess — handy for diagnostics even on success.
    pub stderr: String,
    /// Raw stdout from the subprocess — likely chat-style prose; kept for debugging only.
    pub raw: String,
    /// Path that was used as TEMP_FILE. Useful when `text` is empty so users can poke at it.
    pub temp_path: String,
}

#[tauri::command]
pub async fn ai_generate(
    prompt: String,
    agent: String,
    model: Option<String>,
    cwd: Option<String>,
) -> Result<AiGenerateResult, String> {
    match agent.as_str() {
        "claude-code" => {
            let model = model.as_deref().unwrap_or("sonnet");
            run_claude_code(&prompt, model, cwd.as_deref()).await
        }
        "codex" => Err("Codex agent is not yet supported".into()),
        other => Err(format!("Unknown agent: {other}")),
    }
}

/// Pick a fresh path in the OS temp dir. No crate dep — pid + nanos is plenty unique for
/// per-generation scratch files.
fn make_temp_path() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let name = format!("kosmos-ai-{pid}-{nanos}.txt");
    std::env::temp_dir().join(name)
}

fn build_wrapped_prompt(user_prompt: &str, temp_path: &str) -> String {
    format!(
        "<MustObey>
- You have been given a sideband output file at TEMP_FILE (shown below).
- ONLY deliver the requested code by writing it to TEMP_FILE (overwrite any prior contents).
- NEVER modify any file other than TEMP_FILE. Do not edit the user's source files.
- NEVER read TEMP_FILE — it is for your output only, previous contents are meaningless.
- Write ONLY raw code to TEMP_FILE — no markdown fences, no commentary, no explanations.
- After writing TEMP_FILE once, you are done. End the session immediately.
- Your stdout / chat response is ignored by the caller. The ONLY signal is the contents of TEMP_FILE.
</MustObey>

<TEMP_FILE>{temp_path}</TEMP_FILE>

{user_prompt}
"
    )
}

async fn run_claude_code(
    user_prompt: &str,
    model: &str,
    cwd: Option<&str>,
) -> Result<AiGenerateResult, String> {
    let temp_path = make_temp_path();
    // Pre-create the file so we can tell "agent wrote nothing" from "file missing".
    tokio::fs::write(&temp_path, b"")
        .await
        .map_err(|e| format!("Failed to create temp file {}: {e}", temp_path.display()))?;

    let wrapped = build_wrapped_prompt(user_prompt, &temp_path.to_string_lossy());

    let mut cmd = Command::new("claude");
    cmd.arg("--print")
        .arg(&wrapped)
        .arg("--dangerously-skip-permissions")
        .arg("--model")
        .arg(model)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = cwd {
        cmd.current_dir(path);
    }
    let child = cmd.spawn().map_err(|e| {
        format!("Failed to spawn `claude`: {e}. Is Claude Code installed and on PATH?")
    })?;

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Failed to wait for claude: {e}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(format!(
            "claude exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let text = tokio::fs::read_to_string(&temp_path)
        .await
        .map_err(|e| format!("Failed to read temp file {}: {e}", temp_path.display()))?;

    let temp_path_str = temp_path.to_string_lossy().into_owned();
    let _ = tokio::fs::remove_file(&temp_path).await;

    Ok(AiGenerateResult {
        text,
        stderr,
        raw,
        temp_path: temp_path_str,
    })
}

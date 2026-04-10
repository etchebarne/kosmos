use std::collections::HashMap;
use std::path::Path;

use kosmos_protocol::types::*;

use crate::CoreError;

#[cfg(target_os = "windows")]
use crate::CREATE_NO_WINDOW;

fn git_command(path: &Path, args: &[&str]) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args)
        .current_dir(path)
        .env("GIT_OPTIONAL_LOCKS", "0");
    #[cfg(target_os = "linux")]
    crate::sanitize_child_env(&mut cmd);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[tracing::instrument(level = "debug")]
pub(crate) async fn run_git(path: &Path, args: &[&str]) -> Result<Option<String>, CoreError> {
    let output = git_command(path, args).output().await?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

#[tracing::instrument(level = "debug")]
pub(crate) async fn run_git_strict(path: &Path, args: &[&str]) -> Result<(), CoreError> {
    let output = git_command(path, args).output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string();
        return Err(CoreError::Git { message: stderr });
    }
    Ok(())
}

fn parse_numstat(output: &str) -> HashMap<String, (i32, i32)> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let additions = parts[0].parse::<i32>().unwrap_or(0);
            let deletions = parts[1].parse::<i32>().unwrap_or(0);
            let path = parts[2..].join("\t");
            map.insert(path, (additions, deletions));
        }
    }
    map
}

pub async fn get_git_branch(path: &str) -> Result<Option<String>, CoreError> {
    let dir = Path::new(path);
    if !dir.exists() {
        return Ok(None);
    }
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await
}

pub async fn get_git_status(path: &str) -> Result<GitStatusInfo, CoreError> {
    let dir = Path::new(path);
    if !dir.exists() {
        return Err(CoreError::NotFound("Directory does not exist".to_string()));
    }

    let is_repo =
        run_git(dir, &["rev-parse", "--is-inside-work-tree"]).await?.is_some_and(|s| s.trim() == "true");

    if !is_repo {
        return Ok(GitStatusInfo {
            changes: Vec::new(),
            branch: None,
            remote_branch: None,
            last_commit_message: None,
            has_remote: false,
            is_repo: false,
            ahead: 0,
            behind: 0,
        });
    }

    // All queries below are read-only and run with GIT_OPTIONAL_LOCKS=0,
    // so they can safely execute concurrently.
    let (
        branch,
        remote_branch,
        last_commit_message,
        remote_output,
        counts,
        status_output,
        staged_raw,
        unstaged_raw,
    ) = tokio::join!(
        run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]),
        run_git(dir, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
        run_git(dir, &["log", "-1", "--pretty=%s"]),
        run_git(dir, &["remote"]),
        run_git(dir, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]),
        run_git(dir, &["status", "--porcelain", "-unormal"]),
        run_git(dir, &["diff", "--cached", "--numstat"]),
        run_git(dir, &["diff", "--numstat"]),
    );

    let branch = branch?;
    let remote_branch = remote_branch?;
    let last_commit_message = last_commit_message?;
    let has_remote = remote_output?.is_some_and(|s| !s.trim().is_empty());
    let status_output = status_output?;

    let (ahead, behind) = if remote_branch.is_some() {
        counts?
            .and_then(|s| {
                let parts: Vec<&str> = s.split('\t').collect();
                if parts.len() == 2 {
                    Some((
                        parts[0].trim().parse::<u32>().unwrap_or(0),
                        parts[1].trim().parse::<u32>().unwrap_or(0),
                    ))
                } else {
                    None
                }
            })
            .unwrap_or((0, 0))
    } else {
        (0, 0)
    };

    let staged_stats = staged_raw?
        .map(|s| parse_numstat(&s))
        .unwrap_or_default();

    let unstaged_stats = unstaged_raw?
        .map(|s| parse_numstat(&s))
        .unwrap_or_default();

    let mut changes = Vec::new();

    if let Some(status) = status_output {
        for line in status.lines() {
            if line.len() < 4 {
                continue;
            }

            let bytes = line.as_bytes();
            let x = bytes[0] as char;
            let y = bytes[1] as char;
            let file_path = &line[3..];

            let file_path = file_path.trim_end_matches('/');
            if file_path.is_empty() {
                continue;
            }

            let file_path = if file_path.contains(" -> ") {
                file_path.split(" -> ").last().unwrap_or(file_path)
            } else {
                file_path
            };

            let staged = x != ' ' && x != '?';

            let status_str = match (x, y) {
                ('?', '?') => "untracked",
                ('A', _) => "added",
                (_, 'D') if x == ' ' => "deleted",
                ('D', _) => "deleted",
                ('R', _) => "renamed",
                _ => "modified",
            };

            let (additions, deletions) = if x == '?' && y == '?' {
                let full_path = dir.join(file_path);
                // Cap line-counting to 1 MB to avoid reading huge untracked files
                let count = std::fs::metadata(&full_path)
                    .ok()
                    .filter(|m| m.len() <= 1_024 * 1_024)
                    .and_then(|_| std::fs::read_to_string(&full_path).ok())
                    .map(|s| s.lines().count() as i32)
                    .unwrap_or(0);
                (count, 0)
            } else if staged {
                staged_stats.get(file_path).copied().unwrap_or((0, 0))
            } else {
                unstaged_stats.get(file_path).copied().unwrap_or((0, 0))
            };

            changes.push(GitFileChange {
                path: file_path.to_string(),
                status: status_str.to_string(),
                staged,
                additions,
                deletions,
            });
        }
    }

    changes.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(GitStatusInfo {
        changes,
        branch,
        remote_branch,
        last_commit_message,
        has_remote,
        is_repo: true,
        ahead,
        behind,
    })
}

pub async fn git_stage(path: &str, files: Vec<String>) -> Result<(), CoreError> {
    let dir = Path::new(path);
    let mut args: Vec<&str> = vec!["add", "--"];
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend(refs);
    run_git_strict(dir, &args).await
}

pub async fn git_unstage(path: &str, files: Vec<String>) -> Result<(), CoreError> {
    let dir = Path::new(path);
    let mut args: Vec<&str> = vec!["reset", "HEAD", "--"];
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend(refs);
    run_git_strict(dir, &args).await
}

pub async fn git_stage_all(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["add", "-A"]).await
}

pub async fn git_commit(path: &str, message: &str) -> Result<(), CoreError> {
    if message.trim().is_empty() {
        return Err(CoreError::Git {
            message: "Commit message cannot be empty".to_string(),
        });
    }
    let dir = Path::new(path);
    run_git_strict(dir, &["commit", "-m", message]).await
}

pub async fn git_list_branches(path: &str) -> Result<Vec<GitBranchInfo>, CoreError> {
    let dir = Path::new(path);
    if !dir.exists() {
        return Err(CoreError::NotFound("Directory does not exist".to_string()));
    }

    let output = run_git(
        dir,
        &[
            "branch",
            "-a",
            "--sort=-committerdate",
            "--format=%(HEAD)|%(refname:short)|%(committerdate:relative)",
        ],
    ).await?;

    let mut branches = Vec::new();
    if let Some(output) = output {
        for line in output.lines() {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 3 {
                continue;
            }
            let is_current = parts[0].trim() == "*";
            let name = parts[1].trim().to_string();
            let date = parts[2].trim().to_string();

            if name.contains("->") || name == "HEAD" {
                continue;
            }

            let is_remote = name.starts_with("origin/");

            branches.push(GitBranchInfo {
                name,
                is_remote,
                is_current,
                last_commit_date: if date.is_empty() { None } else { Some(date) },
            });
        }
    }

    Ok(branches)
}

pub async fn git_checkout(path: &str, branch: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["checkout", branch]).await
}

pub async fn git_delete_branch(path: &str, branch: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["branch", "-D", branch]).await
}

pub async fn git_discard(path: &str, files: Vec<String>) -> Result<(), CoreError> {
    let dir = Path::new(path);
    let mut args: Vec<&str> = vec!["checkout", "--"];
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend(refs);
    run_git_strict(dir, &args).await
}

pub fn git_trash_untracked(path: &str, files: Vec<String>) -> Result<(), CoreError> {
    let dir = Path::new(path);
    for file in &files {
        let full_path = dir.join(file);
        if full_path.is_dir() {
            std::fs::remove_dir_all(&full_path)?;
        } else {
            std::fs::remove_file(&full_path)?;
        }
    }
    Ok(())
}

pub async fn git_discard_all_tracked(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["checkout", "--", "."]).await
}

pub async fn git_trash_all_untracked(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["clean", "-fd"]).await
}

pub async fn git_diff(path: &str, file: &str, staged: bool) -> Result<String, CoreError> {
    let dir = Path::new(path);
    let (primary, fallback) = if staged {
        (vec!["diff", "--cached", "--", file], vec!["diff", "--", file])
    } else {
        (vec!["diff", "--", file], vec!["diff", "--cached", "--", file])
    };
    let result = run_git(dir, &primary).await?;
    if result.as_ref().is_some_and(|s| !s.is_empty()) {
        return Ok(result.unwrap());
    }
    let fallback_result = run_git(dir, &fallback).await?;
    Ok(fallback_result.unwrap_or_default())
}

pub async fn git_diff_untracked(path: &str, file: &str) -> Result<String, CoreError> {
    let dir = Path::new(path);
    let full_path = dir.join(file);
    let contents = tokio::fs::read_to_string(&full_path).await?;

    let lines: Vec<&str> = contents.lines().collect();
    let line_count = lines.len();
    let mut diff = format!(
        "diff --git a/{f} b/{f}\nnew file mode 100644\n--- /dev/null\n+++ b/{f}\n@@ -0,0 +1,{line_count} @@\n",
        f = file
    );
    for line in &lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    Ok(diff)
}

pub async fn git_init(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["init", "-b", "main"]).await
}

pub async fn git_fetch(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["fetch"]).await
}

pub async fn git_pull(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["pull"]).await
}

pub async fn git_pull_rebase(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["pull", "--rebase"]).await
}

pub async fn git_push(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["push"]).await
}

pub async fn git_force_push(path: &str) -> Result<(), CoreError> {
    let dir = Path::new(path);
    run_git_strict(dir, &["push", "--force-with-lease"]).await
}

/// Extract the GitHub owner (user or org) from the remote origin URL, if any.
pub async fn get_git_remote_owner(path: &str) -> Result<Option<String>, CoreError> {
    let dir = Path::new(path);
    let url = run_git(dir, &["config", "--get", "remote.origin.url"]).await?;
    Ok(url.and_then(|u| parse_github_owner(&u)))
}

/// Parse a GitHub owner from an SSH or HTTPS remote URL.
fn parse_github_owner(url: &str) -> Option<String> {
    // SSH:   git@github.com:owner/repo.git
    // HTTPS: https://github.com/owner/repo.git
    let after = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else {
        url.split("github.com/").nth(1)?
    };
    let owner = after.split('/').next()?;
    if owner.is_empty() {
        None
    } else {
        Some(owner.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_numstat_normal_input() {
        let input = "10\t5\tsrc/main.rs\n3\t1\tREADME.md";
        let result = parse_numstat(input);
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("src/main.rs"), Some(&(10, 5)));
        assert_eq!(result.get("README.md"), Some(&(3, 1)));
    }

    #[test]
    fn parse_numstat_binary_files() {
        let input = "-\t-\timage.png";
        let result = parse_numstat(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("image.png"), Some(&(0, 0)));
    }

    #[test]
    fn parse_numstat_empty_input() {
        let result = parse_numstat("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_numstat_malformed_lines_skipped() {
        let input = "10\t5\tsrc/main.rs\nmalformed line\n\n3\t1\tREADME.md\nonly_one_part";
        let result = parse_numstat(input);
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("src/main.rs"), Some(&(10, 5)));
        assert_eq!(result.get("README.md"), Some(&(3, 1)));
    }

    #[test]
    fn parse_numstat_path_with_tabs() {
        // If a path contains a tab, parts[2..] should be rejoined
        let input = "4\t2\tpath\twith\ttab";
        let result = parse_numstat(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("path\twith\ttab"), Some(&(4, 2)));
    }
}

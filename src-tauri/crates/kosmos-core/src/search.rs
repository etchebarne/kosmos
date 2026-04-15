use std::io::{BufRead, BufReader};

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

struct RootPrefix {
    forward: String,
    back: String,
}

impl RootPrefix {
    fn new(root: &str) -> Self {
        let forward = if root.ends_with('/') || root.ends_with('\\') {
            root.replace('\\', "/")
        } else {
            format!("{}/", root.replace('\\', "/"))
        };
        let back = forward.replace('/', "\\");
        Self { forward, back }
    }

    fn make_relative(&self, full: &str) -> String {
        let stripped = if full.starts_with(&self.forward) {
            &full[self.forward.len()..]
        } else if full.starts_with(&self.back) {
            &full[self.back.len()..]
        } else {
            full
        };
        stripped.replace('\\', "/")
    }
}

fn build_walker(root: &str) -> ignore::Walk {
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .max_depth(Some(20))
        .build()
}

/// Walk the workspace and return all file paths (respects .gitignore).
pub fn list_workspace_files(path: &str) -> Result<Vec<String>, String> {
    let prefix = RootPrefix::new(path);
    let mut files = Vec::new();

    for entry in build_walker(path).flatten() {
        if entry.file_type().map_or(true, |ft| !ft.is_file()) {
            continue;
        }
        let full = entry.path().to_string_lossy();
        files.push(prefix.make_relative(&full));
    }
    Ok(files)
}

// ── Fuzzy file search ──

#[derive(Serialize, Deserialize, Clone)]
pub struct FuzzyFileMatch {
    pub path: String,
    pub score: i64,
    pub indices: Vec<usize>,
}

/// Fuzzy-search workspace files, returning scored + sorted results with match
/// indices for highlight rendering. Matching and scoring run entirely in Rust.
pub fn fuzzy_search_files(
    path: &str,
    query: &str,
    max_results: Option<usize>,
) -> Result<Vec<FuzzyFileMatch>, String> {
    let max = max_results.unwrap_or(50);
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let q_chars: Vec<char> = trimmed.chars().collect();
    let case_insensitive = q_chars.iter().all(|c| !c.is_uppercase());

    let prefix = RootPrefix::new(path);
    let mut matches: Vec<FuzzyFileMatch> = Vec::new();

    for entry in build_walker(path).flatten() {
        if entry.file_type().map_or(true, |ft| !ft.is_file()) {
            continue;
        }
        let full = entry.path().to_string_lossy();
        let relative = prefix.make_relative(&full);

        if let Some((score, indices)) = fuzzy_match(&q_chars, &relative, case_insensitive) {
            matches.push(FuzzyFileMatch {
                path: relative,
                score,
                indices,
            });
        }
    }

    matches.sort_by(|a, b| b.score.cmp(&a.score));
    matches.truncate(max);
    Ok(matches)
}

/// Two-pass fuzzy match: forward scan to verify, then try all starting
/// positions to find the highest-scoring alignment.
fn fuzzy_match(
    query: &[char],
    target: &str,
    case_insensitive: bool,
) -> Option<(i64, Vec<usize>)> {
    let t_chars: Vec<char> = target.chars().collect();
    let n = query.len();
    let m = t_chars.len();

    if n == 0 {
        return Some((0, vec![]));
    }
    if n > m {
        return None;
    }

    // Quick check: does target contain all query chars at all?
    let mut qi = 0;
    for &tc in &t_chars {
        if char_eq(query[qi], tc, case_insensitive) {
            qi += 1;
            if qi == n {
                break;
            }
        }
    }
    if qi != n {
        return None;
    }

    // Try greedy forward match from every position where query[0] matches.
    // Keep the best-scoring alignment.
    let mut best: Option<(i64, Vec<usize>)> = None;

    for start in 0..m {
        if !char_eq(query[0], t_chars[start], case_insensitive) {
            continue;
        }

        let mut indices = Vec::with_capacity(n);
        indices.push(start);
        let mut qi = 1;
        for j in (start + 1)..m {
            if qi >= n {
                break;
            }
            if char_eq(query[qi], t_chars[j], case_insensitive) {
                indices.push(j);
                qi += 1;
            }
        }
        if qi != n {
            continue;
        }

        let score = score_match(query, &t_chars, &indices, case_insensitive);
        if best.as_ref().map_or(true, |(bs, _)| score > *bs) {
            best = Some((score, indices));
        }
    }

    best
}

/// Score a matched alignment. Higher is better.
fn score_match(
    query: &[char],
    target: &[char],
    indices: &[usize],
    case_insensitive: bool,
) -> i64 {
    let mut score: i64 = 0;

    for (i, &idx) in indices.iter().enumerate() {
        // Consecutive match bonus
        if i > 0 && idx == indices[i - 1] + 1 {
            score += 8;
        }

        // Word-boundary bonus (after separator or at position 0)
        if idx == 0 || is_separator(target[idx - 1]) {
            score += 10;
        }

        // camelCase boundary bonus
        if idx > 0 && target[idx].is_uppercase() && target[idx - 1].is_lowercase() {
            score += 8;
        }

        // Exact case bonus (only meaningful in case-insensitive mode)
        if case_insensitive && target[idx] == query[i] {
            score += 1;
        }
    }

    // Filename bonus: all matches are in the filename portion
    let last_sep = target
        .iter()
        .rposition(|c| *c == '/' || *c == '\\')
        .map(|p| p + 1)
        .unwrap_or(0);
    if indices[0] >= last_sep {
        score += 15;
    }

    // Gap penalty: total span minus matched chars
    if indices.len() > 1 {
        let span = indices[indices.len() - 1] - indices[0] + 1;
        let gaps = span - indices.len();
        score -= gaps as i64;
    }

    // Length penalty: prefer shorter paths
    score -= (target.len() as i64) / 10;

    score
}

#[inline]
fn char_eq(a: char, b: char, case_insensitive: bool) -> bool {
    if case_insensitive {
        a.to_ascii_lowercase() == b.to_ascii_lowercase()
    } else {
        a == b
    }
}

#[inline]
fn is_separator(c: char) -> bool {
    matches!(c, '/' | '\\' | '.' | '-' | '_' | ' ')
}

// ── Content search ──

#[derive(Serialize, Deserialize, Clone)]
pub struct ContentMatch {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub text: String,
}

/// Search file contents for a query string.
///
/// When `use_regex` is true the query is compiled as a regex pattern;
/// otherwise a case-insensitive literal search is performed.
pub fn search_in_files(
    path: &str,
    query: &str,
    max_results: Option<usize>,
    use_regex: Option<bool>,
) -> Result<Vec<ContentMatch>, String> {
    let max = max_results.unwrap_or(100);
    let prefix = RootPrefix::new(path);

    let is_regex = use_regex.unwrap_or(false);
    let searcher: Searcher = if is_regex {
        let re = regex::Regex::new(query).map_err(|e| format!("Invalid regex: {e}"))?;
        Searcher::Regex(re)
    } else {
        Searcher::Literal(query.to_lowercase())
    };

    let mut results = Vec::new();

    for entry in build_walker(path).flatten() {
        if results.len() >= max {
            break;
        }
        if entry.file_type().map_or(true, |ft| !ft.is_file()) {
            continue;
        }

        let file_path = entry.path();
        let file = match std::fs::File::open(file_path) {
            Ok(f) => f,
            Err(_) => continue,
        };

        // Skip binary files by checking first 8 KiB for null bytes
        let mut reader = BufReader::new(file);
        {
            let buf = reader.fill_buf().unwrap_or(&[]);
            let check_len = buf.len().min(8192);
            if buf[..check_len].contains(&0) {
                continue;
            }
        }

        let relative = prefix.make_relative(&file_path.to_string_lossy());

        for (line_num, line_result) in reader.lines().enumerate() {
            if results.len() >= max {
                break;
            }
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let col_pos = match &searcher {
                Searcher::Literal(q) => line.to_lowercase().find(q),
                Searcher::Regex(re) => re.find(&line).map(|m| m.start()),
            };

            if let Some(col) = col_pos {
                results.push(ContentMatch {
                    path: relative.clone(),
                    line: (line_num + 1) as u32,
                    col: (col + 1) as u32,
                    text: if line.len() > 300 {
                        format!("{}...", &line[..300])
                    } else {
                        line
                    },
                });
            }
        }
    }

    Ok(results)
}

enum Searcher {
    Literal(String),
    Regex(regex::Regex),
}

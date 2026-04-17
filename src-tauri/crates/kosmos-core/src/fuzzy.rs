//! Generic fuzzy matcher for filtering arbitrary string lists.
//!
//! This is the companion to [`fff_picker`](crate::fff_picker). The picker is
//! the right tool for "all files in a workspace" — it indexes in the
//! background, watches the filesystem, and scores with frecency. But for any
//! other list the caller already holds in memory (git branches, open buffers,
//! command palette actions, LSP symbols), we want a one-shot match that
//! accepts a slice and returns scored hits with highlight indices.
//!
//! Backed by [`nucleo-matcher`](https://docs.rs/nucleo-matcher), which powers
//! the Zed and Helix pickers.

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::{Deserialize, Serialize};

/// Which matcher flavour to use. `Path` biases scoring toward path separators
/// and filename endings, matching how VS Code / Helix score file paths.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    Plain,
    Path,
}

/// A single scored match with highlight indices (byte offsets into `text`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FuzzyHit {
    pub text: String,
    pub score: u32,
    pub indices: Vec<u32>,
}

/// Filter `items` by `query`, returning only matches sorted by score (best first).
///
/// Empty queries return every item with score 0 and no indices, preserving the
/// caller's original order. `limit` caps the returned list.
pub fn fuzzy_match(
    query: &str,
    items: &[String],
    mode: MatchMode,
    limit: Option<usize>,
) -> Vec<FuzzyHit> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return items
            .iter()
            .take(limit.unwrap_or(items.len()))
            .map(|text| FuzzyHit {
                text: text.clone(),
                score: 0,
                indices: Vec::new(),
            })
            .collect();
    }

    let config = match mode {
        MatchMode::Plain => Config::DEFAULT,
        MatchMode::Path => Config::DEFAULT.match_paths(),
    };
    let mut matcher = Matcher::new(config);
    let pattern = Pattern::new(
        trimmed,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );

    let mut hits: Vec<FuzzyHit> = Vec::new();
    for item in items {
        let mut buf = Vec::new();
        let haystack = Utf32Str::new(item, &mut buf);
        let mut indices = Vec::new();
        if let Some(score) = pattern.indices(haystack, &mut matcher, &mut indices) {
            indices.sort_unstable();
            indices.dedup();
            hits.push(FuzzyHit {
                text: item.clone(),
                score,
                indices,
            });
        }
    }

    hits.sort_by(|a, b| b.score.cmp(&a.score));
    if let Some(max) = limit {
        hits.truncate(max);
    }
    hits
}

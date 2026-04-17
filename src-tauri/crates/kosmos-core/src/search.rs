//! Shared content-search types. The actual search engine now lives in
//! [`crate::fff_picker::FffPicker`], which wraps fff-search's mmap-backed
//! grep. This module only defines the wire format so the protocol and the
//! Tauri layer can agree on the result shape without depending on fff.

use serde::{Deserialize, Serialize};

/// A single content match, relative to a workspace root.
#[derive(Serialize, Deserialize, Clone)]
pub struct ContentMatch {
    /// Workspace-relative path, forward-slash normalized.
    pub path: String,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column where the match starts.
    pub col: u32,
    /// The matched line text, already truncated by the engine for display.
    pub text: String,
}

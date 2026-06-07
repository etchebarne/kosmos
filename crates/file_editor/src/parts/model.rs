use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::{App, Context, FocusHandle, Focusable, actions};
use language::LanguageId;
use settings::{ActiveSettings, SettingValue};

actions!(file_editor, [Save]);

/// Stable identifier for an open buffer. Issued by [`BufferStore`] and never
/// reused, so editor subsystems can hold onto an id across path changes,
/// untitled buffers, or multi-root collisions where two paths could otherwise
/// alias.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct BufferId(u64);

impl BufferId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

pub const SOFT_WRAP_SETTING_ID: &str = "editor.soft_wrap";

/// Resolve `editor.soft_wrap` from the global settings, falling back to the
/// default declared in `settings::registry::EDITOR`.
pub fn soft_wrap_enabled(cx: &App) -> bool {
    cx.settings()
        .get(SOFT_WRAP_SETTING_ID)
        .and_then(SettingValue::as_bool)
        .unwrap_or(false)
}

/// In-memory view of a file open in an editor tab. Shared across all tabs
/// viewing the same path; per-tab editor state lives in the UI component store.
pub struct Buffer {
    id: BufferId,
    path: PathBuf,
    language: Option<LanguageId>,
    content: String,
    saved_content: String,
    disk_fingerprint: Option<DiskFingerprint>,
    dirty: bool,
    focus_handle: FocusHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiskFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

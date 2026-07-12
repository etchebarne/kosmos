use std::sync::{Arc, Mutex};

use crate::language_servers::{LanguageServerDiagnostic, StagedWorkspaceEdit};
use crate::tree::WorkspaceId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerDiagnosticsChanged {
    pub workspace_id: WorkspaceId,
    pub path: String,
    pub server_id: String,
    pub generation: u64,
    pub version: i64,
    pub diagnostics: Vec<LanguageServerDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreEvent {
    LanguageServerDiagnosticsChanged(LanguageServerDiagnosticsChanged),
    LanguageServerStatusChanged { server_id: String },
    LanguageServerLogAvailable { server_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceEditApplication {
    pub applied: bool,
    pub failure_reason: Option<String>,
}

pub trait CoreEventSink: Send + Sync + 'static {
    fn emit(&self, event: CoreEvent);

    fn apply_workspace_edit(&self, _edit: StagedWorkspaceEdit) -> WorkspaceEditApplication {
        WorkspaceEditApplication {
            applied: false,
            failure_reason: Some("no workspace edit renderer is connected".to_owned()),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct CoreEventDispatcher {
    sink: Arc<Mutex<Option<Arc<dyn CoreEventSink>>>>,
}

impl std::fmt::Debug for CoreEventDispatcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CoreEventDispatcher")
    }
}

impl CoreEventDispatcher {
    pub(crate) fn set_sink(&self, sink: Arc<dyn CoreEventSink>) {
        *self.sink.lock().unwrap_or_else(|error| error.into_inner()) = Some(sink);
    }

    pub(crate) fn emit(&self, event: CoreEvent) {
        let sink = self
            .sink
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(sink) = sink {
            sink.emit(event);
        }
    }

    pub(crate) fn apply_workspace_edit(
        &self,
        edit: StagedWorkspaceEdit,
    ) -> WorkspaceEditApplication {
        let sink = self
            .sink
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        sink.map_or_else(
            || WorkspaceEditApplication {
                applied: false,
                failure_reason: Some("no workspace edit renderer is connected".to_owned()),
            },
            |sink| sink.apply_workspace_edit(edit),
        )
    }
}

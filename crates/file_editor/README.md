# file_editor

Role: editor buffer and editor-view state.

Owns:
- File buffer loading, saving, dirty tracking, and buffer identity.
- Text edit application, undo/redo state, selections, and cursor movement state.
- Per-tab editor view state such as scroll, folds, hover status, and cached layout data.
- Editor-specific actions such as save.

Does Not Own:
- Pane or tab layout.
- File-editor tab rendering.
- Syntax parsing, theme styling, or LSP transport.
- Workspace/session persistence.

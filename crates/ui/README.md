# ui

Role: GPUI rendering and UI interaction boundary.

Owns:
- Shared GPUI components.
- Application layout rendering.
- Tab body rendering.
- UI delegate traits implemented by the application shell.
- Drag payloads and drop-zone UI behavior.
- GPUI action wiring that belongs at the UI boundary.
- Tab icon resolution for presentation.

Does Not Own:
- Application state orchestration.
- Domain mutation rules for panes, workspaces, file trees, or editors.
- SQLite persistence.
- External tool installation.

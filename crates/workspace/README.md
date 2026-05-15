# workspace

Role: workspace collection model.

Owns:
- Workspace identity, path, display name, and pane tree.
- Workspace manager state.
- Active and previous-active workspace tracking.
- Workspace add, close, select, and reorder behavior.

Does Not Own:
- Workspace UI rendering.
- File tree root synchronization.
- Persistence.
- File-system watching.

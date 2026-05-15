# persistence

Role: local session persistence.

Owns:
- SQLite connection management for session data.
- Schema migrations for workspace, pane tree, tabs, and window bounds.
- Loading and saving workspace manager state.
- Loading and saving window bounds.

Does Not Own:
- Runtime mutation policy.
- Settings persistence.
- UI state that should remain transient.

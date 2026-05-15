# tabs

Role: pure tab metadata and tab kind registry.

Owns:
- `Tab` identity, kind, optional title, and optional path.
- `TabKind` metadata for built-in tab types.
- Lookup of tab kinds by id.

Does Not Own:
- Icons.
- Rendering tab headers or tab bodies.
- Pane layout.
- Feature-specific tab behavior.

This crate should remain dependency-light because it sits under pane and persistence models.

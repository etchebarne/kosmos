# panes

Role: single-pane tab collection model.

Owns:
- Pane identity.
- Ordered tabs within one pane.
- Active tab selection inside one pane.
- Tab insertion, replacement, selection, and removal within one pane.

Does Not Own:
- Split tree layout.
- Tab kind semantics.
- Rendering or drag-and-drop UI.
- Persistence.

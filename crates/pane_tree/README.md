# pane_tree

Role: pane split tree and tab movement model.

Owns:
- Tree of panes and split nodes.
- Pane, split, and tab id allocation.
- Active pane tracking.
- Tab movement between panes.
- Pane splitting, empty-pane collapse, and split resize ratios.

Does Not Own:
- GPUI action wiring.
- File-editor-specific open behavior.
- Rendering.
- Persistence format.

# file_tree

Role: file tree state and file-system operations.

Owns:
- Active file tree entity state.
- Directory loading, expansion, selection, clipboard, rename, and new-entry state.
- File-system mutation helpers for create, rename, move, paste, delete, and trash.
- Recursive file-system watching and refresh events.

Does Not Own:
- File tree rendering.
- Workspace selection or root discovery policy.
- Opening files into panes.
- Editor buffer contents.

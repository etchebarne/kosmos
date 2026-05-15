# app_shell

Role: application composition layer.

Owns:
- GPUI application startup after the binary hands off control.
- Global state installation and feature-store bootstrapping.
- Main window creation and window persistence hooks.
- `KosmosApp`, delegate implementations, and cross-feature orchestration.
- Runtime coordination between workspace, pane, file tree, editor, settings, and persistence crates.

Does Not Own:
- Domain models for panes, tabs, workspaces, settings, or files.
- Reusable GPUI components or tab rendering.
- Low-level persistence schemas or file-system operations.

Use this crate when a feature needs to connect multiple crates together.

# kosmos

Role: binary entry point.

Owns:
- `main()`.
- Process-level bootstrap before handing control to `app_shell`.
- Future CLI argument parsing, logging setup, panic hooks, or alternate run modes.

Does Not Own:
- Application state.
- Window setup.
- Feature orchestration.
- UI rendering or domain models.

This crate should stay intentionally small.

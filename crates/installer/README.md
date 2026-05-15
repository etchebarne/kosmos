# installer

Role: tool installation and discovery.

Owns:
- Installing registry-defined tools into the Kosmos tool cache.
- Platform target detection.
- Package-manager and GitHub release installation flows.
- Installed binary path resolution.

Does Not Own:
- Tool registry metadata.
- Settings UI for install buttons.
- LSP, formatter, or linter protocol behavior.

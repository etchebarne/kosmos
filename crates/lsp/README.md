# lsp

Role: Language Server Protocol client runtime.

Owns:
- LSP process startup for installed registry entries.
- Per-server, per-root client sessions.
- JSON-RPC request/response plumbing.
- Text document synchronization needed for hover requests.
- Hover request and response models.

Does Not Own:
- Installing language servers.
- Registry definitions for language servers.
- Editor UI for hovers.
- Syntax highlighting.

# lsp

Role: Language Server Protocol client runtime.

Owns:
- LSP process startup for installed registry entries.
- Per-server, per-root client sessions.
- JSON-RPC request/response plumbing.
- Text document synchronization needed for hover and completion requests.
- Hover and completion request/response models.

Does Not Own:
- Installing language servers.
- Registry definitions for language servers.
- Editor UI for hovers.
- Syntax highlighting.

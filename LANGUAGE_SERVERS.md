# Language Server Architecture

This document captures the proposed architecture and decisions for adding Language Server Protocol support to Kosmos.

## Implementation Status

The managed language-tooling architecture is implemented across `core/`, `server/`, and `desktop/`:

- Reviewed, pinned language-server and formatter catalogs with atomic, integrity-verified installation.
- Multi-server document bindings, negotiated synchronization, save notifications, diagnostics, completion and resolve, hover, signature help, colors, navigation, references, and symbols.
- Standalone Prettier, Ruff, and shfmt formatting with persisted priority and LSP fallback.
- Cancellation propagated from Monaco through IPC to LSP `$/cancelRequest`.
- Supervised server restart with bounded backoff, document replay, bounded logs, and typed push notifications.
- Dynamic capability registration, workspace configuration/folders, work-done progress, and contained watched-file notifications.
- Core-validated workspace-edit transactions used by rename, code actions, execute-command, and server-initiated `workspace/applyEdit`.
- Ordered `CreateFile`, `RenameFile`, and `DeleteFile` resource operations with durable crash recovery and editor-tab reconciliation.
- Feature-specific Monaco service suppression while healthy external providers are active.

Current intentional limits:

- Destructive directory transactions above the bounded safety limits are rejected rather than processed without a complete rollback snapshot.
- Managed tools are cataloged for supported Linux architectures; arbitrary remote plugins are not accepted.
- Formatter and language-server selection is deterministic and user-controlled; installation never happens when opening a file.

## Decisions

- The initial catalog will support as many practical languages as possible, including a broad language set and web languages.
- Language server installation will only be initiated from Settings. Opening a file must not prompt or automatically install anything.
- Managed server versions will be pinned to Kosmos's built-in catalog. Users explicitly choose when to update.
- Full core language features are the release target, delivered as incremental vertical slices.
- The initial registry will be a compiled, reviewed built-in catalog rather than a remote plugin marketplace.

## Architecture

Three concepts must remain separate:

| Concept | Responsibility |
| --- | --- |
| Catalog | Which language servers Kosmos supports and how they behave |
| Installation store | Which versions are installed and where |
| Session registry | Which servers are currently running for each workspace and project |

```text
Monaco UI
  -> editor and feature requests over IPC
server/
  -> transport, routing, push notifications
core/language_servers/
  -> catalog, installation, LSP protocol, sessions, document sync
managed upstream artifacts
  -> XDG data directory
```

All LSP logic belongs in `core/`. The server should only expose routes and notifications. Desktop should only adapt Monaco to those routes.

This follows the existing terminal process pattern in `core/src/tabs/terminal.rs`, but language servers need piped stdio, JSON-RPC framing, cancellation, and capability negotiation rather than a PTY.

## Catalog

Start with a compiled, built-in catalog. Do not start with a remotely executable plugin registry.

A definition would roughly contain:

```rust
struct LanguageServerDefinition {
    id: LanguageServerId,
    name: &'static str,
    languages: &'static [LanguageId],
    root_markers: &'static [&'static str],
    launch: LaunchDefinition,
    installation: InstallationDefinition,
    adapter: LanguageServerAdapter,
}
```

The catalog needs:

- Stable server ID.
- Monaco and LSP language IDs.
- File extensions.
- Project root markers and fallback behavior.
- Exact reviewed server version.
- Platform and architecture artifacts.
- SHA-256 checksums.
- Archive format and executable location.
- Runtime dependencies such as Node or Java.
- Default launch arguments and environment.
- Initialization options and configuration.
- Server-specific compatibility hooks.

Most configuration can be declarative. Keep a small adapter hook because servers have unavoidable quirks. Do not build a complex manifest DSL to represent every possible behavior.

A future remote registry can distribute signed, schema-only catalog updates. It should never supply arbitrary shell commands or executable adapter code.

## Server Sources

Kosmos should fetch directly from reviewed upstream sources:

- GitHub release assets for native standalone binaries.
- Exact npm packages for Node-based servers.
- Official distribution archives for servers such as JDTLS.
- Additional ecosystem backends only when they can be made deterministic.

Resolution should be:

1. Explicit user executable override.
2. User-selected Kosmos-managed version.
3. Compatible executable discovered on `PATH`.
4. Unavailable.

Because the target is broad language support, installer backends should be extensible:

```text
PortableArchive
NpmPackage
JavaArchive
PythonEnvironment
ToolchainInstall
```

`PortableArchive` and `NpmPackage` should come first. Avoid a generic "run this installation command" backend. It would make the built-in registry an arbitrary command execution mechanism.

Node-based servers raise an important requirement: either Kosmos manages a pinned Node runtime or clearly marks Node as an external runtime dependency. For genuinely managed installs, Kosmos should manage Node too.

## Installation Store

Use:

```text
$XDG_DATA_HOME/kosmos/language-servers/
  runtimes/
  servers/
    rust-analyzer/
      2026-xx-xx/
    typescript-language-server/
      x.y.z/

$XDG_CACHE_HOME/kosmos/language-server-downloads/
```

Each installation should:

- Download into a temporary location.
- Verify size and checksum.
- Reject archive path traversal.
- Extract into a versioned temporary directory.
- Validate the expected executable and version.
- Atomically rename into its final location.
- Write an installation manifest containing provenance.
- Never overwrite the currently working version.
- Allow rollback and deletion of unused versions.

Versions remain pinned to the built-in catalog. A newer Kosmos catalog can show an update in Settings, but switching versions requires user action.

Installation and updates should only be initiated from Settings. Opening a file can report "server unavailable," but must not prompt or download anything.

## Runtime Model

Sessions should be keyed by:

```text
(workspace_id, server_id, project_root)
```

Sessions must not be keyed by editor tab. One language server generally serves multiple files and possibly multiple languages.

Lifecycle:

1. A relevant Monaco document opens.
2. Desktop sends `didOpen` with its unsaved text and model version.
3. Core locates the project root using server-specific root markers.
4. Core resolves the selected installed executable.
5. The session manager starts or reuses the matching process.
6. Core sends `initialize`, `initialized`, configuration, then `didOpen`.
7. Changes and feature requests enter one ordered per-session queue.
8. Closing the workspace performs `shutdown`, `exit`, timeout, then kill if necessary.

Servers should start lazily on the first relevant document. Keep them alive while their workspace remains open to match Kosmos's background-workspace model. An idle timeout can be added later for memory-heavy servers.

## Document Synchronization

This is the most important existing constraint.

Unsaved text currently lives in `desktop/src/renderer/lib/editor-buffers.ts`, while `core/src/tabs/editor.rs` reads disk. Reading files from core is therefore insufficient for LSP.

Desktop must send:

- `didOpen`: full contents, language, and version.
- `didChange`: Monaco incremental changes and the new version.
- `didSave`.
- `didClose`.

Core should mirror currently open documents so it can:

- Guarantee monotonic document versions.
- Reject stale changes.
- Convert positions correctly.
- Recover with a full-document synchronization if changes are dropped.
- Validate and apply returned edits.

Monaco positions are UTF-16-oriented. Kosmos should request UTF-16 during initialization and implement conversion for servers that negotiate another position encoding.

LSP should receive standard `file://` URIs. Monaco can continue using `kosmos://` internally, but URI translation must be centralized in core.

## Concurrency

Do not process LSP requests through the existing shared external worker. A running server would block unrelated filesystem and Git operations.

Each session needs:

- A bounded outgoing command queue.
- A dedicated stdout reader.
- A dedicated stdin writer.
- A stderr ring buffer.
- A pending request map.
- Request timeouts and cancellation.
- Crash detection and bounded restart/backoff.
- Graceful shutdown with forced cleanup fallback.

The `State` can hold a transient `LanguageServerManager` handle, but no IPC handler should wait for an LSP response while holding the global state mutex.

## IPC

The current notification path only understands `workspaceChanged`. It should become a general discriminated notification envelope.

Required events include:

- Diagnostics changed.
- Server status changed.
- Installation progress changed.
- Server crashed.
- Server message or log available.

Diagnostics should be coalesced by `(workspace, path, server)` so a slow renderer cannot overflow the response queue.

Feature requests remain request-response operations:

```text
completion
hover
signatureHelp
definition
references
documentSymbols
workspaceSymbols
formatting
rename
codeActions
executeCommand
```

Cancellation tokens from Monaco should propagate through IPC to LSP `$/cancelRequest`.

## Edit-Producing Features

Rename, formatting, and code actions are harder than hover and diagnostics because edits can affect both open unsaved buffers and closed files.

Core should normalize and validate every `WorkspaceEdit`:

- Reject unsupported URI schemes.
- Prevent writes outside the workspace.
- Validate model versions for open documents.
- Validate file hashes for closed documents.
- Convert edits into a Kosmos workspace edit transaction.

Desktop applies edits to open Monaco models. Core applies edits to closed files. Initially advertise conservative LSP `failureHandling` capabilities rather than claiming fully transactional cross-process edits.

Resource operations such as create, rename, and delete should be a separate later slice from ordinary text edits.

## Broad Initial Catalog

A reasonable broad target is:

| Area | Servers |
| --- | --- |
| Rust | rust-analyzer |
| JavaScript and TypeScript | typescript-language-server |
| Web | vscode-html, vscode-css, vscode-json |
| Linting | ESLint language server |
| Tailwind | tailwindcss-language-server |
| Python | basedpyright or pyright |
| Go | gopls |
| C and C++ | clangd |
| Java | Eclipse JDTLS |
| Lua | lua-language-server |
| Shell | bash-language-server |
| YAML | yaml-language-server |

Catalog breadth should not mean enabling everything simultaneously. Kosmos needs one selected primary server per language initially, with capability-based multiple-server composition deferred.

Monaco's built-in TypeScript, JSON, CSS, and HTML services must be disabled or reduced whenever an external server is active, otherwise users will receive duplicate diagnostics and completions.

## Suggested Layout

```text
core/src/language_servers/
  mod.rs
  catalog.rs
  definitions.rs
  roots.rs
  documents.rs
  edits.rs
  installation/
    mod.rs
    store.rs
    download.rs
    archive.rs
    npm.rs
    runtime.rs
  protocol/
    mod.rs
    framing.rs
    messages.rs
    positions.rs
  runtime/
    mod.rs
    manager.rs
    session.rs
    process.rs
    pending_requests.rs

server/src/ipc/
  messages/language_servers.rs
  router/language_servers.rs

desktop/src/renderer/
  ipc/language-servers.ts
  lib/language-client.ts
  stores/language-server-store.ts
  components/internal/language-server-settings.tsx
```

Keep this as a module inside `core/` rather than introducing another crate immediately.

## Delivery Order

Even with full features as the target, implementation should proceed in these slices:

1. Built-in catalog, installation store, Settings UI, and server status.
2. Process supervision, LSP framing, initialization, and document synchronization.
3. Diagnostics and generic server notifications.
4. Completion, hover, signature help, navigation, references, and symbols.
5. Formatting, rename, code actions, and validated workspace edits.
6. Broad adapter catalog, updates, crash recovery, logs, and compatibility testing.

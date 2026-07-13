# Kosmos Desktop

Electron desktop frontend for Kosmos, managed with Bun, React, Vite, Tailwind CSS, and shadcn/ui with Base UI primitives.

## Requirements

- Bun
- The Rust `kosmos-server` binary from the repository workspace

## Install

```bash
bun install
```

## Development

From `desktop/`:

```bash
bun run build
bun run start
```

From the repository root, `./scripts/run.sh` builds the Rust server, starts it, builds the Electron frontend, and launches Electron. Unpackaged builds use a separate `Kosmos Development` instance lock, user-data directory, and state database, so an installed Kosmos instance can remain open.

## Structure

- `src/main/` owns the Electron main process and Unix socket connection to `server/`.
- `src/preload/` exposes a small, safe renderer API through Electron IPC.
- `src/renderer/` contains the React renderer.
- `src/renderer/ipc/` contains domain functions that renderer code imports to talk to the Rust server.
- `src/components/ui/` contains shadcn/ui components generated for Base UI.
- `src/shared/ipc/generated/` contains schema-derived IPC declarations and runtime validators; `src/shared/ipc/index.ts` is the small Electron-facing facade.

The renderer never talks to the Rust server directly. Renderer consumers should import functions from `src/renderer/ipc/`, which call Electron IPC through the preload API. The main process forwards those calls to the existing newline-delimited JSON protocol on the server Unix socket.

Core owns application policy and state transitions. The server translates IPC and schedules core commands. Desktop renders the UI and adapts Electron and UI libraries; it does not own application policy.

Example:

```ts
import { listWorkspaces, openWorkspace } from "@/renderer/ipc/workspace";

const workspaces = await listWorkspaces();
await openWorkspace("/path/to/project");
```

The desktop uses `$KOSMOS_SOCKET` when set. Otherwise, it creates a per-process socket under `$XDG_RUNTIME_DIR/kosmos/` or the system temporary directory.

## Scripts

```bash
bun run build
bun run dev
bun run start
bun run generate:ipc
bun run check:ipc
bun run typecheck
```

## Verification

Run the complete verification sequence from the repository root:

```bash
bash scripts/check-boundaries.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bun run --cwd desktop check:ipc
bun run --cwd desktop typecheck
bun run --cwd desktop test
bun run --cwd desktop build
```

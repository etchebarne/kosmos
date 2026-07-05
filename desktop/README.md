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

From the repository root, `./scripts/run.sh` builds the Rust server, starts it, builds the Electron frontend, and launches Electron.

## Structure

- `src/main/` owns the Electron main process and Unix socket connection to `server/`.
- `src/preload/` exposes a small, safe renderer API through Electron IPC.
- `src/renderer/` contains the React renderer.
- `src/renderer/ipc/` contains domain functions that renderer code imports to talk to the Rust server.
- `src/components/ui/` contains shadcn/ui components generated for Base UI.
- `src/shared/ipc/` contains protocol envelope and domain types shared across Electron processes.

The renderer never talks to the Rust server directly. Renderer consumers should import functions from `src/renderer/ipc/`, which call Electron IPC through the preload API. The main process forwards those calls to the existing newline-delimited JSON protocol on the server Unix socket.

Example:

```ts
import { listWorkspaces, openWorkspace } from "@/renderer/ipc/workspace";

const workspaces = await listWorkspaces();
await openWorkspace("/path/to/project");
```

The socket path matches the Rust server lookup order: `$KOSMOS_SOCKET`, then `$XDG_RUNTIME_DIR/kosmos/server.sock`, then `/tmp/kosmos/server.sock`.

## Scripts

```bash
bun run build
bun run dev
bun run start
bun run typecheck
```

<div align="center">
  <img width="1200" height="630" alt="Kosmos" src="https://github.com/user-attachments/assets/424b1052-850b-4aff-abca-63ad3c2cedd3" />
  <br>
  <br>
  <p>A code editor where every view is a tab you can place anywhere.</p>

[![Release](https://img.shields.io/github/v/release/etchebarne/kosmos?style=flat-square&color=6366f1)](https://github.com/etchebarne/kosmos/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-6366f1?style=flat-square)](LICENSE)
[![Stars](https://img.shields.io/github/stars/etchebarne/kosmos?style=flat-square&color=6366f1)](https://github.com/etchebarne/kosmos/stargazers)
[![Issues](https://img.shields.io/github/issues/etchebarne/kosmos?style=flat-square&color=6366f1)](https://github.com/etchebarne/kosmos/issues)

</div>

## Why Kosmos

Most editors dictate where things go. Kosmos lets you treat every view as a tab you can place anywhere, split in any direction, and rearrange freely. Multiple workspaces stay alive in the background so you can context-switch without losing terminals, layouts, or in-progress work.

## Platform support

Kosmos only runs on Linux. Windows and macOS are not supported, and support for those platforms is not planned at this time.

## Installation

- Debian, Ubuntu, Linux Mint, and Pop!_OS: install the `.deb` package from the [latest release](https://github.com/etchebarne/kosmos/releases/latest).
- Fedora and openSUSE: install the `.rpm` package from the [latest release](https://github.com/etchebarne/kosmos/releases/latest).
- Arch Linux and derivatives: install [`kosmos-bin`](https://aur.archlinux.org/packages/kosmos-bin) from the AUR, for example with `yay -S kosmos-bin`.
- Other distributions: download the AppImage from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), make it executable, and run it.

## Project layout

- `core/` owns application policy, state transitions, and persistence decisions.
- `server/` translates IPC and schedules core commands; it owns the Unix-socket transport.
- `desktop/` renders the UI and adapts Electron, Monaco, and other UI libraries through IPC.

## Development

- Run the app with `./scripts/run.sh`.
- Install desktop dependencies with `bun install --cwd desktop --frozen-lockfile`.
- Run the full local verification sequence from the repository root:

  ```bash
  bash scripts/check-boundaries.sh
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  bun run --cwd desktop typecheck
  bun run --cwd desktop test
  bun run --cwd desktop build
  ```

- Build production Linux AppImage, deb, and rpm packages with `./scripts/bundle-linux.sh`. Artifacts are written to `desktop/release/`.
- Building the rpm locally requires `rpmbuild`; install it with `sudo pacman -S rpm-tools` on Arch Linux, `sudo apt-get install rpm` on Debian-based systems, or `sudo dnf install rpm-build` on Fedora.
- The AppImage-based AUR package template lives in `aur/kosmos-bin/`.
- Bump release metadata with `./scripts/bump-version.sh patch|minor|major|x.y.z`.

The Electron main process launches the Rust server as a sidecar process. They communicate over a Unix socket, while the renderer communicates with Electron main through Electron IPC only. By default, both use `$XDG_RUNTIME_DIR/kosmos/server.sock`; set `KOSMOS_SOCKET` to override it.

## Language tooling

Kosmos manages reviewed, version-pinned language servers and formatters from Settings. Installed standalone formatters take precedence over language-server formatting according to the configured formatter priority; language-server formatting remains the fallback.

- `Shift+Alt+F` formats the active document.
- `Ctrl+T` searches workspace symbols.
- Language features include diagnostics, completion, hover, signature help, navigation, references, symbols, colors, rename, and code actions.
- Managed formatters include Prettier, Ruff, and shfmt. Prettier installation requires Node.js 22.6 or newer and npm; Ruff and shfmt use verified native artifacts.
- Format on save is available under Editor settings and defaults to off.

Language tools run with the permissions of the opened workspace. Managed installations live under the Kosmos XDG data directory.

## License

MIT - see [LICENSE](LICENSE).

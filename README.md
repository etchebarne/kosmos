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

## Project layout

- `core/` contains the Rust logic layer.
- `server/` contains the Rust backend process that imports `core`.
- `ui/` contains the Qt frontend.
- `assets/` contains shared branding and icon assets.

## Development

- Run the app with `./scripts/run.sh`.
- Test the Rust workspace with `cargo test --workspace`.
- Build the Qt UI with `cmake -S ui -B ui/build` and `cmake --build ui/build`.

The server and UI communicate over a Unix socket. By default, both use `$XDG_RUNTIME_DIR/kosmos/server.sock`; set `KOSMOS_SOCKET` to override it.

## License

MIT - see [LICENSE](LICENSE).

# Kosmos Desktop

GTK desktop frontend for Kosmos, written in C and built with Meson.

## Requirements

- Meson
- Ninja
- pkg-config
- GTK 4 development files
- JSON-GLib development files

On Debian or Ubuntu, install the native dependencies with:

```sh
sudo apt install meson ninja-build pkg-config libgtk-4-dev libjson-glib-dev
```

## Build

From the repository root:

```sh
meson setup desktop/build desktop
meson compile -C desktop/build
```

## Run

```sh
./desktop/build/kosmos-desktop
```

The root `scripts/run.sh` script also builds the Rust server, starts it, and launches this GTK app.

## Structure

- `src/main.c` bootstraps the GTK application.
- `src/app/` owns GTK application lifecycle and initial server synchronization.
- `src/ui/` owns windows and widgets.
- `src/ipc/` owns Unix socket transport, newline-delimited JSON framing, and protocol helpers for `server/`.

The IPC client resolves the socket with the same order as the server: `$KOSMOS_SOCKET`, then `$XDG_RUNTIME_DIR/kosmos/server.sock`, then `/tmp/kosmos/server.sock`.

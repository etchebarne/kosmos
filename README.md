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

## Features

- **Everything is a tab** — file tree, terminal, git, settings. No fixed panels, no locked sidebars.
- **Flexible layouts** — split panes horizontally or vertically. Drag any tab into any pane.
- **Persistent workspaces** — switch between projects instantly. Every workspace keeps its state running in the background.
- **Infinity Tab** — an infinite canvas where you can place any tab — editors, terminals, previews — and arrange them freely in open space.
- **And more to come...**

## Platform support

Kosmos only runs on Linux. Windows and macOS are not supported, and support for those platforms is not planned at this time.

## Installation

**Quick install (any Linux distro)**

```bash
curl -fsSL https://raw.githubusercontent.com/etchebarne/kosmos/main/scripts/install.sh | sh
```

This downloads the latest release tarball, installs it under `~/.local/Kosmos.app/`, symlinks `kosmos` into `~/.local/bin`, and registers a desktop entry so the app shows up in your launcher.

**Manual download**

Grab `kosmos-linux-x86_64.tar.gz` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then:

```bash
KOSMOS_BUNDLE_PATH=./kosmos-linux-x86_64.tar.gz \
    sh <(curl -fsSL https://raw.githubusercontent.com/etchebarne/kosmos/main/scripts/install.sh)
```

**Debian / Ubuntu**

Download `kosmos_<version>_amd64.deb` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then:

```bash
sudo apt install ./kosmos_<version>_amd64.deb
```

**Fedora / RPM distros**

Download `kosmos-<version>-1.x86_64.rpm` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then:

```bash
sudo dnf install ./kosmos-<version>-1.x86_64.rpm
```

**AppImage**

Download `Kosmos-<version>-x86_64.AppImage` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then:

```bash
chmod +x ./Kosmos-<version>-x86_64.AppImage
./Kosmos-<version>-x86_64.AppImage
```

**Arch Linux (AUR)**

```bash
yay -S kosmos-bin       # pre-built binary
yay -S kosmos           # build from source
```

## Uninstall

If you installed via the script:

```bash
curl -fsSL https://raw.githubusercontent.com/etchebarne/kosmos/main/scripts/uninstall.sh | sh
```

If you installed via the AUR:

```bash
sudo pacman -R kosmos-bin   # or: sudo pacman -R kosmos
```

If you ran the AppImage and want to remove its desktop entry and icon:

```bash
./Kosmos-<version>-x86_64.AppImage --uninstall
```

Then delete the AppImage file itself.

## Building from source

**Prerequisites:** [Rust](https://www.rust-lang.org/tools/install) (stable) and the GPUI system dependencies.

<details>
<summary>Arch Linux</summary>

```bash
sudo pacman -S --needed base-devel pkgconf openssl fontconfig \
    libxcb libxkbcommon libxkbcommon-x11 wayland \
    vulkan-icd-loader vulkan-headers
```

</details>

<details>
<summary>Debian / Ubuntu</summary>

```bash
sudo apt install build-essential pkg-config libssl-dev libfontconfig-dev \
    libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
    libwayland-dev libvulkan-dev
```

</details>

<details>
<summary>Fedora</summary>

```bash
sudo dnf install gcc pkgconf openssl-devel fontconfig-devel \
    libxcb-devel libxkbcommon-devel libxkbcommon-x11-devel \
    wayland-devel vulkan-loader-devel
```

</details>

Then:

```bash
cargo run --release
```

## License

MIT — see [LICENSE](LICENSE).

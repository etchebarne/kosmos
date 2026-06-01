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

Choose the install method that matches your Linux distribution or workflow.

### Quick Install

Use the install script for a distro-agnostic setup:

```bash
curl -fsSL https://raw.githubusercontent.com/etchebarne/kosmos/main/scripts/install.sh | sh
```

The script:

- Downloads the latest release tarball
- Installs Kosmos under `~/.local/Kosmos.app/`
- Symlinks `kosmos` into `~/.local/bin`
- Registers a desktop entry so Kosmos appears in your launcher

### Manual Tarball Install

Use this if you already downloaded the release bundle.

Download `kosmos-linux-x86_64.tar.gz` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then run:

```bash
KOSMOS_BUNDLE_PATH=./kosmos-linux-x86_64.tar.gz \
    sh <(curl -fsSL https://raw.githubusercontent.com/etchebarne/kosmos/main/scripts/install.sh)
```

### Debian / Ubuntu

Download `kosmos_<version>_amd64.deb` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then:

```bash
sudo apt install ./kosmos_<version>_amd64.deb
```

### Fedora / RPM Distros

Download `kosmos-<version>-1.x86_64.rpm` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then:

```bash
sudo dnf install ./kosmos-<version>-1.x86_64.rpm
```

### AppImage

Download `Kosmos-<version>-x86_64.AppImage` from the [latest release](https://github.com/etchebarne/kosmos/releases/latest), then:

```bash
chmod +x ./Kosmos-<version>-x86_64.AppImage
./Kosmos-<version>-x86_64.AppImage
```

### Arch Linux (AUR)

Install the pre-built binary:

```bash
yay -S kosmos-bin
```

Or build from source:

```bash
yay -S kosmos
```

## Uninstall

Use the uninstall method that matches how you installed Kosmos.

### Install Script

```bash
curl -fsSL https://raw.githubusercontent.com/etchebarne/kosmos/main/scripts/uninstall.sh | sh
```

### Arch Linux (AUR)

Remove the pre-built package:

```bash
sudo pacman -R kosmos-bin
```

Or remove the source-built package:

```bash
sudo pacman -R kosmos
```

### AppImage

Remove the AppImage desktop entry and icon:

```bash
./Kosmos-<version>-x86_64.AppImage --uninstall
```

Then delete the AppImage file itself.

## Building from source

Install [Rust](https://www.rust-lang.org/tools/install) stable and the GPUI system dependencies for your distro.

### System Dependencies

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

### Run From Source

```bash
cargo run --release
```

## License

MIT - see [LICENSE](LICENSE).

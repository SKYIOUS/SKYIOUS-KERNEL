# Required Tools and Dependencies

To build SkyOS, you need the following tools installed.

## Essential Tools

| Tool | Version | Purpose |
|------|---------|---------|
| Rust nightly | > 2024-01-01 | Compiler |
| Cargo | > 1.75 | Package manager |
| QEMU | > 7.0 | Emulation and testing |
| Python | > 3.8 | Build scripts |
| make | > 4.0 | Boot image creation |
| xorriso | > 1.5 | ISO creation (optional) |
| git | > 2.30 | Version control |

## Rust Setup

```bash
# Install rustup (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install nightly toolchain
rustup install nightly

# Add the kernel target
rustup target add x86_64-unknown-none --toolchain nightly

# Install additional components
rustup component add rust-src --toolchain nightly
rustup component add llvm-tools-preview --toolchain nightly
```

## QEMU Setup

### Linux
```bash
# Ubuntu/Debian
sudo apt-get install qemu-system-x86 qemu-utils

# Fedora
sudo dnf install qemu-system-x86 qemu-img

# Arch Linux
sudo pacman -S qemu-desktop
```

### Windows / macOS
Download QEMU from the official website or use a package manager:
- **macOS**: `brew install qemu`
- **Windows**: Download from https://www.qemu.org/download/

## Python Packages

```bash
pip install toml jinja2
```

## Verification

Run `cargo check` in the project root to verify the build environment is correctly set up. The command should complete without errors.

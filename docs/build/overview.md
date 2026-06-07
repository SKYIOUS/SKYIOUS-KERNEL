# Build System Overview

SkyOS uses a Rust-based build system built on Cargo with custom build scripts.

## Build Architecture

The build process consists of several stages:

1. **Kernel compilation**: The kernel is compiled as a Rust static library for `x86_64-skyos-unknown` target
2. **Linker script**: A custom linker script positions kernel segments at the correct virtual addresses
3. **Boot image creation**: The bootloader (UEFI) and kernel are combined into a bootable image
4. **Filesystem image**: A ramdisk containing the init binary and essential files is created

## Build Commands

| Command | Description |
|---------|-------------|
| `cargo build` | Debug build |
| `cargo build --release` | Release build (optimized) |
| `cargo run` | Build and run in QEMU |
| `cargo test` | Run unit tests |
| `cargo doc` | Generate documentation |
| `cargo clippy` | Run linter |

## Build Outputs

- `target/x86_64-skyos/release/skyos` - Kernel ELF binary
- `target/x86_64-skyos/release/bootimage-skyos.bin` - UEFI bootable image
- `target/initrd.img` - Initial ramdisk image

## Build Configuration

The build can be configured through:
- Cargo features (see [Configuration](configuration.md))
- Environment variables (`QEMU_MEM`, `QEMU_SMP`, etc.)
- Command-line arguments to the build script

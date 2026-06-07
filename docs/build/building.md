# How to Build the Kernel

This guide covers the various build configurations for SkyOS.

## Standard Build

```bash
# Debug build (faster compilation, more runtime checks)
cargo build

# Release build (optimized, smaller binary)
cargo build --release
```

## Building for QEMU

```bash
# Build and run in QEMU
cargo run --release

# With specific QEMU configuration
RUST_LOG=debug QEMU_MEM=1G QEMU_SMP=4 cargo run --release
```

## Build Artifacts

After a successful build, the files are located in:
- Kernel binary: `target/x86_64-skyos/release/skyos`
- Bootable image: `target/x86_64-skyos/release/bootimage-skyos.bin`
- Object files: `target/x86_64-skyos/release/deps/`

## Building Individual Components

```bash
# Build only the kernel library
cargo build -p skyos-kernel

# Build the bootloader
cargo build -p skyos-bootloader

# Build userspace programs
cargo build -p skyos-userspace
```

## Build Verbosity

To see detailed compiler output:

```bash
cargo build --verbose
CARGO_LOG=trace cargo build
```

## Cleaning

```bash
# Clean all build artifacts
cargo clean

# Clean only the kernel
cargo clean -p skyos-kernel
```

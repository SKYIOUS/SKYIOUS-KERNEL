# Getting Started

This guide walks you through setting up the development environment and running SkyOS for the first time.

## Prerequisites

- Rust nightly toolchain
- QEMU 6.0+ (for testing)
- Python 3.8+ (for build scripts)
- A Linux or macOS host (Windows via WSL2 works)

## Quick Start

```bash
# Clone the repository
git clone https://github.com/skyos/skyos.git
cd skyos

# Build the kernel
cargo build --release

# Run in QEMU
cargo run --release
```

## Project Structure

The repository is organized as follows:
- `src/kernel/` - Core kernel code
- `src/arch/` - Architecture-specific code (x86_64, aarch64)
- `src/drivers/` - Hardware drivers
- `src/syscalls/` - System call implementations
- `src/vfs/` - Virtual file system
- `userspace/` - Userspace libraries and programs
- `boot/` - UEFI bootloader

## First Boot

When SkyOS boots, you should see:
1. UEFI bootloader initializes
2. Kernel decompresses and loads
3. CPU and memory initialization logs
4. The async executor starts
5. A shell prompt appears on the serial console

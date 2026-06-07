# Building Userspace Programs

This guide covers building programs for the SkyOS userspace environment.

## Cross-Compiler Setup

SkyOS userspace programs require a cross-compiler targeting `x86_64-skyos-unknown`.

### Using the Provided Toolchain

```bash
# Download the pre-built toolchain
make toolchain-download

# Add to PATH
export PATH=$PATH:$(pwd)/toolchain/bin
```

### Building from Source

```bash
# Build GCC cross-compiler
make toolchain-build
```

## Compiling Programs

```bash
# Compile a C program
x86_64-skyos-gcc -o hello hello.c

# Compile with optimizations
x86_64-skyos-gcc -O2 -o hello hello.c

# Static linking
x86_64-skyos-gcc -static -o hello hello.c
```

## Building the Init System

The init system and core userspace programs are built as part of the main build:

```bash
cargo build -p skyos-userspace
```

This produces binaries in `target/x86_64-skyos/release/`.

## Including in the Initrd

Userspace binaries are packed into the initial ramdisk:

```bash
# Create initrd with default binaries
make initrd

# Create initrd with custom binaries
make initrd INITRD_EXTRA=/path/to/binaries
```

## Library Support

Userspace programs link against `libc.a` (the SkyOS C standard library) and `libgui.a` (GUI toolkit):

```bash
x86_64-skyos-gcc -o myapp myapp.c -lgui -lc
```

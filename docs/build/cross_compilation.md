# Cross-compilation for x86_64

SkyOS is cross-compiled using a custom target specification.

## Target Specification

The kernel uses `x86_64-skyos-unknown.json` target spec:

```json
{
    "llvm-target": "x86_64-unknown-none",
    "data-layout": "e-m:e-i64:64-f80:128-n8:16:32:64-S128",
    "arch": "x86_64",
    "target-endian": "little",
    "target-pointer-width": "64",
    "target-c-int-max-width": "64",
    "os": "none",
    "executables": true,
    "linker-flavor": "ld.lld",
    "linker": "rust-lld",
    "panic-strategy": "abort",
    "disable-redzone": true,
    "features": "-mmx,-sse,+soft-float"
}
```

## Key Settings

- **`disable-redzone: true`**: Required for interrupt handling (the red zone would be corrupted by interrupt stacks)
- **`panic-strategy: abort`**: No unwinding in kernel space; panics halt the system
- **`-mmx,-sse,+soft-float`**: Disable SIMD registers (not saved on context switches); use soft floats

## Cross-compiling from Any Host

The build works on any host platform (Linux, macOS, Windows) because:
1. Rust supports cross-compilation natively
2. The target spec avoids host-specific dependencies
3. `rust-lld` is used as the linker (bundled with Rust)
4. The bootloader is built in the same target

## Building Userspace Programs

Userspace programs use the same target spec or a separate `x86_64-skyos-unknown` user target:

```bash
# Build a userspace program
x86_64-skyos-gcc -ffreestanding -nostdlib -o hello hello.c libskyos.a
```

A GCC cross-compiler for the SkyOS target can be built using `crosstool-ng` or obtained from the project's toolchain releases.

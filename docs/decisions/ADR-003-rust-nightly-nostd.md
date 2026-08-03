# ADR-003: Rust Nightly + no_std + Bootloader Ecosystem

## Status
Accepted

## Date
2026-08-01

## Context
The kernel required a language and toolchain that could target bare-metal x86_64 with no underlying OS. Key constraints:
- No standard library (no_std)
- UEFI boot protocol support
- Kernel-level features: inline assembly, custom allocator, interrupt ABI
- Multi-architecture support (x86_64, aarch64)
- `-Z stack-protector` for stack canary mitigation

## Decision
Use Rust nightly toolchain with `#![no_std]`, `bootloader_api` v0.11 for UEFI boot, and `x86_64` crate for architecture-specific types.

```toml
cargo-features = ["profile-rustflags"]
```
Required nightly features:
- `abi_x86_interrupt` — x86 interrupt calling convention
- `alloc_error_handler` — custom OOM handling
- `-Z stack-protector=strong` — stack canary injection

## Alternatives Considered

### Stable Rust
- Pros: No nightly instability
- Cons: Missing `abi_x86_interrupt`, no `-Z stack-protector`
- Rejected: Stack canaries and interrupt ABI are non-negotiable for kernel safety

### C/C++
- Pros: Mature kernel ecosystem (Linux, BSD)
- Cons: No memory safety guarantees, manual RAII
- Rejected: Safety guarantees of Rust ownership model are a core design goal

### Custom boot protocol (Multiboot2)
- Pros: No dependency on bootloader_api crate
- Cons: More complex boot path, less portable across firmware
- Rejected: bootloader_api provides clean UEFI abstraction with physical memory mapping

## Consequences
- Nightly updates may occasionally break builds (managed via rust-toolchain.toml pinning)
- bootloader_api provides framebuffer, memory map, RSDP, and initrd in a portable format
- Stack canaries protect against stack buffer overflows in kernel mode
- No std means no Vec::reserve_exact, no std::thread, no std::sync — all custom implementations
- The `alloc` crate is available for Vec, Box, Arc, String, etc.
# Vahi Kernel Crates

Modular `#![no_std]` library crates extracted from the monolithic kernel to
enable parallel development, independent testing, and clean dependency boundaries.

**22 crates** — all build clean for `x86_64-unknown-none`.

## Crate Map

```text
Layer 0 (Foundation — no kernel deps):
    vahi-types          Shared types, trait interfaces, cycle-breaking traits
    vahi-sync           IRQ-safe spin mutex (depends on spin only)
    vahi-crypto         SHA-256, HMAC, PBKDF2, entropy (depends on alloc only)

Layer 1 (Hardware Abstraction):
    vahi-hal            IRQ controller, platform info, TSC timer
    vahi-limine         Limine boot protocol (HHDM, memory map, framebuffer)
    vahi-memory         Frame refcounts, allocation tracking
    vahi-acpi           ACPI table parsing, PCI routing, APIC IDs
    vahi-gdt            GDT/IDT/TSS, per-CPU selectors
    vahi-arch           CPU detection, register manipulation, MSRs
    vahi-interrupts     Exception vectors, IRQ dispatch, page faults

Layer 2 (Subsystem Interfaces):
    vahi-task           Process lifecycle, scheduler, OOM killer, signals
    vahi-vfs            Virtual filesystem: ext2, ext4, SkyFS, FAT32, ramfs, devfs, FUSE
    vahi-net            TCP/UDP networking, DNS, DHCP, congestion control, Unix sockets
    vahi-drivers        NVMe, E1000, xHCI, HDA audio, VirtIO-GPU, PS/2
    vahi-apic           x86_64 APIC: LAPIC, I/O APIC, MSI, timer calibration

Layer 3 (Advanced Subsystems):
    vahi-objects        Kernel object handles, security descriptors
    vahi-syscalls       Syscall dispatch, errno, user memory access, signal types
    vahi-ipc            Pipes, shared memory, message queues
    vahi-pci            PCI bus enumeration, config space access
    vahi-gui            Compositor, windowing, input, rendering pipeline
```

## Dependency Graph

```text
vahi-types (foundation: ProcessProvider, PageFaultHandler, FileOps, etc.)
    ↑
vahi-sync ← spin
vahi-crypto ← alloc only
vahi-limine ← limine crate
vahi-hal ← vahi-sync
vahi-memory ← vahi-sync, x86_64
vahi-apic ← vahi-sync, x86_64, volatile, pic8259
vahi-acpi ← x86_64, spin
vahi-gdt ← x86_64, spin
vahi-arch ← x86_64, spin
vahi-interrupts ← spin
vahi-task ← vahi-types, vahi-sync, vahi-memory, vahi-hal, vahi-arch, vahi-objects, vahi-gdt
vahi-vfs ← vahi-sync, vahi-memory, vahi-drivers, vahi-syscalls, vahi-objects, hashbrown
vahi-net ← vahi-sync, vahi-syscalls, vahi-task, smoltcp, hashbrown
vahi-drivers ← vahi-types, vahi-sync, vahi-interrupts, vahi-acpi, vahi-memory
vahi-objects ← vahi-sync
vahi-syscalls ← vahi-sync, vahi-types
vahi-ipc ← vahi-sync, vahi-types
vahi-pci ← vahi-sync, vahi-memory, vahi-limine, vahi-apic
vahi-gui ← vahi-sync, vahi-types, vahi-memory, vahi-drivers, vahi-vfs, font8x8
```

## Breaking Cycles with Traits

The kernel has three circular dependencies that prevent clean extraction:

| Cycle | Break Strategy | Trait |
|-------|---------------|-------|
| task ↔ memory | `ProcessProvider` trait | `vahi-types` |
| memory ↔ interrupts | `PageFaultHandler` trait | `vahi-types` |
| task ↔ vfs | `FileOps` trait | `vahi-types` |

The kernel registers real implementations during boot via:
```rust
vahi_types::register_process_provider(&PROVIDER);
vahi_types::register_page_fault_handler(&HANDLER);
```

## Extraction Status

| Crate | Status | Lines | Notes |
|-------|--------|-------|-------|
| vahi-types | ✅ Extracted | 532 | 11 trait interfaces, 9 shared types |
| vahi-sync | ✅ Extracted | 165 | IRQ-safe mutex, named constants, inline hot paths |
| vahi-crypto | ✅ Extracted + Tested | 478 | SHA-256, HMAC, PBKDF2 — 9 unit tests passing |
| vahi-hal | ✅ Extracted | 256 | IRQ vectors, platform detection, TSC timer |
| vahi-limine | ✅ Extracted | 140 | Limine protocol bindings |
| vahi-memory | ✅ Partial | 189 | frame_info extracted; buddy/slab/paging in kernel |
| vahi-apic | ✅ Extracted | 865 | Trait-based ACPI/memory providers |
| vahi-acpi | ✅ Scaffolded | 220 | MADT, PCI routing, AP IDs |
| vahi-gdt | ✅ Scaffolded | 76 | Selectors, TSS |
| vahi-arch | ✅ Scaffolded | 141 | CPU detection, MSRs |
| vahi-boot | ✅ Scaffolded | 178 | State machine, logger |
| vahi-interrupts | ✅ Scaffolded | 295 | Exceptions, timer, IDT |
| vahi-task | ✅ Scaffolded | 652+ | Process types, scheduler, OOM, VfsNode trait |
| vahi-vfs | ✅ Scaffolded + Code | 6861 | Full ext2/ext4/SkyFS/FAT32/ramfs/devfs/FUSE |
| vahi-net | ✅ Scaffolded | 500+ | TCP, DNS, DHCP, Unix sockets, zerocopy |
| vahi-drivers | ✅ Scaffolded | 600+ | NVMe, E1000, xHCI, HDA, VirtIO-GPU, PS/2 |
| vahi-objects | ✅ Extracted | 660 | Handle table, security |
| vahi-syscalls | ✅ Scaffolded | 200+ | Dispatch, errno, user access, signal types |
| vahi-ipc | ✅ Scaffolded | 120 | Pipe, shared memory, message queue traits |
| vahi-pci | ✅ Scaffolded | 150 | Config space, BAR, enumeration |
| vahi-gui | ✅ Scaffolded | 500+ | Compositor, windows, input, rendering |

**Total crate code:** ~12,000+ lines of Rust

## Kernel Integration Status

The kernel depends on all 20 crates. The remaining integration work is
making the kernel's internal modules re-export crate types instead of
defining their own copies. This affects:

1. `objects/` — kernel defines `ObjectTypeId`, `ObjectHeader`, `KernelObject`
2. `task/` — kernel defines `Process`, `VfsNode`, `Credentials`
3. `vfs/` — kernel defines `Stat`, `FileSystem`, `VfsManager`
4. `net/` — kernel defines `SocketHandle`, TCP state
5. `syscalls/` — kernel defines `SignalState`, `SeccompState`, `PtraceState`

The crates provide the canonical type definitions; the kernel modules need
to re-export from them rather than redefining locally.

## Build Commands

```bash
# Build all crates
for crate in $(ls crates/ | grep -v README); do
  cargo build -p "vahi-${crate}" --target x86_64-unknown-none
done

# Build kernel (uses all crates)
cargo build -p vahi_kernel --target x86_64-unknown-none --release

# Clippy
cargo clippy --target x86_64-unknown-none -p vahi_kernel -- -D warnings
```

## Crate Design Principles

1. **`no_std` only.** All crates use `alloc` never `std`.
2. **Trait-based decoupling.** Cross-crate communication via traits, not direct imports.
3. **Zero kernel deps in foundation.** `vahi-types` depends on nothing but `spin`.
4. **`Send + Sync` on all traits.** Safe for SMP from day one.
5. **Explicit `# Safety` on `unsafe` blocks.** Every unsafe documents its invariant.
6. **Lock ordering documented per-crate.** Prevents deadlocks across modules.
7. **`#[must_use]` on query functions.** Prevents silently dropped results.
8. **`#[inline]` on hot paths.** Zero-cost abstractions for critical paths.
9. **Named constants.** No magic numbers in public APIs.

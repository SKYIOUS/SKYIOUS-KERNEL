# Vahi Kernel Crates

Modular `#![no_std]` library crates extracted from the monolithic kernel to
enable parallel development, independent testing, and clean dependency boundaries.

**17 workspace crates** (+ gdt not in workspace, sync is kernel-internal) — all build clean for `x86_64-unknown-none`.
**Cross-arch broken:** aarch64 fails at kernel's sync (x86_64-only asm in kernel/src/sync/); riscv64 not built in CI.

## Crate Map

```text
Layer 0 (Foundation — no kernel deps):
    vahi-types          Shared types, trait interfaces, cycle-breaking traits (532 lines)
    vahi-crypto         SHA-256, HMAC, PBKDF2, entropy (478 lines, 9 tests)
    vahi-hal            IRQ controller, platform info, TSC timer (256 lines)
    vahi-limine         Limine boot protocol (HHDM, memory map, framebuffer) (140 lines)

Layer 1 (Hardware Abstraction):
    vahi-memory         Frame refcounts, allocation tracking ONLY (189 lines) — buddy/slab/paging in kernel
    vahi-acpi           ACPI table parsing, PCI routing, APIC IDs (220 lines, scaffolded)
    vahi-gdt            GDT/IDT/TSS, per-CPU selectors (76 lines, scaffolded) — NOT IN WORKSPACE
    vahi-arch           CPU detection, register manipulation, MSRs (141 lines, scaffolded)
    vahi-apic           x86_64 APIC: LAPIC, I/O APIC, MSI, timer calibration (865 lines, trait-based) — kernel copy LIVE

Layer 2 (Subsystem Interfaces):
    vahi-task           Process lifecycle, scheduler, OOM killer, signals (652+ lines, scaffolded)
    vahi-vfs            Virtual filesystem: ext2, ext4, SkyFS, FAT32, ramfs, devfs, FUSE (6861 lines, scaffolded+code)
    vahi-net            TCP/UDP networking, DNS, DHCP, congestion control, Unix sockets (500+ lines, scaffolded)
    vahi-drivers        NVMe, E1000, xHCI, HDA audio, VirtIO-GPU, PS/2 (600+ lines, scaffolded)
    vahi-pci            PCI bus enumeration, config space access (150 lines, scaffolded)
    vahi-ipc            Pipes, shared memory, message queues (120 lines, scaffolded)

Layer 3 (Advanced Subsystems):
    vahi-objects        Kernel object handles, security descriptors (660 lines, extracted)
    vahi-syscalls       Syscall dispatch, errno, user memory access, signal types (200+ lines, scaffolded)
    vahi-verified       Verified proofs (concurrency, journal, scheduler, runner) — feature-gated, optional dep
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
vahi-task ← vahi-types, vahi-sync, vahi-memory, vahi-hal, vahi-arch, vahi-objects, vahi-gdt
vahi-vfs ← vahi-sync, vahi-memory, vahi-drivers, vahi-syscalls, vahi-objects, hashbrown
vahi-net ← vahi-sync, vahi-syscalls, vahi-task, smoltcp, hashbrown
vahi-drivers ← vahi-types, vahi-sync, vahi-interrupts, vahi-acpi, vahi-memory
vahi-objects ← vahi-sync
vahi-syscalls ← vahi-sync, vahi-types
vahi-ipc ← vahi-sync, vahi-types
vahi-pci ← vahi-sync, vahi-memory, vahi-limine, vahi-apic
```

**Critical issue:** `vahi-sync` uses `core::arch::x86_64::__cpuid` and `asm!("pushfq; pop {rflags}; cli", options(att_syntax))` unconditionally at `crates/sync/src/lib.rs:205,255` — breaks aarch64/riscv64 builds (issues.md P-1).

## Extraction Status (Honest)

| Crate | Status | Lines | Reality |
|-------|--------|-------|---------|
| vahi-types | ✅ Extracted | 532 | 11 trait interfaces, 9 shared types — foundation |
| vahi-sync | ✅ Extracted | 165 | IRQ-safe mutex — **x86_64-only asm breaks cross-arch** |
| vahi-crypto | ✅ Extracted + Tested | 478 | SHA-256, HMAC, PBKDF2 — 9 unit tests passing |
| vahi-hal | ✅ Extracted | 256 | IRQ vectors, platform detection, TSC timer |
| vahi-limine | ✅ Extracted | 140 | Limine protocol bindings |
| vahi-memory | ⚠️ Partial | 189 | frame_info ONLY; buddy/slab/paging remain in kernel |
| vahi-apic | ✅ Extracted | 865 | Trait-based ACPI/memory providers — **kernel copy still live (41 call sites)** |
| vahi-acpi | ⚠️ Scaffolded | 220 | MADT, PCI routing, AP IDs |
| vahi-arch | ⚠️ Scaffolded | 141 | CPU detection, MSRs |
| vahi-gdt | ⚠️ Scaffolded | 76 | Selectors, TSS — **NOT in workspace** |
| vahi-interrupts | ❌ Kernel only | — | Exception vectors, page faults — NOT extracted |
| vahi-task | ⚠️ Scaffolded | 652+ | Process types, scheduler, OOM, VfsNode trait |
| vahi-vfs | ⚠️ Scaffolded + Code | 6861 | Full ext2/ext4/SkyFS/FAT32/ramfs/devfs/FUSE |
| vahi-net | ⚠️ Scaffolded | 500+ | TCP, DNS, DHCP, Unix sockets, zerocopy |
| vahi-drivers | ⚠️ Scaffolded | 600+ | NVMe, E1000, xHCI, HDA, VirtIO-GPU, PS/2 |
| vahi-objects | ✅ Extracted | 660 | Handle table, security |
| vahi-syscalls | ⚠️ Scaffolded | 200+ | Dispatch, errno, user access, signal types |
| vahi-ipc | ⚠️ Scaffolded | 120 | Pipe, shared memory, message queue traits |
| vahi-pci | ⚠️ Scaffolded | 150 | Config space, BAR, enumeration |
| vahi-verified | ⚠️ Feature-gated | — | Verified proofs (optional dep) |
| vahi-gui | ❌ Kernel only | — | Compositor, windows, input, rendering — NOT extracted |

**Total crate code:** ~12,000+ lines of Rust

## Kernel Integration Status

The kernel depends on all 17 workspace crates. The remaining integration work is
making the kernel's internal modules re-export crate types instead of
defining their own copies. This affects:

1. `objects/` — kernel defines `ObjectTypeId`, `ObjectHeader`, `KernelObject` (crate extracted)
2. `task/` — kernel defines `Process`, `VfsNode`, `Credentials` (crate scaffolded)
3. `vfs/` — kernel defines `Stat`, `FileSystem`, `VfsManager` (crate has full implementation)
4. `net/` — kernel defines `SocketHandle`, TCP state (crate scaffolded)
5. `syscalls/` — kernel defines `SignalState`, `SeccompState`, `PtraceState` (crate scaffolded)
6. `apic/` — **kernel copy is LIVE** (41 call sites use `crate::apic::`); crate duplicate has 4 call sites (MSI broken — issues.md A-3)
7. `memory/` — buddy/slab/paging in kernel; crate has frame_info only
8. `interrupts/` — not extracted; kernel internal
9. `gui/` / `compositor/` — not extracted; kernel internal
10. `ebpf/` / `hypervisor/` / `ash/` — not extracted; kernel internal

The crates provide the canonical type definitions; the kernel modules need
to re-export from them rather than redefining locally.

## Build Commands

```bash
# Build all workspace crates
for crate in crypto hal limine apic memory types arch acpi objects ipc pci syscalls task vfs net drivers verified; do
  cargo build -p "vahi-${crate}" --target x86_64-unknown-none
done

# Build kernel (uses all crates)
cargo build -p vahi_kernel --target x86_64-unknown-none --release

# Clippy
cargo clippy --target x86_64-unknown-none -p vahi_kernel -- -D warnings

# Cross-arch (currently FAILS at vahi-sync)
cargo build --target aarch64-unknown-none -p vahi_kernel --features smp
cargo check --target riscv64gc-unknown-none-elf -p vahi_kernel
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

## Known Crate Issues (from issues.md)

| Crate | Issue | Severity |
|-------|-------|----------|
| `vahi-sync` | x86_64-only asm/intrinsics unconditional (P-1) | HIGH — blocks cross-arch |
| `vahi-apic` | MSI pool never initialized (A-3) — crate duplicate unused for MSI | HIGH — NVMe/PCIe silently fall back to INTx |
| `vahi-memory` | Only frame_info extracted; buddy/slab/paging in kernel | MEDIUM — incomplete extraction |
| `vahi-task` | Scaffolded; kernel defines real Process/VfsNode/Credentials | MEDIUM — integration incomplete |
| `vahi-syscalls` | Scaffolded; kernel defines real SignalState/SeccompState/PtraceState | MEDIUM — integration incomplete |
| `vahi-vfs` | Ext2 dir traversal unbounded (F-1); SkyFS extent copy unbounded (F-2) | HIGH — security/stability |
| `vahi-drivers` | NVMe queue bugs (N-1, N-2); AHCI completion unbounded (A-1), CTBA bug (A-2); E1000 IRQ allocates (E-1); xHCI IMAN bug (X-2) | HIGH |
| `vahi-net` | Socket options return ENOPROTOOPT (fixed); only SO_REUSEPORT works | MEDIUM |
| `vahi-verified` | Optional dep; feature-gated; not built by default | LOW |
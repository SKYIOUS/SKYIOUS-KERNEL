# Vahi Kernel — Module Map

**Purpose:** Define module boundaries, ownership, interfaces, and dependencies so multiple agents can work simultaneously without conflicts.

## Crate Structure (Workspace Reality)

```
crates/
  types/       ← vahi-types: shared types, Errno, credentials, VMA, trait interfaces (532 lines) — WORKSPACE
  crypto/      ← vahi-crypto: SHA-256, HMAC, PBKDF2, entropy (478 lines, 9 tests) — WORKSPACE
  hal/         ← vahi-hal: DMA, IRQ controller, platform, timer (256 lines) — WORKSPACE
  limine/      ← vahi-limine: Limine boot protocol (140 lines) — WORKSPACE
  apic/        ← vahi-apic: Local APIC, I/O APIC, MSI (865 lines, trait-based providers) — WORKSPACE
  memory/      ← vahi-memory: frame_info ONLY (189 lines) — buddy/slab/paging in kernel — WORKSPACE
  objects/     ← vahi-objects: KernelObject trait, handle table, security (660 lines) — WORKSPACE
  ipc/         ← vahi-ipc: IPC primitives (120 lines, scaffolded) — WORKSPACE
  pci/         ← vahi-pci: PCI config space, BAR mapping (150 lines, scaffolded) — WORKSPACE
  syscalls/    ← vahi-syscalls: syscall numbers, dispatch, errno, user access, signal types (200+ lines, scaffolded) — WORKSPACE
  task/        ← vahi-task: process, thread, scheduler, OOM (652+ lines, scaffolded) — WORKSPACE
  vfs/         ← vahi-vfs: VFS layer, ext2/ext4/SkyFS/FAT32/devfs/FUSE (6861 lines, scaffolded+code) — WORKSPACE
  net/         ← vahi-net: TCP/UDP, DHCP, DNS, Unix sockets (500+ lines, scaffolded) — WORKSPACE
  drivers/     ← vahi-drivers: NVMe, E1000, xHCI, HDA, VirtIO-GPU, PS/2 (600+ lines, scaffolded) — WORKSPACE
  arch/        ← vahi-arch: x86_64, aarch64, HAL (141 lines, scaffolded) — WORKSPACE
  acpi/        ← vahi-acpi: ACPI tables, MADT, PRT (220 lines, scaffolded) — WORKSPACE
  verified/    ← vahi-verified: Verified proofs (feature-gated, optional dep) — WORKSPACE
  gdt/         ← vahi-gdt: GDT/IDT/TSS management (76 lines, scaffolded) — NOT IN WORKSPACE
```

**Total: 17 workspace crates + gdt (not in workspace); sync is kernel-internal (kernel/src/sync/)**

**Kernel internal (not extracted):**
- `kernel/src/boot/` — boot state machine, init, logger, tasks
- `kernel/src/interrupts/` — exception vectors, IRQ dispatch, page faults
- `kernel/src/gui/` — GUI compositor, window manager, terminal, splash
- `kernel/src/compositor/` — blend, blur, flush, scene, shadow, vsync
- `kernel/src/ebpf/` — eBPF VM, verifier, JIT, maps, helpers
- `kernel/src/hypervisor/` — VMX/SVM, EPT/NPT, vCPU, hypercalls
- `kernel/src/ash/` — ASH scripting engine, verifier, hooks
- `kernel/src/shell/` — kernel shell
- `kernel/src/sync/` — IrqSafeMutex, CFI, RCU (kernel copy, crate also exists)
- `kernel/src/apic/` — APIC subsystem (kernel copy LIVE, crate duplicate)
- `kernel/src/memory/` — buddy, slab, paging, swap (crate has frame_info only)
- `kernel/src/task/` — process, thread, scheduler (crate scaffolded)
- `kernel/src/vfs/` — VFS manager (crate has full implementation)
- `kernel/src/drivers/` — device drivers (crate scaffolded)
- `kernel/src/net/` — network stack (crate scaffolded)
- `kernel/src/syscalls/` — syscall implementations (crate scaffolded)
- `kernel/src/objects/` — object manager (crate extracted)
- `kernel/src/verified/` — verified proofs (crate feature-gated)

## Crate Dependency Graph

```
Tier 0 (no kernel deps):
  vahi-sync, vahi-types, vahi-crypto, vahi-hal, vahi-limine, vahi-apic, vahi-acpi

Tier 1 (depends on Tier 0):
  vahi-memory ← vahi-sync + x86_64
  vahi-arch   ← vahi-sync + x86_64
  vahi-objects ← vahi-sync + vahi-types

Tier 2 (depends on Tier 0-1):
  vahi-interrupts ← vahi-sync + vahi-arch  (NOT IN WORKSPACE - kernel internal)
  vahi-task       ← vahi-sync + vahi-memory + vahi-types
  vahi-vfs        ← vahi-sync + vahi-memory
  vahi-net        ← vahi-sync + vahi-syscalls + vahi-task + smoltcp + hashbrown
  vahi-drivers    ← vahi-types + vahi-sync + vahi-interrupts + vahi-acpi + vahi-memory
  vahi-ipc        ← vahi-sync + vahi-types
  vahi-pci        ← vahi-sync + vahi-memory + vahi-limine + vahi-apic
  vahi-syscalls   ← vahi-sync + vahi-types
  vahi-gui        ← vahi-sync + vahi-types + vahi-memory + vahi-drivers + vahi-vfs + font8x8 (NOT IN WORKSPACE - kernel internal)

Tier 3+ (depends on Tier 0-2):
  vahi_kernel ← everything above
```

**Critical issue:** `vahi-sync` uses `core::arch::x86_64::__cpuid` and `asm!("pushfq; pop {rflags}; cli", options(att_syntax))` unconditionally — breaks aarch64/riscv64 builds (issues.md P-1).

## Extraction Status (Honest)

| Crate | Status | Lines | Reality |
|-------|--------|-------|---------|
| vahi-types | ✅ Extracted | 532 | 11 trait interfaces, 9 shared types — foundation |
| vahi-sync | ✅ Extracted | 165 | IRQ-safe mutex — **x86_64-only asm breaks cross-arch** |
| vahi-crypto | ✅ Extracted + Tested | 478 | SHA-256, HMAC, PBKDF2 — 9 unit tests passing |
| vahi-hal | ✅ Extracted | 256 | IRQ vectors, platform detection, TSC timer |
| vahi-limine | ✅ Extracted | 140 | Limine protocol bindings |
| vahi-memory | ⚠️ Partial | 189 | frame_info ONLY; buddy/slab/paging remain in kernel |
| vahi-apic | ✅ Extracted | 865 | Trait-based ACPI/memory providers — **kernel copy still live** |
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

## Kernel Integration Status

The kernel depends on all 19 workspace crates. Remaining integration work:

1. **`objects/`** — kernel defines `ObjectTypeId`, `ObjectHeader`, `KernelObject` (crate extracted, kernel must re-export)
2. **`task/`** — kernel defines `Process`, `VfsNode`, `Credentials` (crate scaffolded)
3. **`vfs/`** — kernel defines `Stat`, `FileSystem`, `VfsManager` (crate has full implementation)
4. **`net/`** — kernel defines `SocketHandle`, TCP state (crate scaffolded)
5. **`syscalls/`** — kernel defines `SignalState`, `SeccompState`, `PtraceState` (crate scaffolded)
6. **`apic/`** — kernel copy is LIVE (41 call sites); crate duplicate has 4 call sites (MSI broken A-3)
7. **`memory/`** — buddy/slab/paging in kernel; crate has frame_info only
8. **`interrupts/`** — not extracted; kernel internal
9. **`gui/` / `compositor/`** — not extracted; kernel internal

## Module Dependency Graph

```
                          ┌─────────────┐
                          │   boot      │
                          │  (init)     │
                          └──────┬──────┘
                                 │
                      ┌──────────┼──────────┐
                      │          │          │
                ┌─────▼────┐ ┌──▼───┐ ┌───▼────┐
                │ memory   │ │ arch │ │ sync   │
                │ (paging) │ │      │ │(mutex) │
                └─────┬────┘ └──┬───┘ └───┬────┘
                      │         │         │
           ┌──────────┼─────────┼─────────┼──────────┐
           │          │         │         │          │
      ┌────▼────┐ ┌───▼──┐ ┌───▼───┐ ┌───▼──┐ ┌────▼────┐
      │  task   │ │ vfs  │ │syscalls│ │ net  │ │drivers │
      │(process)│ │      │ │        │ │      │ │        │
      └────┬────┘ └──┬───┘ └───┬───┘ └──┬───┘ └────┬────┘
           │         │         │         │          │
           └─────────┴─────────┴─────────┴──────────┘
                                 │
                          ┌──────▼──────┐
                          │  tests      │
                          │ (selftest)  │
                          └─────────────┘
```

**Arrows indicate "depends on"** — the source module can use the target module's public API. Reverse dependencies are forbidden (no cycles).

---

## Module Registry

### Tier 0: Foundation (no kernel dependencies)

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `types` | `crates/types/` | TBD | 10 | 532 | Extracted |
| `crypto` | `crates/crypto/` | TBD | 4 | 478 | Extracted + Tested |
| `hal` | `crates/hal/` | TBD | 5 | 256 | Extracted |
| `limine` | `crates/limine/` | TBD | 1 | 140 | Extracted |
| `apic` | `crates/apic/` | TBD | 6 | 865 | Extracted (kernel copy LIVE) |
| `acpi` | `crates/acpi/` | TBD | 3 | 220 | Scaffolded |

**Note:** `sync` (IrqSafeMutex) is kernel-internal at `kernel/src/sync/`, not a workspace crate. It uses x86_64-only asm (issues.md P-1).

### Tier 1: Core (depends on Tier 0)

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `memory` | `crates/memory/` | TBD | 2 | 189 | Partial (frame_info only) |
| `objects` | `crates/objects/` | TBD | 4 | 660 | Extracted |
| `ipc` | `crates/ipc/` | TBD | 1 | 120 | Scaffolded |
| `pci` | `crates/pci/` | TBD | 1 | 150 | Scaffolded |
| `arch` | `crates/arch/` | TBD | 3 | 141 | Scaffolded |

### Tier 2: Subsystems (depends on Tier 0-1)

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `task` | `crates/task/` | TBD | 8 | 652+ | Scaffolded |
| `vfs` | `crates/vfs/` | TBD | 25 | 6861 | Scaffolded + Code |
| `drivers` | `crates/drivers/` | TBD | 15 | 600+ | Scaffolded |
| `net` | `crates/net/` | TBD | 8 | 500+ | Scaffolded |
| `syscalls` | `crates/syscalls/` | TBD | 5 | 200+ | Scaffolded |

### Tier 3: Features (depends on Tier 0-2)

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `verified` | `crates/verified/` | TBD | 4 | — | Feature-gated |
| `gui` | `kernel/src/gui/` | TBD | 15 | 5500 | Kernel internal |
| `ebpf` | `kernel/src/ebpf/` | TBD | 7 | 1907 | Kernel internal |
| `hypervisor` | `kernel/src/hypervisor/` | TBD | 11 | 2641 | Kernel internal |
| `ash` | `kernel/src/ash/` | TBD | 8 | 759 | Kernel internal |
| `shell` | `kernel/src/shell/` | TBD | 7 | 1200 | Kernel internal |

### Tier 4: Tests

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `tests` | `kernel/src/tests/` | TBD | 16 | ~800 | Growing |
| `selftest` | `kernel/src/selftest.rs` | TBD | 1 | 71 | Stable |

---

## Cross-Module Interface Rules

### Rule 1: Public API is the Contract

Each module exposes a public API. Other modules must use only this API. Internal implementation details are not accessible across modules.

**Enforced by:** Rust's visibility rules (`pub`, `pub(crate)`, private)

### Rule 2: No Circular Dependencies

Module A can depend on Module B, but Module B cannot depend on Module A. The dependency graph must be a DAG (directed acyclic graph).

**Enforced by:** The tier system (Tier 0 → Tier 1 → Tier 2 → Tier 3 → Tier 4)

### Rule 3: Cross-Module Changes Require Both Owners

If a change touches code in Module A AND Module B, both owners must approve. If the owners disagree, escalate to the project lead.

### Rule 4: Interface Changes Require Documentation Update

If you change a module's public API, you must update the corresponding interface document in `docs/interfaces/`.

### Rule 5: Invariants Must Be Preserved

Each module has invariants documented in `docs/invariants/`. Changes must not violate these invariants. If a change requires modifying an invariant, the invariant document must be updated in the same commit.

---

## Lock Ordering (Global Constraint)

The following lock ordering must be respected across ALL modules:

```
PROCESS_TABLE
  → per-process locks (files, memory, signals, identity, ...)
    → REFCOUNTS
      → BUDDY_ALLOCATOR
        → VFS locks
          → NETWORK locks
```

**Violation = deadlock.** This ordering is enforced by code review, not by the type system.

---

## File Size Guidelines

| Threshold | Action |
|-----------|--------|
| < 500 lines | Normal |
| 500-1000 lines | Monitor — consider splitting |
| > 1000 lines | Must split or document why |

**Current oversized files (kernel):**
- `kernel/src/main.rs`: 614 lines — needs decompose (F2.3)
- `kernel/src/task/process.rs`: 1034 lines — needs split (extract types)
- `kernel/src/syscalls/process_lifecycle.rs`: 982 lines — needs split (extract wait/exit)
- `kernel/src/iommu.rs`: 1141 lines — acceptable (single driver)
- `kernel/src/syscalls/dispatch.rs`: 1692 lines — OVER, needs split
- `kernel/src/syscalls/fs_stat.rs`: 1152 lines — OVER, needs split
- `crates/vfs/src/defs.rs`: 933 lines — impending
- `crates/drivers/src/usb/xhci/mod.rs`: 899 lines — impending
- `crates/vfs/src/skyfs/mod.rs`: 893 lines — impending

---

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
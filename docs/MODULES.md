# Vahi Kernel — Module Map

**Purpose:** Define module boundaries, ownership, interfaces, and dependencies so multiple agents can work simultaneously without conflicts.

## Crate Structure

```
crates/
  sync/        ← vahi-sync: IrqSafeMutex (EXTRACTED)
  types/       ← vahi-types: shared types, Errno, cycle-breaking traits (EXTRACTED)
  crypto/      ← vahi-crypto: SHA-256, HMAC, PBKDF2, entropy (EXTRACTED)
  hal/         ← vahi-hal: IRQ, platform, timer (EXTRACTED)
  limine/      ← vahi-limine: Limine boot protocol (EXTRACTED)
  apic/        ← vahi-apic: Local APIC, I/O APIC, MSI (EXTRACTED)
  memory/      ← vahi-memory: frame_info EXTRACTED, rest scaffolded
  objects/     ← vahi-objects: kernel object handles (EXTRACTED)
  ipc/         ← vahi-ipc: IPC endpoints, messages, ports (EXTRACTED)
  pci/         ← vahi-pci: PCI config space, BAR mapping (PARTIAL)
  syscalls/    ← vahi-syscalls: errno, numbers (SCAFFOLDED)
  ebpf/        ← vahi-ebpf: eBPF VM, verifier, JIT (SCAFFOLDED)
  gui/         ← vahi-gui: compositor, windows, input (SCAFFOLDED)
  interrupts/  ← vahi-interrupts: IRQ, page fault, exceptions (SCAFFOLDED)
  task/        ← vahi-task: process, thread, scheduler, OOM (SCAFFOLDED)
  vfs/         ← vahi-vfs: VFS layer, filesystems (SCAFFOLDED)
  net/         ← vahi-net: TCP, UDP, DHCP, DNS (SCAFFOLDED)
  drivers/     ← vahi-drivers: serial, PS/2, E1000, VirtIO (SCAFFOLDED)
  arch/        ← vahi-arch: x86_64, aarch64, HAL (SCAFFOLDED)
  boot/        ← vahi-boot: boot state machine, init (SCAFFOLDED)
  gdt/         ← vahi-gdt: GDT/TSS management (EXTRACTED)
  acpi/        ← vahi-acpi: ACPI tables, MADT, PRT (EXTRACTED)
kernel/
  src/         ← vahi_kernel: everything else (depends on vahi-sync)
```

**Extraction plan:** See `docs/CRATE_EXTRACTION_PLAN.md`

**Build:** `cd kernel && cargo build --release --target x86_64-unknown-none`
**Build all crates:** `cargo build --release --target x86_64-unknown-none -p vahi_kernel`

### Crate Dependency Graph

```
Tier 0 (no kernel deps):
  vahi-sync, vahi-types, vahi-crypto, vahi-hal, vahi-limine, vahi-apic, vahi-gdt, vahi-acpi

Tier 1 (depends on Tier 0):
  vahi-memory ← vahi-sync + x86_64
  vahi-arch   ← vahi-sync + x86_64
  vahi-boot   ← vahi-sync + vahi-arch
  vahi-objects ← vahi-sync + vahi-types

Tier 2 (depends on Tier 0-1):
  vahi-interrupts ← vahi-sync + vahi-arch
  vahi-task       ← vahi-sync + vahi-memory + vahi-types
  vahi-vfs        ← vahi-sync + vahi-memory
  vahi-net        ← vahi-sync
  vahi-drivers    ← vahi-sync + vahi-memory
  vahi-ipc        ← vahi-sync + vahi-types
  vahi-pci        ← vahi-sync + vahi-apic + vahi-limine + vahi-memory
  vahi-syscalls   ← vahi-sync + vahi-types + vahi-ipc + vahi-task + ...
  vahi-gui        ← vahi-sync + vahi-types + vahi-vfs + vahi-drivers + ...
  vahi-ebpf       ← vahi-sync + vahi-types + vahi-syscalls

Tier 3+ (depends on Tier 0-2):
  vahi_kernel ← everything above
```

### How to Add a New Crate

1. Create `crates/<name>/Cargo.toml` with `#![no_std]`
2. Add to workspace members in root `Cargo.toml`
3. Add as dependency in `kernel/Cargo.toml`
4. Re-export from `kernel/src/<module>/mod.rs` so existing imports work
5. Update this document

**Rule:** Each module has ONE owner at a time. Changes to a module require the owner's approval. Cross-module changes require both owners' approval.

---

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
| `sync` | `kernel/src/sync/` | TBD | 3 | 525 | Stable |
| `types` | `crates/types/` | TBD | 1 | 751 | Stable |
| `crypto` | `kernel/src/crypto/` | TBD | 3 | 290 | Stable |
| `hal` | `crates/hal/` | TBD | 4 | 172 | Stable |
| `limine` | `crates/limine/` | TBD | 1 | 140 | Stable |
| `apic` | `crates/apic/` | TBD | 5 | 1072 | Stable |
| `gdt` | `crates/gdt/` | TBD | 1 | 225 | Stable |
| `acpi` | `crates/acpi/` | TBD | 3 | 800 | Stable |

### Tier 1: Core (depends on Tier 0)

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `memory` | `kernel/src/memory/` | TBD | 12 | 2,556 | Partial |
| `boot` | `crates/boot/` | TBD | 5 | 587 | Stable |
| `objects` | `crates/objects/` | TBD | 4 | 1400 | Stable |
| `ipc` | `crates/ipc/` | TBD | 1 | 402 | Extracted |
| `pci` | `kernel/src/pci/` | TBD | 1 | 341 | Partial |

### Tier 2: Subsystems (depends on Tier 0-1)

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `task` | `kernel/src/task/` | TBD | 11 | 3,943 | Scaffolded |
| `vfs` | `kernel/src/vfs/` | TBD | 21 | 6,774 | Partial |
| `drivers` | `kernel/src/drivers/` | TBD | 36 | 8,266 | Scaffolded |
| `net` | `kernel/src/net/` | TBD | 7 | 1,927 | Scaffolded |
| `interrupts` | `kernel/src/interrupts/` | TBD | 5 | 1,050 | Scaffolded |
| `syscalls` | `kernel/src/syscalls/` | TBD | 42 | 16,008 | Scaffolded |
| `gui` | `kernel/src/gui/` | TBD | 15 | 5,500 | Scaffolded |
| `ebpf` | `kernel/src/ebpf/` | TBD | 7 | 1,907 | Scaffolded |

### Tier 3: Features (depends on Tier 0-2)

| Module | Path | Owner | Files | Lines | Status |
|--------|------|-------|-------|-------|--------|
| `hypervisor` | `kernel/src/hypervisor/` | TBD | 11 | 2,641 | Stub |
| `ash` | `kernel/src/ash/` | TBD | 8 | 759 | Experimental |
| `shell` | `kernel/src/shell/` | TBD | 7 | 1,200 | Experimental |

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

**Current oversized files:**
- `process.rs`: 1,034 lines — needs split (extract types to submodule)
- `process_lifecycle.rs`: 982 lines — needs split (extract wait/exit to submodule)
- `iommu.rs`: 1,141 lines — acceptable (single driver, hard to split)

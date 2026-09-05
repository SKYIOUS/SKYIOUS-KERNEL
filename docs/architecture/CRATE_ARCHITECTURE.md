# Crate Architecture — Target Design

## Principle

**Crates define interfaces, shared types, and testable logic.  
Kernel owns concrete implementations, integration, and initialization.**

A crate exists for one of two reasons:
1. **Breaks a dependency cycle** (types, hal, objects, sync)
2. **Contains substantial testable logic** (task, vfs, net, drivers, memory, crypto, ipc, ebpf, gui)

If a crate is <500 lines and provides no testable logic beyond re-exports, it should be absorbed into whichever larger crate or the kernel uses it.

## Dependency Layers

```
Layer 0 (no deps):     sync, types, crypto, limine, gdt
Layer 1 (L0):          hal, arch, apic, ipc, objects, syscalls
Layer 2 (L1):          memory, boot, pci, acpi, ebpf
Layer 3 (L2):          task, interrupts, drivers
Layer 4 (L3):          net, vfs
Layer 5 (L4):          gui
```

## What Lives Where

### Crates (reusable, testable, independently compilable)

| Crate | Owns | Does NOT own |
|-------|------|-------------|
| `vahi-types` | Trait interfaces (ProcessProvider, PageFaultHandler, FileOps), shared type aliases (Pid, VirtAddr) | Concrete implementations |
| `vahi-sync` | IrqSafeMutex, IrqSafeMutexGuard | — |
| `vahi-objects` | ObjectTypeId constants, KernelObject trait, HandleTable, SecurityDescriptor, ObjectNamespace | Concrete object types (ProcessObject, ThreadObject) |
| `vahi-task` | Task, YieldNow, TaskId, ELF loader, scheduler algorithm | Process struct, Thread struct, CURRENT_PROCESS |
| `vahi-vfs` | FileSystem/VfsNode traits, path utilities, mount logic, ext2/skyfs/fat implementations | VfsManager (kernel-owned, uses IrqSafeMutex) |
| `vahi-net` | Socket table, smoltcp integration, TCP Reno, DNS, DHCP, Unix sockets | Network interface initialization |
| `vahi-drivers` | All hardware drivers (AHCI, NVMe, E1000, VirtIO, USB, HDA, PS/2) | Driver registration, IRQ routing |
| `vahi-memory` | Buddy allocator, slab, page tables, frame_info | AddressSpace (kernel-owned, uses CURRENT_PROCESS) |
| `vahi-crypto` | SHA-256, HMAC, PBKDF2, entropy harvester | — |
| `vahi-ipc` | IPC endpoints, message queues, port-based IPC | — |
| `vahi-hal` | IRQ controller trait, platform info, timer trait, DMA buffer | Concrete IRQ routing |
| `vahi-acpi` | MADT parsing, PRT parsing | AML interpreter |
| `vahi-apic` | Local APIC, I/O APIC, MSI allocator, IPI | Interrupt vector registration |
| `vahi-interrupts` | Exception vectors, IRQ dispatch, page fault handler | — |
| `vahi-ebpf` | eBPF VM, verifier, JIT compiler | — |
| `vahi-gui` | Compositor, widgets, drawing, input handling | Framebuffer initialization |
| `vahi-arch` | CPUID, register manipulation | — |
| `vahi-gdt` | GDT/IDT/TSS setup | — |
| `vahi-limine` | Boot protocol requests | — |
| `vahi-pci` | Config space read/write, BAR reading | — |
| `vahi-boot` | Boot state machine | — |

### Kernel (integration, glue, kernel-specific logic)

| Module | Purpose | Relationship to crate |
|--------|---------|----------------------|
| `syscalls/` | Syscall dispatch, all syscall implementations | Uses crate types as parameters/returns |
| `task/` | Process struct, Thread struct, CURRENT_PROCESS, scheduler integration | Extends crate's Task/YieldNow with concrete Process/Thread |
| `drivers/` | Driver registration, IRQ routing, DMA setup | Imports crate drivers, adds kernel-specific init |
| `vfs/` | VfsManager, mount syscall, file descriptor table | Imports crate VFS traits/impls, adds kernel integration |
| `net/` | Network init, socket syscalls, DHCP/DNS client | Imports crate net, adds kernel-specific init |
| `memory/` | AddressSpace, frame_info tracking, buddy init | Imports crate memory, adds kernel-specific init |
| `objects/` | ProcessObject, ThreadObject, WindowObject | Imports crate's HandleTable/KernelObject, adds concrete types |
| `interrupts/` | IDT setup, interrupt vector registration | Uses crate's exception/IRQ handlers |
| `hal/` | DMA setup, exec_mem, IRQ routing | Imports crate's irq/timer/platform |
| `gui/` | Window manager, desktop shell | Imports crate's compositor/widgets |
| `crypto/` | Boot-time entropy init | Imports crate's crypto primitives |
| `sync/` | RCU, CFI | Imports crate's IrqSafeMutex |
| `boot/` | Init process, shell, module loading | Uses crate's boot state machine |

## Rules for New Code

1. **Never duplicate a type.** If a type exists in a crate, import it. If you need to extend it, wrap it (newtype pattern) or add fields in the crate.

2. **Kernel modules are thin.** A kernel module should be <200 lines of glue code. If it's >500 lines, extract logic into the corresponding crate.

3. **One owner per piece of state.** `CURRENT_PROCESS` is owned by `kernel/src/task/process.rs`. `IPC_STATE` is owned by `crates/ipc/src/lib.rs`. `BUDDY_ALLOCATOR` is owned by `crates/memory/src/buddy.rs`. Never have two statics for the same concept.

4. **Crates define, kernel implements.** Crates define traits (FileSystem, VfsNode, KernelObject, ProcessProvider). Kernel implements them for concrete types.

5. **Import direction is one-way.** Kernel imports from crates. Crates never import from kernel. If a crate needs kernel functionality, use a trait defined in a lower-layer crate.

## Migration Status

| Module | Status | Notes |
|--------|--------|-------|
| `crypto/` | ✅ Clean | Re-exports from crate |
| `sync/` | ✅ Clean | Re-exports from crate + kernel-specific RCU/CFI |
| `ipc/` | ✅ Clean | Pure re-export |
| `pci/` | ✅ Clean | Pure re-export |
| `hal/` | ✅ Clean | Re-exports + kernel-specific DMA/exec_mem |
| `objects/` | ⚠️ Partial | 3 files duplicate crate (handle, namespace, security), 8 files kernel-specific |
| `drivers/` | ✅ Clean | Dead files deleted; only `pub use vahi_drivers::*` re-export remains |
| `vfs/` | ✅ Clean | Dead files deleted; only `pub use vahi_vfs::*` re-export remains |
| `task/` | ⚠️ Partial | Kernel owns Process/Thread (transitive types: real syscall security state, vfs VfsNode); re-exports PtyPair, Task/YieldNow, FORK_CHILD_* |
| `net/` | ✅ Clean | Dead files deleted; only `pub use vahi_net::*` re-export remains |
| `memory/` | ⚠️ Partial | Some files duplicate, some kernel-specific |

### Migration Status (updated 2026-09-02)

- **drivers/** ✅ Dead files deleted (36 files, ~8K lines removed). Only `pub use vahi_drivers::*` re-export remains.
- **vfs/** ✅ Dead files deleted (29 files, ~5K lines removed). Only `pub use vahi_vfs::*` re-export remains.
- **net/** ✅ Dead files deleted (6 files, ~2K lines removed). Only `pub use vahi_net::*` re-export remains.
- **objects/** ⚠️ Partial — kernel's Credentials has extra fields (fsuid/fsgid/cap_effective). TYPE_* constants already deduplicated (use crate's).
- **task/** ⚠️ Kernel owns Process/Thread (CURRENT_PROCESS, signal handling, real fd types). Full dedup attempted 2026-09-03 and rejected: the crate's Process transitively references vahi-syscalls security stubs (landlock/seccomp/namespaces/signal/ptrace: 12–97 lines vs kernel's 417–682) and a minimal task-local VfsNode trait, so re-exporting it breaks the kernel's real security + VFS code. Kernel copies are reconstructed twins of the crate with kernel paths; PtyPair + FORK_CHILD_* live in vahi-task. See CLAUDE.md for the full analysis.

# Architecture Deepening Review

**Date:** 2026-08-23
**Scope:** Full kernel codebase (55,761 lines, 260 files)
**Method:** Deletion test + friction analysis + locality/leverage evaluation
**Vocabulary:** module, interface, depth, seam, adapter, leverage, locality (per codebase-design skill)

---

## Executive Summary

Scanned the entire Vahi Kernel codebase for **deepening opportunities**: refactors that turn shallow modules into deep ones. Each candidate was evaluated with the **deletion test** — would deleting this module concentrate complexity or just move it?

**5 candidates identified, ranked by leverage:**

| # | Candidate | Lines | Severity | Recommendation |
|---|-----------|-------|----------|----------------|
| 1 | Process god object | 886 | Strong | Extract 7 sub-structs |
| 2 | interrupts.rs monolith | 979 | Worth exploring | Decompose into 6 files |
| 3 | objects/ shallow abstraction | 1,082 | Speculative | Deepen or prune |
| 4 | Syscall dispatch match sprawl | 490 | Worth exploring | Split init from dispatch |
| 5 | VFS mod.rs mixed concerns | 700 | Worth exploring | Split traits/mount/init |

**Top recommendation:** Start with #1 (Process). It touches 10+ modules — any improvement cascades everywhere.

---
## Candidate 1: Process — The God Object

**Rating:** Strong (highest leverage in the codebase)

### Files Involved

| File | Lines | Role |
|------|-------|------|
| task/process.rs | 886 | Process struct definition, VMA, fork/execve helpers |
| task/process.rs (Process struct) | 35+ fields | Gravitational center |
| Consumed by: dispatch.rs, process_lifecycle.rs, process_creds.rs, process_signal.rs, scheduler.rs, emulation.rs, pipe.rs, unix.rs, ipc/mod.rs| — | 10+ dependent modules |

### Deletion Test

Deleting Process would **concentrate** complexity everywhere. Every subsystem accesses 3–8 of its fields. The struct is the gravitational center of the kernel — it is the one type that ties together every subsystem.

### Problem

Process is a 35+ field god object where every subsystem injects its own state as independent Mutex<T> fields:



This makes the struct a **shallow module**: its interface (35 public fields) is nearly as complex as its implementation (a bag of data). There is no **locality** — to understand signal handling, you must read signal.rs, process.rs, process_signal.rs, and emulation.rs.

### Solution

Extract 7 subsystem-owned sub-structs from Process:



### Benefits

| Dimension | Before | After |
|-----------|--------|-------|
| **Locality** | Signal code spans 4 files, accessing scattered fields | SignalState + process_signal.rs form a self-contained unit |
| **Leverage** | fork/exec must lock 8+ separate fields | fork clones FsState and Credentials as atomic units |
| **Testability** | Cannot unit-test signal handling without full Process | SignalState is independently constructable and testable |
| **Lock granularity** | Per-field Mutex means 8 lock acquisitions per fork | Sub-struct Mutex: 7 lock acquisitions, but with atomic clones |

### Implementation Strategy

1. Define sub-structs in task/process_state.rs (new file)
2. Add sub-struct fields to Process (one Mutex<FsState> replaces 4 separate fields)
3. Update all process.field.lock() sites to process.fs.lock().field(mechanical)
4. Verify build + self-test passes after each sub-struct extraction
5. Update CONTEXT.md with new domain terms

### ADR Conflict

No existing ADR directly forbids this. ADR-010 (syscalls decomposition) is already done. ADR-019 (kernel object model redesign) proposes changes to Process identity fields but does not conflict with sub-struct extraction.

---
## Candidate 2: interrupts.rs - Interrupt Handler Monolith

**Rating:** Worth exploring

### Files Involved

| File | Lines | Role |
|------|-------|------|
|  | 979 | IDT setup, all exception/IRQ handlers, IPI dispatch, panic |

### Deletion Test

Deleting this file would **scatter** handlers across the codebase. But it is doing too much - the file is a catch-all. Splitting IDT setup from handlers concentrates; splitting handler code distributes.

### Problem

 handles **10+ distinct concerns** in 979 lines: IDT initialization, PIC configuration, timer tick counting, keyboard scancode processing, mouse packet decoding, page fault resolution, GP/DF/UD fault handling, IPI dispatch for TLB shootdown, and the panic handler.

The keyboard handler interleaves with , the page fault handler interleaves with , and the timer handler interleaves with .

### Solution

Decompose by handler type into 6 focused files:

| New File | Content | Est. Lines |
|----------|---------|-----------|
|  | IDT setup, PIC init, entry point registration | ~150 |
|  | PF, GP, DF, UD, NM, SS, DB, BP handlers | ~250 |
|  | LAPIC timer handler, tick counting | ~100 |
|  | Scancode to KeyEvent processing | ~80 |
|  | PS/2 packet decode | ~60 |
|  | TLB flush, function pointer dispatch | ~100 |

Keep  as a thin facade.

### Benefits

- **Locality:** Page fault handler lives with page fault logic, not mixed with keyboard handling
- **Testability:** Keyboard scancode to KeyEvent can be unit-tested in isolation
- **Readability:** 6 files of 100-250 lines each vs one 979-line file

---
## Candidate 3: objects/ - Shallow Kernel Object Abstraction

**Rating:** Speculative

### Files Involved

| File | Lines | Role |
|------|-------|------|
| objects/mod.rs | 119 | ObjectTypeId, ObjectHeader, KernelObject trait |
| objects/handle.rs | 203 | HandleTable with dup/close/lookup |
| objects/namespace.rs | 144 | ObjectNamespace for named objects |
| objects/security.rs | 187 | SecurityDescriptor, ACL |
| objects/syscalls.rs | 149 | OBJMGR syscalls |
| 6 integration files | 11-79 each | Thin wrappers (gui, fs, proc, net, thread, window) |

**Total:** 12 files, 1,082 lines. 8 files under 80 lines.

### Deletion Test

Deleting objects/ would **not concentrate** complexity - the module is already thin. The integration files are 11-79 line wrappers that barely use the KernelObject trait. The module promises a unified object model but does not deliver depth.

### Problem

The objects/ module defines a KernelObject trait and ObjectHeader with ref-counting - a good idea. But 8 of 12 files are under 80 lines. Most subsystems (VFS, process, networking) bypass it entirely. It is an **identity wrapper** - adds indirection without buying clarity.

### Solution

**Option A (Deepen):** Make KernelObject the canonical handle for all resources. Route VFS, process, and socket operations through the handle table. See ADR-019.

**Option B (Prune):** Delete thin wrappers. Keep handle.rs and namespace.rs only. Let subsystems manage their own types directly.

### Benefits (if deepened)

- **Leverage:** Capability-based security becomes real, not just ObjectHeader decoration
- **Locality:** Security checks live on the object, not scattered in each syscall handler

---

## Candidate 4: Syscall Dispatch - Match Arm Sprawl

**Rating:** Worth exploring

### Files Involved

| File | Lines | Role |
|------|-------|------|
| syscalls/dispatch.rs | 490 | MSR init, per-CPU setup, 90+ match arms |

### Deletion Test

Deleting the dispatch would **concentrate** complexity - every handler needs a single entry point. But the current dispatch is a single giant match with no structure.

### Problem

The 90+ arm match is flat and unstructured. The file mixes concerns: MSRs init, per-CPU data, GS base init, and handler dispatch.

### Solution

Split into:
- syscalls/init.rs - MSRs, GS base, per-CPU data setup
- syscalls/dispatch.rs - Just the match, with subsystem-grouped comments

Keep handlers in current submodules.

---

## Candidate 5: VFS - Trait + Manager + Path Resolution in One File

**Rating:** Worth exploring

### Files Involved

| File | Lines | Role |
|------|-------|------|
| vfs/mod.rs | 700 | VfsNode trait, FileSystem trait, VFS manager, mount table, path resolution, init |

### Deletion Test

Deleting vfs/mod.rs would **concentrate** complexity - it is the backbone of the filesystem layer.

### Problem

vfs/mod.rs mixes three concerns: (1) trait definitions, (2) VFS manager with mount table and path resolution (400+ lines), and (3) initialization with auto-mount logic.

### Solution

Split into: vfs/traits.rs (VfsNode + FileSystem + Stat), vfs/mount.rs (mount table + path resolution), vfs/init.rs (auto-mount + device scanning). Keep mod.rs as thin re-export facade.

---

## Top Recommendation

**Start with Candidate 1: Process God Object.**

| Criterion | Assessment |
|-----------|------------|
| Gravitational center | Process is touched by 10+ modules - improvement cascades everywhere |
| Clear decomposition path | 35+ fields cluster naturally into 7 subsystem groups |
| Low risk | Extracting sub-structs is mechanical - no behavior change needed |
| Immediate testability | Each sub-struct can be unit-tested independently |
| No ADR conflicts | No existing ADR forbids this approach |

**Second priority:** Candidate 2 (interrupts.rs) - largest file, straightforward decomposition.

---

## Implementation Sequence

1. **Process sub-structs** (Candidate 1) - highest leverage, lowest risk
2. **interrupts.rs decomposition** (Candidate 2) - largest file, mechanical split
3. **vfs/mod.rs split** (Candidate 5) - clean separation of stable traits from growing logic
4. **syscall dispatch split** (Candidate 4) - small cleanup, reduces cognitive load
5. **objects/ decision** (Candidate 3) - depends on whether ADR-019 is adopted

Each step should be a separate commit with self-test verification.

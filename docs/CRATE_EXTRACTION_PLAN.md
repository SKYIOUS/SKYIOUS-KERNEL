# Crate Extraction Plan

**Purpose:** Extract remaining kernel modules into standalone `no_std` crates so multiple agents can work simultaneously without file conflicts.

**Status:** 5 crates extracted (sync, crypto, hal, limine, memory-frame_info), 9 scaffolded.

## Current State

| Crate | Lines | Status | Dependencies |
|-------|-------|--------|-------------|
| `vahi-sync` | 90 | ✅ Extracted | `spin` |
| `vahi-crypto` | 308 | ✅ Extracted | `alloc` only |
| `vahi-hal` | 172 | ✅ Extracted | `vahi-sync` |
| `vahi-limine` | 140 | ✅ Extracted | `limine` crate |
| `vahi-memory` | 189 | Partial | `vahi-sync`, `x86_64` |
| `vahi-arch` | 18 | Scaffold | — |
| `vahi-boot` | 27 | Scaffold | — |
| `vahi-interrupts` | 27 | Scaffold | — |
| `vahi-task` | 28 | Scaffold | — |
| `vahi-vfs` | 29 | Scaffold | — |
| `vahi-net` | 26 | Scaffold | — |
| `vahi-drivers` | 29 | Scaffold | — |

## Dependency Analysis

### Actual `crate::` Dependencies (from source)

```
task   → gdt, memory, objects, sync, vfs
memory → sync, task
interrupts → memory, task
boot   → arch, interrupts, memory, sync, task
vfs    → drivers, interrupts, objects, sync, syscalls, task
net    → drivers, objects, sync, syscalls
drivers → interrupts, sync
```

### Cycles That Must Be Broken

| Cycle | Through | Break Strategy |
|-------|---------|----------------|
| `task ↔ memory` | `AddressSpace`, `CURRENT_PROCESS` | Extract shared types into `vahi-types` |
| `memory ↔ interrupts` | Page fault handler callback | Trait-based callback in `vahi-types` |
| `task ↔ vfs` | `FileDescriptor` table | Trait-based file handle in `vahi-types` |

### Module Sizes

| Module | Files | Lines | Extraction Difficulty |
|--------|-------|-------|----------------------|
| `task` | 11 | 3,952 | Hard (cycle with memory) |
| `memory` | 12 | 2,454 | Hard (cycle with interrupts) |
| `interrupts` | 5 | 1,053 | Medium |
| `boot` | 5 | 587 | Easy |
| `vfs` | 21 | 6,774 | Hard (largest, many deps) |
| `net` | 7 | 1,927 | Medium |
| `drivers` | 36 | 8,266 | Hard (largest by file count) |

## Extraction Phases

### Phase 0: Foundation (break cycles)

**Create `vahi-types` crate** — shared types and traits that break circular dependencies.

```rust
// crates/types/src/lib.rs
#![no_std]

extern crate alloc;

use alloc::sync::Arc;

/// Process identifier
pub type Pid = u64;

/// File descriptor table trait — implemented by task, consumed by vfs
pub trait FdTable: Send + Sync {
    fn get_file(&self, fd: u32) -> Option<Arc<dyn FileHandle>>;
    fn insert_file(&self, file: Arc<dyn FileHandle>) -> Result<u32, ()>;
}

/// File handle trait — implemented by vfs, consumed by syscalls
pub trait FileHandle: Send + Sync {
    fn read(&self, buf: &mut [u8], offset: u64) -> Result<usize, ()>;
    fn write(&self, buf: &[u8], offset: u64) -> Result<usize, ()>;
    fn stat(&self) -> Result<FileStat, ()>;
}

/// Page fault callback — implemented by task/memory, consumed by interrupts
pub trait PageFaultHandler: Send + Sync {
    fn handle_page_fault(&self, addr: u64, error_code: u64) -> bool;
}

/// Address space trait — breaks task ↔ memory cycle
pub trait AddressSpaceOps: Send + Sync {
    fn map_page(&self, virt: u64, phys: u64, flags: u64) -> Result<(), ()>;
    fn unmap_page(&self, virt: u64) -> Result<(), ()>;
}
```

**Files to create:**
- `crates/types/Cargo.toml`
- `crates/types/src/lib.rs`

**Files to modify:**
- `Cargo.toml` (add to workspace)
- `kernel/Cargo.toml` (add dependency)
- `kernel/src/types.rs` (re-export from `vahi_types`)

**Effort:** ~1 hour
**Risk:** Low (additive, no existing code changes)

---

### Phase 1: Leaf Modules (no cycles)

These modules have no bidirectional dependencies and can be extracted independently.

#### 1a. `vahi-interrupts` (1,053 lines, 5 files)

**Dependencies:** `vahi-sync`, `vahi-types` (for `PageFaultHandler`)

**Files to move:**
- `interrupts/mod.rs` → `crates/interrupts/src/lib.rs`
- `interrupts/irq.rs` → `crates/interrupts/src/irq.rs`
- `interrupts/exceptions.rs` → `crates/interrupts/src/exceptions.rs`
- `interrupts/page_fault.rs` → `crates/interrupts/src/page_fault.rs`
- `interrupts/diag.rs` → `crates/interrupts/src/diag.rs`

**Interface contract:**
```rust
pub trait InterruptController: Send + Sync {
    fn eoi(&self, vector: u8);
    fn mask_irq(&self, irq: u8, masked: bool);
}

pub fn init_idt();
pub fn get_ticks() -> u64;
pub fn register_page_fault_handler(handler: &'static dyn PageFaultHandler);
```

**Effort:** ~2 hours
**Risk:** Medium (page fault handler needs trait-based callback)

#### 1b. `vahi-boot` (587 lines, 5 files)

**Dependencies:** `vahi-sync`, `vahi-types` (minimal)

**Files to move:**
- `boot/mod.rs` → `crates/boot/src/lib.rs`
- `boot/state.rs` → `crates/boot/src/state.rs`
- `boot/init.rs` → `crates/boot/src/init.rs`
- `boot/logger.rs` → `crates/boot/src/logger.rs`
- `boot/tasks.rs` → `crates/boot/src/tasks.rs`

**Interface contract:**
```rust
pub struct BootContext { /* ... */ }
pub fn run_boot() -> Result<(), &'static str>;
pub fn store_trace(trace: Vec<String>, paths: Vec<String>);
pub fn with_trace<F, R>(f: F) -> R where F: FnOnce(&[String], &[String]) -> R;
```

**Effort:** ~1.5 hours
**Risk:** Low (self-contained boot sequence)

#### 1c. `vahi-net` (1,927 lines, 7 files)

**Dependencies:** `vahi-sync`, `vahi-types` (for `FileHandle`)

**Files to move:**
- `net/mod.rs` → `crates/net/src/lib.rs`
- `net/tcp_congestion.rs` → `crates/net/src/tcp_congestion.rs`
- `net/dns.rs` → `crates/net/src/dns.rs`
- `net/dhcp.rs` → `crates/net/src/dhcp.rs`

**Interface contract:**
```rust
pub fn init();
pub fn poll();
pub fn sockets() -> &'static SOCKETS_TYPE;
```

**Effort:** ~2 hours
**Risk:** Medium (smoltcp dependency management)

---

### Phase 2: Tightly Coupled Group (task + memory)

These modules form a bidirectional dependency cycle and must be extracted together.

#### Combined `vahi-task` + `vahi-memory` (6,406 lines, 23 files)

**Strategy:** Extract as two crates with shared trait definitions in `vahi-types`.

**`vahi-memory` files:**
- `memory/mod.rs` → `crates/memory/src/lib.rs`
- `memory/buddy.rs` → `crates/memory/src/buddy.rs`
- `memory/phys.rs` → `crates/memory/src/phys.rs`
- `memory/virt.rs` → `crates/memory/src/virt.rs`
- `memory/slab.rs` → `crates/memory/src/slab.rs`
- `memory/paging.rs` → `crates/memory/src/paging.rs`
- `memory/swap.rs` → `crates/memory/src/swap.rs`
- `memory/stack.rs` → `crates/memory/src/stack.rs`
- `memory/isolate.rs` → `crates/memory/src/isolate.rs`
- `memory/overcommit.rs` → `crates/memory/src/overcommit.rs`
- `memory/aarch64.rs` → `crates/memory/src/aarch64.rs`
- `memory/frame_info.rs` → already extracted

**`vahi-task` files:**
- `task/mod.rs` → `crates/task/src/lib.rs`
- `task/process.rs` → `crates/task/src/process.rs`
- `task/process/types.rs` → `crates/task/src/process/types.rs`
- `task/thread.rs` → `crates/task/src/thread.rs`
- `task/scheduler/` → `crates/task/src/scheduler/`
- `task/oom.rs` → `crates/task/src/oom.rs`
- `task/lock.rs` → `crates/task/src/lock.rs`
- `task/executor.rs` → `crates/task/src/executor.rs`

**Cycle-breaking interface:**
```rust
// In vahi-types:
pub trait ProcessProvider: Send + Sync {
    fn current_pid(&self) -> Pid;
    fn current_process(&self) -> Option<Arc<dyn ProcessOps>>;
}

pub trait ProcessOps: Send + Sync {
    fn address_space(&self) -> Arc<dyn AddressSpaceOps>;
    fn fd_table(&self) -> Arc<dyn FdTable>;
}
```

**Effort:** ~6 hours (largest extraction)
**Risk:** High (bidirectional dependencies, scheduler complexity)

---

### Phase 3: Large Modules

#### 3a. `vahi-vfs` (6,774 lines, 21 files)

**Dependencies:** `vahi-sync`, `vahi-types`, `vahi-task` (for `FileHandle`)

**Files to move (in order):**
1. `vfs/mod.rs` → core VFS types
2. `vfs/vfs.rs` → VFS manager
3. `vfs/node.rs` → VfsNode trait
4. `vfs/ramfs.rs` → RAM filesystem
5. `vfs/devfs.rs` → Device filesystem
6. `vfs/tmpfs.rs` → Temporary filesystem
7. `vfs/tarfs.rs` → TAR filesystem
8. `vfs/ext2/` → ext2 filesystem
9. `vfs/ext4/` → ext4 filesystem
10. `vfs/fat32/` → FAT32 filesystem
11. `vfs/skyfs/` → SkyFS journaling

**Interface contract:**
```rust
pub trait VfsNode: Send + Sync {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, ()>;
    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, ()>;
    fn stat(&self) -> Result<Stat, ()>;
    fn lookup(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()>;
}

pub struct VfsManager { /* ... */ }
pub fn init();
```

**Effort:** ~4 hours
**Risk:** Medium (well-defined interface, but many filesystem implementations)

#### 3b. `vahi-drivers` (8,266 lines, 36 files)

**Dependencies:** `vahi-sync`, `vahi-hal`, `vahi-memory`

**Files to move (by subsystem):**
1. `drivers/serial/` → Serial console
2. `drivers/keyboard.rs` → PS/2 keyboard
3. `drivers/mouse.rs` → PS/2 mouse
4. `drivers/block/` → Block device layer
5. `drivers/net/` → Network drivers (E1000, VirtIO-net)
6. `drivers/usb/` → USB stack
7. `drivers/graphics/` → GPU/framebuffer
8. `drivers/audio/` → HDA audio
9. `drivers/storage/nvme.rs` → NVMe driver

**Interface contract:**
```rust
pub trait BlockDevice: Send + Sync {
    fn read_blocks(&self, start: u64, count: u64, buf: &mut [u8]) -> Result<(), ()>;
    fn write_blocks(&self, start: u64, count: u64, buf: &[u8]) -> Result<(), ()>;
    fn block_size(&self) -> usize;
}

pub trait NetworkDevice: Send + Sync {
    fn send(&self, packet: &[u8]) -> Result<(), ()>;
    fn recv(&self, buf: &mut [u8]) -> Result<usize, ()>;
}
```

**Effort:** ~5 hours
**Risk:** Medium (driver complexity, but well-isolated subsystems)

---

### Phase 4: Remaining

#### 4a. `vahi-arch` (995 lines, 4 files)

**Dependencies:** `vahi-sync`, `vahi-task` (for thread context switch)

**Files to move:**
- `arch/mod.rs`
- `arch/arch_x86_64.rs`
- `arch/arch_aarch64.rs`
- `arch/arch_riscv64.rs`

**Effort:** ~2 hours
**Risk:** Low (architecture-specific, well-guarded with `#[cfg]`)

#### 4b. `vahi-acpi` (standalone)

**Dependencies:** `vahi-sync`, `vahi-memory`

**Effort:** ~1 hour
**Risk:** Low

#### 4c. `vahi-gdt` (standalone)

**Dependencies:** `vahi-sync`, `vahi-task`

**Effort:** ~1 hour
**Risk:** Low

---

## Extraction Order (Recommended)

```
Phase 0: vahi-types          (~1 hour)   ← breaks all cycles
Phase 1a: vahi-interrupts    (~2 hours)  ← leaf module
Phase 1b: vahi-boot          (~1.5 hours) ← leaf module
Phase 1c: vahi-net           (~2 hours)  ← leaf module
Phase 2: vahi-task + memory  (~6 hours)  ← tightly coupled pair
Phase 3a: vahi-vfs           (~4 hours)  ← large, many files
Phase 3b: vahi-drivers       (~5 hours)  ← largest by file count
Phase 4a: vahi-arch          (~2 hours)  ← architecture-specific
Phase 4b: vahi-acpi          (~1 hour)   ← standalone
Phase 4c: vahi-gdt           (~1 hour)   ← standalone
─────────────────────────────────────────
Total:                       ~25.5 hours
```

## Verification Checklist

After each phase:

- [ ] `cargo build --release --target x86_64-unknown-none -p <crate>` passes
- [ ] `cargo clippy -p <crate> -- -D warnings` passes
- [ ] `cargo build --release --target x86_64-unknown-none -p vahi_kernel` passes
- [ ] Boot test: login prompt reached
- [ ] Selftests: 131/131 pass
- [ ] All existing imports work via re-exports (zero churn)

## Agent Assignment

| Agent | Phase | Crate(s) | Estimated Time |
|-------|-------|----------|---------------|
| Agent A | 0 | `vahi-types` | 1 hour |
| Agent B | 1a | `vahi-interrupts` | 2 hours |
| Agent C | 1b | `vahi-boot` | 1.5 hours |
| Agent D | 1c | `vahi-net` | 2 hours |
| Agent E+F | 2 | `vahi-task` + `vahi-memory` | 6 hours (pair) |
| Agent G | 3a | `vahi-vfs` | 4 hours |
| Agent H | 3b | `vahi-drivers` | 5 hours |
| Agent I | 4a-c | `vahi-arch` + `vahi-acpi` + `vahi-gdt` | 4 hours |

**Parallelization:** Phases 0 must complete first. Phases 1a, 1b, 1c can run in parallel. Phase 2 requires Phase 0. Phases 3a, 3b require Phase 2. Phase 4 can run anytime after Phase 0.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Cycle break introduces trait object overhead | Performance | Profile before/after; use static dispatch where possible |
| Re-exports break import paths | Build failures | Test each phase with full build |
| Scheduler complexity in vahi-task | Extraction time | Extract scheduler last within the task crate |
| smoltcp version conflicts in vahi-net | Build failures | Pin smoltcp version in vahi-net's Cargo.toml |
| Driver DMA requirements | Memory coupling | Keep DMA allocators in vahi-memory, expose via trait |

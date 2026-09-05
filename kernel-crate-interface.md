# Kernel ↔ Crate Interface

This document describes how the kernel (`vahi_kernel`) interacts with the
22 extracted crates. It exists so that multiple agents can work on different
crates simultaneously without creating incompatible changes.

## 1. Stub / Override Pattern

Several crates define stub functions that return dummy values. Two
mechanisms are in use:

1. **Mangled-name stubs** (e.g. `get_ticks`, crate `serial_write`): the crate
defines `pub fn get_ticks() -> u64 { 0 }`; the kernel defines its own
`get_ticks` with a different mangled name. **Calls inside the crate resolve
to the crate's own stub — these are NOT overridden.** Anything that needs a
real value must call a kernel-owned function instead.
2. **Unmangled kernel-provided symbols** (e.g. `vfs_serial_write`,
`vfs_serial_putc`): the crate declares the kernel's `#[no_mangle]` symbol
`extern "Rust"` and wraps it in a safe fn; the kernel owns the single
definition in `kernel/src/main.rs`. Defining the same `#[no_mangle]`
symbol on BOTH sides fails the link with `duplicate symbol` — never do that.

**This is the most dangerous pattern in the codebase.** If a crate adds a
new call to a stub the kernel doesn't provide, the crate silently gets the
dummy value (e.g., `get_ticks() → 0`) or fails to link.

### Stub Registry

| Stub Function | Crates Defining It | Kernel Override Location | Notes |
|---|---|---|---|
| `get_ticks() → u64` | `vahi-apic`, `vahi-drivers`, `vahi-net`, `vahi-task`, `vahi-vfs` (safe wrapper + `extern "Rust"` decl of `vahi_kernel_get_ticks`) | `kernel/src/interrupts/mod.rs` (`#[no_mangle] fn vahi_kernel_get_ticks`) | 100Hz timer-ISR counter, single owner. Fixed 2026-09-03: the crate stubs were never overridden (mangled names don't collide — the old “overridden at link time” claim was false) and silently returned 0. `vahi-hal`'s clock is separate and real: `timer::current_time_us()` (TSC microseconds). |
| `serial_write(msg: &str)` | `vahi-drivers`, `vahi-interrupts`, `vahi-memory`, `vahi-task` | `kernel/src/main.rs:272` | Writes to QEMU serial port. Only the kernel's version produces output. |
| `vfs_serial_write(msg: &str)` | `vahi-vfs` (safe wrapper + `extern "Rust"` decl of `vahi_kernel_serial_write`) | `kernel/src/main.rs` (`#[no_mangle] fn vahi_kernel_serial_write`) | VFS debug output (ext4 self-test, tarfs, fuse). The crate defines NO `#[no_mangle]` body — it declares the kernel symbol `extern` and wraps it. A both-sides `#[no_mangle]` definition fails the link (`duplicate symbol`). |
| `vfs_serial_putc(c: u8)` | `vahi-vfs` (safe wrapper + `extern "Rust"` decl of `vahi_kernel_serial_putc`) | `kernel/src/main.rs` (`#[no_mangle] fn vahi_kernel_serial_putc`) | Per-char sink for `/dev/tty0` — carries userspace stdout/stderr (fd 1/2) to serial. Fixed 2026-09-03: previously the crate's no-op stub was linked (no override existed), so all userspace console output was silently discarded. |

### Danger: New Stubs

When adding a new crate function that depends on kernel state (time, serial
output, process context), you must:

1. Define a stub in the crate that returns a safe default
2. Document the stub in this file
3. Add an override in the kernel with `#[no_mangle]`
4. Verify the override is actually linked (check symbol table if uncertain)

### Danger: Stub Divergence

If two crates define the same stub name with different signatures, the
linker will error. If they define it with the same signature but different
return types (e.g., one returns `u64`, another returns `Option<u64>`),
the behavior is undefined.

**Current status:** All `get_ticks()` stubs return `u64` and are compatible.
The kernel's override in `hal/timer.rs` returns the real tick count.

## 2. Canonical Type Ownership

Some types exist in both the kernel and crates with identical (or near-
identical) definitions. This is the **primary design debt** in the codebase.

### Types Owned by Crates (Canonical)

| Type | Crate | Kernel Re-export |
|---|---|---|
| `Task`, `TaskId`, `YieldNow` | `vahi-task` | `kernel/src/task/mod.rs:3` |
| `FORK_CHILD_CS`, `FORK_CHILD_SS` | `vahi-task::thread` | `kernel/src/task/thread.rs:89-90` |
| `ObjectTypeId`, `ObjectHeader`, `KernelObject` | `vahi-objects` | `kernel/src/objects/mod.rs` |
| `Signal` | `vahi-syscalls::signal` | `kernel/src/syscalls/signal.rs` |
| `SeccompMode` | `vahi-syscalls::seccomp` | `kernel/src/syscalls/seccomp.rs` |
| `PtraceStop` | `vahi-syscalls::ptrace` | `kernel/src/syscalls/ptrace.rs` |
| `sha256`, `hmac_sha256`, `pbkdf2`, `GLOBAL_ENTROPY` | `vahi-crypto` | `kernel/src/crypto/mod.rs` |
| `IrqSafeMutex`, `IrqSafeMutexGuard` | `vahi-sync` | `kernel/src/sync/mod.rs` |
| Everything in `vahi-vfs` | `vahi-vfs` | `kernel/src/vfs/mod.rs:1` (`pub use vahi_vfs::*`) |
| Everything in `vahi-net` | `vahi-net` | `kernel/src/net/mod.rs:1` (`pub use vahi_net::*`) |
| Everything in `vahi-drivers` | `vahi-drivers` | `kernel/src/drivers/mod.rs:1` (`pub use vahi_drivers::*`) |
| Everything in `vahi-ipc` | `vahi-ipc` | `kernel/src/ipc/mod.rs:4` (`pub use vahi_ipc::*`) |
| Everything in `vahi-memory` | `vahi-memory` | `kernel/src/memory/mod.rs:1` (`pub use vahi_memory::*`) |
| Everything in `vahi-pci` | `vahi-pci` | `kernel/src/pci/mod.rs:6` (`pub use vahi_pci::*`) |
| Everything in `vahi-limine` | `vahi-limine` | `kernel/src/limine.rs:3` (`pub use vahi_limine::*`) |
| Everything in `vahi-ebpf` | `vahi-ebpf` | `kernel/src/ebpf/mod.rs:7` (`pub use vahi_ebpf::*`) |
| Everything in `vahi-hal` | `vahi-hal` | `kernel/src/hal/mod.rs:6-8` (selective re-exports) |

### Types Duplicated (Kernel Has Its Own Copy)

These types exist in both the kernel and a crate with **identical or near-
identical definitions**. The kernel's copies are what the rest of the kernel
uses. The crate copies are unused or used only within the crate.

| Type | Kernel Location | Crate Location | Lines | Status |
|---|---|---|---|---|
| `Process` | `kernel/src/task/process.rs:141` | `crates/task/src/process.rs:223` | 1034 vs 1149 | **Kernel copy is canonical** — kernel imports its own `crate::task::process::Process` everywhere |
| `Thread` | `kernel/src/task/thread.rs:104` | `crates/task/src/thread.rs:139` | 681 vs 814 | **Kernel copy is canonical** |
| `AddressSpace` | `kernel/src/memory/paging.rs:47` | `crates/memory/src/paging.rs:52` | — | **Kernel copy is canonical** |
| All drivers (15 files) | `kernel/src/drivers/` | `crates/drivers/src/` | 8235 vs 8516 | **Both exist** — kernel uses its own copies |
| `ext4.rs` | `kernel/src/vfs/ext4.rs` | `crates/vfs/src/ext4.rs` | 667 vs 667 | **Near-identical** — differs only by import paths (`crate::` vs `vahi_::`) |
| `unix.rs` | `kernel/src/net/unix.rs` | `crates/net/src/unix.rs` | 491 vs ~491 | Import-path difference only |

**This duplication means:** A bug fix in `crates/task/src/process.rs` does
NOT fix the bug in `kernel/src/task/process.rs`. The kernel's copies are
what actually runs.

### Resolution Path

The correct fix is to make crates the single source of truth:

1. Delete kernel's duplicate files (`kernel/src/drivers/`, `kernel/src/vfs/ext4.rs`)
2. Update kernel imports from `crate::task::process::Process` to `vahi_task::process::Process`
3. For types that the kernel extends (Process, Thread), either:
   - Move the kernel-specific fields into the crate type, or
   - Have the kernel wrap the crate type in its own struct

## 3. Feature Flags

### Kernel Features (kernel/Cargo.toml)

| Feature | Default | Description |
|---|---|---|
| `smp` | ✅ | Symmetric multiprocessing (per-CPU scheduling, IPI) |
| `net` | ✅ | Networking stack (TCP/UDP, DHCP, DNS) |
| `ext4` | ✅ | ext4 read-only filesystem support |
| `verification` | ❌ | Formal verification hooks (journal state machine) |
| `uhci` | ❌ | USB 1.x UHCI host controller |
| `self_test` | ❌ | In-kernel selftest suite (runs after boot) |
| `ash` | ❌ | Ash shell integration |
| `gpu` | ❌ | VirtIO-GPU and GUI compositor |
| `ebpf` | ❌ | eBPF virtual machine and JIT |
| `hypervisor` | ❌ | KVM-style hypervisor (VMX/SVM/EPT) |

### Crate Feature Flags

| Crate | Features | Default |
|---|---|---|
| `vahi-task` | `smp` | `smp` |
| `vahi-memory` | `smp` | — |
| `vahi-vfs` | `ext4`, `net`, `verification` | `ext4` |
| `vahi-syscalls` | `self_test` | — |
| `vahi-drivers` | `net`, `gpu`, `uhci`, `smp`, `ebpf` | — |
| `vahi-gui` | `gpu` | `gpu` |
| `vahi-ebpf` | `jit` | — |
| `vahi-interrupts` | `net` | — |
| `vahi-acpi` | `aml` (optional) | — |
| `vahi-objects` | — | — |
| `vahi-types` | `x86_64` (optional) | — |

### Feature Propagation

The kernel enables features on crates via Cargo.toml dependencies.
When the kernel enables `net`, it propagates to `vahi-vfs` (enables
network filesystem support), `vahi-drivers` (enables network drivers),
and `vahi-interrupts` (enables network interrupt handling).

**Important:** The kernel's `default` features (`smp`, `net`, `ext4`)
are the production configuration. CI builds with `--all-features` to
test everything.

## 4. Module Map

The kernel's source tree mirrors the crate structure:

```
kernel/src/
├── arch/           ← vahi-arch + kernel-specific (syscall entry, context switch)
├── boot/           ← vahi-boot + kernel-specific (init process, shell)
├── drivers/        ← DUPLICATE of vahi-drivers (should be deleted)
├── ebpf/           ← re-exports vahi-ebpf
├── gui/            ← kernel-specific GUI init (vahi-gui provides rendering)
├── hal/            ← re-exports vahi-hal (irq, timer, platform)
├── interrupts/     ← DUPLICATE of vahi-interrupts (should be deleted)
├── memory/         ← re-exports vahi-memory + kernel-specific frame_info
├── net/            ← DUPLICATE of vahi-net (should be deleted)
├── objects/        ← re-exports vahi-objects + kernel-specific integration
├── shell/          ← kernel-specific (debug commands, built-in utilities)
├── sync/           ← re-exports vahi-sync
├── syscalls/       ← kernel-specific (syscall dispatch, implementations)
├── task/           ← DUPLICATE of vahi-task (should be deleted)
├── vfs/            ← DUPLICATE of vahi-vfs (should be deleted)
└── main.rs         ← kernel entry point, panic handler, serial_write override
```

## 5. Rules for Parallel Agents

1. **Never add a new stub function** without documenting it here and adding
   a kernel override.

2. **Never define a type in a crate that also exists in the kernel** without
   either deleting the kernel copy or documenting why both exist.

3. **When modifying a duplicated file** (e.g., `crates/drivers/src/net/e1000.rs`),
   apply the same change to `kernel/src/drivers/net/e1000.rs` until the
   duplication is resolved.

4. **Feature flags must be consistent.** If a crate adds a new feature that
   the kernel should enable, add it to `kernel/Cargo.toml`'s `[features]`
   section and the relevant dependency.

5. **All crate public APIs must have rustdoc.** The crate-level doc comment
   must list exports, dependencies, and invariants.

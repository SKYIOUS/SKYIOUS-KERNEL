# Vahi Kernel — Audit Report

**Date:** 2026-11-19  
**Scope:** Entire repository (kernel + 17 workspace crates + builder + docs)  
**Assumed load:** QEMU x86_64 only, SMP 1-2, single-user boot-to-login. Real hardware never tested.

---

## What this repo does

Vahi is a `#![no_std]` monolithic Rust kernel (x86_64 primary, aarch64/riscv64 scaffolded) that boots via Limine/UEFI on QEMU, reaches a login prompt, and runs ~130 dispatched syscalls (~40 fully functional). It includes a stride scheduler, buddy/slab memory, 9 filesystems (2 working), 7 drivers (serial, PS/2, E1000, VirtIO-blk, PC speaker, RTC), a partial GUI compositor, and stub subsystems for hypervisor, eBPF, ASH, io_uring. All development is in QEMU; no real-hardware testing exists.

---

## Findings

### MUST FIX — Bugs, Security, Data Loss, Breaks at Expected Load

#### 1. **KASLR is obfuscation only — relocations never applied** (`kernel/src/kaslr_reloc.rs:338`, `issues.md A-6`)
- **What this is:** KASLR randomizes the kernel page-table mapping at boot but the ELF magic check fails on every build, so `apply_kaslr_relocations` returns early without applying any relocations.
- **Problem:** The kernel boots only because every old address remains aliased (identity-mapped). Absolute pointers in `.rodata`, `.data`, `.got`, `.bss` are never fixed up. The boot log prints "Relocations applied" unconditionally, so the failure is invisible.
- **Fix:** Apply relocations properly: emit `.rela` as an alloc section inside a PT_LOAD, add `__rela_start`/`__rela_end` symbols, walk the table at runtime, and only claim success if relocations actually applied. See `issues.md` A-6 for full evidence.
- **If we skip it:** KASLR provides zero ASLR guarantee; an info leak reveals all kernel addresses. The claim "KASLR works" is false.

#### 2. **SMP-4 hangs after all tests pass; SMP-8 hangs in early boot** (`issues.md S-1, S-2`)
- **What this is:** Stress testing revealed two distinct SMP failures on the pristine pre-change image.
- **Problem:** 
  - SMP-8: Last serial output is `[BOOT] syscalls init...` — hangs before any test runs.
  - SMP-4: Reaches `161/161 passed`, then `[LOCKUP] rip=0xffffffff800374d8 stuck 500 ticks` — kernel completes work then wedges in shutdown/idle path.
- **Fix:** Instrument the boot/shutdown path to identify the lockup RIP. S-1 needs early-boot debugging; S-2 needs the watchdog output symbolized (`0xffffffff800374d8`).
- **If we skip it:** SMP >2 is unusable. CI only tests SMP-1/2, so regressions on 4/8 CPUs go undetected.

#### 3. **aarch64 build fails — x86_64-only asm in `vahi-sync`** (`crates/sync/src/lib.rs:205,255`, `issues.md P-1`)
- **What this is:** The `IrqSafeMutex` implementation uses `core::arch::x86_64::__cpuid` and `asm!("pushfq; pop {rflags}; cli", options(att_syntax))` unconditionally.
- **Problem:** aarch64 and riscv64 builds fail immediately with "cannot find x86_64 in arch" and "att_syntax only supported on x86". Cross-arch CI is permanently red.
- **Fix:** Guard the x86_64 asm with `#[cfg(target_arch = "x86_64")]` and provide aarch64/riscv64 implementations (MSR-based or GIC-based IRQ masking).
- **If we skip it:** Multi-arch is blocked. No aarch64/riscv64 progress possible.

#### 4. **ext2 directory traversal unbounded — corrupt rec_len walks past block** (`crates/vfs/src/ext2/directory.rs:45,94`, `issues.md F-1`)
- **What this is:** Directory iteration does `off += e.rec_len` with no bounds check against the block size.
- **Problem:** A hostile or corrupt filesystem image with oversized `rec_len` causes out-of-bounds reads past the directory block, potentially leaking kernel memory or crashing.
- **Fix:** Validate `rec_len` ≤ remaining bytes in block before advancing. Return error on corruption.
- **If we skip it:** Mounting a malicious ext2 image = kernel memory read / crash. Security boundary violated.

#### 5. **SkyFS extent copy unbounded — disk-supplied extent_count * sizeof(Extent) unchecked** (`crates/vfs/src/skyfs/mod.rs:531-535`, `issues.md F-2`)
- **What this is:** `store_data_blocks` multiplies a disk-supplied `extent_count` by `size_of::<Extent>()` without checking against `MAX_INLINE_DATA`, then copies into a fixed-size array.
- **Problem:** A crafted SkyFS image can overflow the stack/heap buffer, corrupting kernel memory.
- **Fix:** Clamp `extent_count` so `extent_count * size_of::<Extent>() ≤ MAX_INLINE_DATA`. Reject images that exceed it.
- **If we skip it:** SkyFS mount = potential kernel memory corruption. Security boundary violated.

#### 6. **NVMe admin and I/O queues share head/tail/phase — commands execute garbage** (`crates/drivers/src/storage/nvme.rs:155-157,399-403,429-433`, `issues.md N-1`)
- **What this is:** Single set of `sq_tail`, `cq_head`, `phase` used for both admin and I/O queue pairs.
- **Problem:** After N admin commands, I/O submit lands at index N, so controller executes unwritten entries. Completion read from wrong slot.
- **Fix:** Give each queue pair its own head/tail/phase state at the `reap` choke point.
- **If we skip it:** NVMe driver executes garbage opcodes, returns corrupted data, or crashes.

#### 7. **NVMe reap discards command status — failed commands reported as success** (`crates/drivers/src/storage/nvme.rs:350-358`, `issues.md N-2`)
- **What this is:** `reap` reads only CQE phase bit, returns `true`, discards `cqe.status` (SCT/SC) and never matches CID.
- **Problem:** A failed command is reported success; `read_sector` copies buffer as valid data, disclosing stale heap when `DmaBuf` is uninitialized.
- **Fix:** In `reap`, check `cqe.status`, match CID, return error on failure.
- **If we skip it:** Silent data corruption — failed NVMe reads return garbage.

#### 8. **AHCI completion wait unbounded — hangs CPU with interrupts disabled** (`crates/drivers/src/storage/ahci.rs:432-444,677-688`, `issues.md A-1`)
- **What this is:** The completion wait loop has no iteration bound.
- **Problem:** A device that never completes hangs the CPU inside a block-device `IrqSafeMutex` — no tick, no watchdog. Preceding `tfd` wait is bounded, so the omission is local.
- **Fix:** Add bounded retry with timeout; on timeout return error instead of spinning forever.
- **If we skip it:** Stuck AHCI device = total kernel hang (no scheduler tick, no watchdog).

#### 9. **AHCI only programs CTBA header 0 — slots 1-31 use physical address 0** (`crates/drivers/src/storage/ahci.rs:271-277,321-329,349`, `issues.md A-2`)
- **What this is:** Only command-table header 0 gets its `CTBA` programmed. Free-slot selection can pick header 1-31, which are zero-filled.
- **Problem:** HBA takes command table from physical address 0. Reachable whenever slot 0 is left busy (e.g., after the `spin > 1000000` bail-out).
- **Fix:** Program CTBA for all 32 slots during port initialization.
- **If we skip it:** AHCI commands on slots 1-31 execute from physical 0 = data corruption or crash.

#### 10. **E1000 IRQ path allocates heap + takes 4 blocking locks in IRQ context** (`kernel/src/interrupts/irq.rs:141-161` → `crates/net/src/lib.rs:282-300` → `crates/drivers/src/net/e1000.rs:508,724`, `issues.md E-1`)
- **What this is:** IRQ handler calls `net::poll()` which takes four `IrqSafeMutex` locks and allocates `vec![0u8; pkt_len]` with interrupts disabled.
- **Problem:** Direct violation of kernel rule #1 (IRQ context safety): no heap allocation, no blocking locks in IRQ context. Deadlock risk.
- **Fix:** Move packet processing to a threaded/softirq context; IRQ handler only acks hardware and schedules work. Pre-allocate DMA buffers.
- **If we skip it:** Network interrupt under load = deadlock or panic.

#### 11. **xHCI IMAN bit 1 written where bit 0 meant — interrupter never enabled** (`crates/drivers/src/usb/xhci/mod.rs:260,705-706`, `issues.md X-2`)
- **What this is:** IMAN IP (bit 1) written instead of IE (bit 0).
- **Problem:** Interrupter never enabled; ack is no-op. Controller runs fully polled (no IRQ handler registered).
- **Fix:** Write IMAN bit 0 (IE) to enable interrupter. Register IRQ handler.
- **If we skip it:** xHCI runs polled only, wasting CPU. No interrupt-driven USB.

#### 12. **`vahi-sync` crate duplicated in kernel — MSI pool never initialized for crate users** (`crates/apic/src/msi.rs:120`, `issues.md A-3`)
- **What this is:** `vahi-pci` and NVMe call `vahi_apic::msi::alloc()` which reads the crate's `POOL` static. Nothing calls `vahi_apic::init()`, so pool is permanently `None`.
- **Problem:** Both devices silently fall back to legacy INTx. No crash, no log — MSI simply never works.
- **Fix:** Wire crate's ACPI provider and call `vahi_apic::init()`, leaving kernel copy in place for now (see `issues.md` A-4 for why swap is not safe deletion).
- **If we skip it:** NVMe/PCIe on real hardware use slow INTx; MSI never works.

#### 13. **Silently-accepted socket options — ~15 options return success while doing nothing** (`kernel/src/syscalls/net_options.rs:36-120`, `issues.md`)
- **What this is:** `SO_LINGER`, `SO_SNDBUF`, `SO_RCVBUF`, `SO_ERROR`, `SO_TYPE`, `SO_BINDTODEVICE` are literal `{ /* comment */ 0u64 }` bodies.
- **Problem:** A program setting `SO_LINGER` before `close()` expects blocking close and gets none. `SO_RCVBUF`/`SO_SNDBUF` sizing ignored entirely.
- **Fix:** Return `ENOPROTOOPT` for unimplemented options (partially fixed 2026-10-21 per `issues.md`; verify all options covered).
- **If we skip it:** Userspace network code behaves incorrectly with no error signal.

#### 14. **Test suite gives false confidence** (`issues.md` "Test suite — false confidence" section)
- **What this is:** 
  - `stress.rs`: 8 of 13 tests exercise local `Vec`/`VecDeque`, not the named subsystem. `alloc_dealloc_churn` and `fragmented_alloc` have no assertions.
  - `fuzzer.rs`: Never invokes a syscall; validates only its own PRNG/counter bookkeeping.
  - `benchmarks.rs`: `BenchStats::report()` only prints; no bound asserted, so regressions stay green.
  - `boot_stress_100.ps1`: Declares `$PassToken` but never reads it — boot that dies silently without fail tokens counts as pass. Also boots production image (no `self_test` feature), so 154 tests never run.
  - `extended_fuzz.ps1`: Same — boots production image, fuzzer not compiled in.
  - Network packet parsing: **Zero** registered tests.
- **Fix:** Rewrite stress tests to exercise real subsystems with assertions. Make fuzzer invoke syscalls. Add bounds to benchmarks. Fix boot stress to read pass token and use self_test image. Add network packet parsing tests.
- **If we skip it:** CI green ≠ kernel works. Regressions in memory, scheduler, network go undetected.

#### 15. **Dead code: 209 of 1,213 `pub fn`s have zero callers (17%)** (`issues.md` "Dead code" section)
- **What this is:** Mechanical count: function name appears nowhere outside its own definition line.
- **Notable clusters:**
  - `crates/net/src/congestion.rs` (81 lines) — complete duplicate of `tcp_congestion.rs` (221 lines) with zero callers.
  - `kernel/src/compositor/compositor.rs` — 10 dead fns including `composite_frame` and `composite_window`, the two core entry points.
  - `kernel/src/hypervisor/ept.rs` + `sched.rs` — 16 dead fns including `handle_ept_violation`, `schedule_vcpus`, `unmap_guest`.
  - `crates/drivers/src/block/scheduler.rs` — whole block-scheduler unwired.
  - `crates/vfs/src/skyfs/journal.rs` — `journal_begin` and `is_full` dead; journaling path only partly live.
- **Fix:** Delete dead code. Keep only what has callers.
- **If we skip it:** 17% of public API is dead weight — increases review burden, hides real dead code behind `allow(dead_code)`.

---

### SHOULD FIX — Risky Code Without Tests, Real Slowness, Duplication, Code That Should Not Exist

#### 16. **Documentation contradictions across AGENTS.md, README.md, PLAN.md, ROADMAP.md, tested-working.md, CONTEXT.md, MODULES.md, crates/README.md**
- **What this is:** Multiple docs claim different numbers for the same facts.
- **Specific contradictions:**
  - **Crate count:** AGENTS.md/CONTEXT.md/PLAN.md say "18 extracted crates"; MODULES.md/crates/README.md say "17 workspace crates"; tested-working.md says "19 workspace crates". Actual: 17 workspace crates + gdt (not in workspace) + sync (kernel-internal).
  - **Syscall count:** AGENTS.md says "~225 numbers, ~223 handlers, ~225 dispatch, ~40 functional"; README.md architecture says "207 syscalls"; syscall-classification.md says "~130 dispatched, ~40 functional"; PLAN.md says "207 syscalls".
  - **KASLR status:** PLAN.md Phase A0 claims "IMPLEMENTED AND VERIFIED (2026-10-16)"; issues.md A-6 says "STILL OPEN — relocations never applied"; tested-working.md says "Obfuscation only"; README.md says "relocations broken" in one place but "KASLR works" in badges.
  - **SMP status:** README.md says "SMP-4/8 boot and run (S-1, S-2 fixed)"; tested-working.md/ROADMAP.md/issues.md say SMP-4 hangs, SMP-8 hangs, S-1/S-2 OPEN.
  - **Hypervisor:** PLAN.md/README.md architecture diagram present as working; AGENTS.md/issues.md/tested-working.md say "stub, no real VMX/SVM execution".
  - **GUI/Compositor:** README.md lists features as if working; then says "PARTIAL — 10 dead fns including core entry points. Not usable as desktop."
  - **Selftest count:** README.md says "247/256 pass"; tested-working.md says "228/228 pass"; issues.md says "161/161 passed" for SMP tests.
  - **Driver count:** README.md says "7 working" but lists 6 names; tested-working.md lists same 6 names but says 7.
- **Fix:** Single source of truth. Pick one doc (ROADMAP.md claims to be it) and make all others reference it or derive from it. Delete stale claims.
- **If we skip it:** Contributors and reviewers cannot trust any documentation. Onboarding is broken.

#### 17. **vDSO claimed working but syscall-classification.md shows clock_gettime [F] only — vDSO fast path unverified** (`docs/architecture/vdso-design.md`, `syscall-classification.md`)
- **What this is:** ROADMAP.md A1.6 claims "vDSO ✅ Implemented (init, updates, address-space mapping, libc use)"; syscall-classification.md shows `clock_gettime` [F] but no vDSO-specific entries.
- **Problem:** If vDSO works, `clock_gettime`/`getpid`/`getcpu` should be userspace-only (no syscall). No test verifies this.
- **Fix:** Add test that `clock_gettime` in userspace does not trap to kernel. Verify vDSO page mapped in every process.
- **If we skip it:** vDSO claim is unverified; syscall overhead not actually reduced.

#### 18. **Hypervisor/ASH/eBPF/io_uring/compositor built by default or feature-gated but unreachable/stub** (`issues.md` "Subsystems compiled but barely reachable")
- **What this is:** 
  - `hypervisor/` (2843 lines): feature-gated off, 16 dead fns.
  - `ash/` (825 lines): feature-gated off, 8 dead fns, 4 syscall handlers unreachable.
  - `ebpf/` (2228 lines): built by default, 7 dead fns, reachable via `SYS_BPF`.
  - `compositor/` (743 lines): built by default, 10 dead fns incl. two core entry points.
- **Problem:** Dead code compiled into kernel image. Compositor built by default but unusable (core entry points dead).
- **Fix:** Gate compositor behind feature flag (like ash/hypervisor). Remove dead fns or wire them up.
- **If we skip it:** Kernel image bloated with dead code. Compositor wastes memory/CPU at boot.

#### 19. **APIC crate/kernel duplication — kernel copy live (41 call sites), crate has MSI bug** (`issues.md` "APIC: crate/kernel duplication")
- **What this is:** `vahi-apic` crate (1071 lines) and `kernel/src/apic` (1102 lines) define same 26 public functions. Kernel copy is live; crate duplicate has 4 call sites (all MSI, which is broken).
- **Problem:** Two copies to maintain. Crate's MSI pool never initialized (A-3). Swapping to crate requires adapter code for different state model (A-4).
- **Fix:** Fix crate's MSI (A-3) first as standalone, verify, then consider retiring kernel copy.
- **If we skip it:** Two APIC implementations drift. MSI silently broken for crate users.

#### 20. **Crate extraction incomplete — kernel redefines types instead of re-exporting** (`MODULES.md` "Kernel Integration Status", `crates/README.md`)
- **What this is:** Kernel defines `ObjectTypeId`, `ObjectHeader`, `KernelObject`, `Process`, `VfsNode`, `Credentials`, `Stat`, `FileSystem`, `VfsManager`, `SocketHandle`, TCP state, `SignalState`, `SeccompState`, `PtraceState` — all exist in crates but kernel redefines them.
- **Problem:** Divergent definitions. Crate types are canonical; kernel copies drift.
- **Fix:** Make kernel modules `pub use` crate types instead of redefining.
- **If we skip it:** Type mismatches, confusion, double maintenance.

#### 21. **FD-table growth paths have unbounded allocations** (`kernel/src/task/process.rs`, `issues.md FD-16`)
- **What this is:** FD table growth can allocate without bound.
- **Problem:** Malicious process could exhaust kernel memory via fd exhaustion.
- **Fix:** Add per-process fd limit (RLIMIT_NOFILE) enforcement at growth point.
- **If we skip it:** DoS via fd exhaustion.

#### 22. **DMA/IOMMU: `PooledDma::alloc` ignores `bdf`, never calls `iommu_map`** (`crates/hal/src/dma.rs:82-94`, `issues.md D-1`)
- **What this is:** Drivers import `PooledDma` but it doesn't map buffers through IOMMU.
- **Problem:** On IOMMU-capable hardware (enabled by default in `default` features), NVMe/xHCI DMA faults.
- **Fix:** Implement `iommu_map` in `PooledDma::alloc` or remove the type if unused.
- **If we skip it:** IOMMU + NVMe/xHCI = DMA faults on real hardware.

#### 23. **xHCI frees transfer DMA buffer on timeout while controller may still write** (`crates/drivers/src/usb/xhci/mod.rs:817-836`, `issues.md D-2`)
- **What this is:** `pending_dma.pop()` on timeout frees buffer; USB HID poller submits interrupt-IN per tick.
- **Problem:** Idle tick hits timeout path routinely; controller may still write into freed buffer.
- **Fix:** Wait for controller completion before freeing; use reference counting or completion callback.
- **If we skip it:** Use-after-free in DMA = memory corruption on real hardware.

#### 24. **E1000 ICR acknowledge code has zero callers — legacy IRQ path broken** (`crates/drivers/src/net/e1000.rs:613-641`, `issues.md E-2`)
- **What this is:** Only ICR acknowledge code has no callers; live handler reads ICR but never writes it (write-1-to-clear).
- **Problem:** Latent while MSI used, live on `pci_route_legacy_irq` fallback.
- **Fix:** Add ICR ack write in legacy IRQ path.
- **If we skip it:** Legacy IRQ (non-MSI) = lost interrupts, network stalls.

#### 25. **E1000 `reset_nic` never re-arms RDT — RX permanently killed after reset** (`crates/drivers/src/net/e1000.rs:435-468,327`, `issues.md E-3`)
- **What this is:** `reset_nic` rewrites RX ring base and re-enables RCTL but never re-arms RDT.
- **Problem:** After chip reset, no descriptors available; one TX timeout permanently kills RX.
- **Fix:** Re-arm RDT in `reset_nic`.
- **If we skip it:** Any NIC reset = permanent RX failure.

#### 26. **E1000 descriptor rings allocated with alignment 1 — hardware requires 16-byte** (`crates/drivers/src/net/e1000.rs:288,342`, `issues.md E-4`)
- **What this is:** `Box::new([..])` of packed type gives alignment 1; RDBAL/TDBAL require 16-byte.
- **Problem:** Undefined hardware behavior; may work on QEMU but fail on real hardware.
- **Fix:** Use `align(16)` or `alloc::alloc::alloc` with alignment.
- **If we skip it:** Real hardware NIC initialization unreliable.

#### 27. **xHCI `rt_regs()` casts 32 KiB struct at capability-register offset with no bounds check** (`crates/drivers/src/usb/xhci/mod.rs:105-114`, `issues.md X-1`)
- **What this is:** `rt_offset` from capability register used directly; `op_regs()`/`rt_regs()` hand out `&mut` from `&self` (clippy::mut_from_ref).
- **Problem:** Multiple live `&mut` can alias one MMIO region; offset unbounded.
- **Fix:** Validate `rt_offset` against MMIO BAR length. Use `UnsafeCell` or split borrows.
- **If we skip it:** UB from aliasing `&mut`; potential MMIO corruption.

#### 28. **xHCI `control_transfer` returns success when DMA alloc fails — feeds uninitialized dev_desc** (`crates/drivers/src/usb/xhci/mod.rs:786-802,838-846`, `issues.md X-3`)
- **What this is:** On `PooledDma::alloc` failure, TD submitted with no data stage; `cc == 1` reported as success with caller's buffer untouched.
- **Problem:** Uninitialized `dev_desc` printed as VID/PID.
- **Fix:** Return error on DMA alloc failure; don't submit TD without data stage.
- **If we skip it:** USB device enumeration prints garbage; may crash on real device.

#### 29. **Scheduler wake duplication — `wake_blocked_threads` and `wake_futex` share ~90% logic** (`PLAN.md` F3.2, `issues.md` "Thermo-Nuclear Review Findings")
- **What this is:** Two separate wake implementations for different wait queues.
- **Problem:** Duplication; bug fixes must be applied twice.
- **Fix:** Merge into single `wake_matching` helper (evaluated in F3.2 — decided to keep separate because different wake queues; document why).
- **If we skip it:** Maintenance burden; inconsistency risk.

#### 30. **Boot stress script declares pass token but never reads it** (`tests/boot_stress_100.ps1`, `issues.md H-1/H-2`)
- **What this is:** Script boots production image (no self_test), declares `$PassToken` but never checks it.
- **Problem:** Boot that dies silently without fail tokens counts as pass. Concurrent runs destroy each other's serial capture.
- **Fix:** Read pass token; use unique serial path per PID; boot self_test image for actual test coverage.
- **If we skip it:** "100 boots pass" claim is meaningless.

#### 31. **Extended fuzz script boots production image — fuzzer not compiled in** (`tests/extended_fuzz.ps1`, `issues.md`)
- **What this is:** Same as above — no `self_test` feature, so fuzzer absent.
- **Problem:** Zero crash-detection capability as written.
- **Fix:** Build and boot self_test image for fuzzing.
- **If we skip it:** Fuzzing gate is theater.

#### 32. **Sync tests silently disappear on aarch64** (`issues.md` "Documentation" section)
- **What this is:** `kernel/src/tests/mod.rs` gates `sync_tests::register()` on `target_arch` but `harness_tests::register()` on `self_test` feature.
- **Problem:** On aarch64, sync suite silently disappears and TAP plan shrinks while still reporting PASS. No aarch64 job runs the gate.
- **Fix:** Gate sync tests on feature, not arch. Run aarch64 tests in CI (once P-1 fixed).
- **If we skip it:** aarch64 test coverage silently reduced; false PASS.

---

### NICE TO HAVE — Small Speed-ups, Shorter Forms

#### 33. **Remove `kernel/linker.ld` — dead duplicate of root `linker.ld`** (`issues.md A-11`, `PLAN.md` "Not in this phase")
- **What this is:** Root `linker.ld` is the live one (`-C link-arg=-Tlinker.ld` resolves to workspace root). Kernel copy has CRLF line endings that break `rust-lld`.
- **Fix:** Delete `kernel/linker.ld`.
- **Savings:** One less file to maintain; avoids confusion.

#### 34. **`coverage::record_block` has no callers** (`PLAN.md` "Not in this phase")
- **What this is:** Coverage tracking function never called.
- **Fix:** Delete or wire up.
- **Savings:** Dead code removal.

#### 35. **Reduce `main.rs` clippy suppressions further** (`PLAN.md` F2.7, `AGENTS.md` Non-negotiable #1)
- **What this is:** 27 clippy lints suppressed in `main.rs` via blanket `#![allow]`.
- **Fix:** Fix the real issues (too_many_arguments → restructure; collapsible_if → flatten) instead of suppressing.
- **Savings:** Cleaner code; real style issues visible.

#### 36. **Gate `test_memory_allocations` behind `self_test` or delete** (`PLAN.md` F2.4)
- **What this is:** 8 `serial_write` TRACE lines + 10k `spin_loop` iterations in boot path.
- **Fix:** `#[cfg(feature = "self_test")]` or delete.
- **Savings:** Cleaner serial output; faster boot.

#### 37. **APIC unused public functions: `apic_id_for_cpu`, `init_timer_count`, `set_timer_count`, `timer_ticks`** (`PLAN.md` F2.5)
- **What this is:** Public functions never called from live code.
- **Fix:** Delete or `#[cfg(test)]` gate.
- **Savings:** Dead code removal.

#### 38. **3 TODOs in codebase = 1 logical issue (vDSO namespace flags)** (`PLAN.md` F2.6)
- **What this is:** 3 TODO/FIXME/HACK comments found (3 in `apic/errata.rs` stubs, 1 in `vfs/fuse.rs`).
- **Fix:** Categorize — errata stubs are intentional placeholders; fuse TODO needs real fix or ADR.
- **Savings:** Zero actionable TODOs.

#### 39. **`vahi-gdt` crate not in workspace but listed in dependency graph** (`MODULES.md`, `crates/README.md`)
- **What this is:** `vahi-gdt` (76 lines, scaffolded) is NOT in workspace but `vahi-task` depends on it.
- **Fix:** Add to workspace or remove dependency.
- **Savings:** Consistent build.

#### 40. **`crates/vfs/src/defs.rs` at 933 lines — impending 1k threshold** (`MODULES.md` "File Size Guidelines")
- **What this is:** Single file approaching 1000 lines.
- **Fix:** Evaluate split (e.g., separate filesystem type definitions from VFS core).
- **Savings:** Maintainability.

---

## Verdict

**Not healthy.** The kernel boots and reaches login on QEMU x86_64 SMP-1/2, but:
- KASLR is fake (critical security claim false)
- SMP >2 hangs (S-1, S-2 open)
- Cross-arch build broken (P-1)
- Multiple driver bugs are security/stability issues (F-1, F-2, N-1, N-2, A-1, A-2, E-1)
- Documentation is contradictory across 8+ files
- Test suite gives false confidence (stress tests don't stress, fuzzer doesn't fuzz, benchmarks don't assert, boot stress doesn't verify)
- 17% of public API is dead code

**Priority order to fix:**
1. KASLR relocations (A-6) — security claim must be honest
2. SMP-4/8 hangs (S-1, S-2) — blocks multi-core
3. aarch64 build (P-1) — blocks multi-arch
4. ext2/SkyFS bounds checks (F-1, F-2) — security boundary
5. NVMe/AHCI/E1000/xHCI driver bugs (N-1, N-2, A-1, A-2, E-1, X-2) — data corruption/hang risks
6. Documentation reconciliation — single source of truth
7. Test suite rewrite — make CI evidence meaningful
8. Dead code deletion — 209 dead pub fns

---

## Lean: -X lines, -Y dependencies possible

- **Dead code removal:** ~209 `pub fn`s across kernel + crates → estimate 5,000-10,000 lines
- **Duplicate `congestion.rs`:** -81 lines (entire file)
- **Compositor dead fns:** 10 functions including 2 core entry points
- **Hypervisor dead fns:** 16 functions
- **ASH dead fns:** 8 functions
- **Block scheduler:** entire unwired module
- **SkyFS journal dead fns:** 2 functions
- **Unused APIC functions:** 4 functions
- **`test_memory_allocations`:** ~50 lines of trace spam
- **`kernel/linker.ld`:** entire file (duplicate)
- **`coverage::record_block`:** 1 function

**Dependencies:** `vahi-sync` x86_64-only asm blocks cross-arch — fix or split arch-specific impl. No new dependencies needed; deletions only.

---

## Not Checked

- Real hardware behavior (never tested)
- aarch64/riscv64 runtime behavior (build fails)
- Dynamic linking (ELF loader supports but no musl binary tested)
- Filesystem crash recovery (no crash testing)
- Concurrent file access under load
- Network stress >1 connection
- Hypervisor VMX/SVM execution (stub only)
- ASH sandbox runtime (syscall handlers unreachable)
- eBPF JIT on real workloads
- io_uring beyond stub
- GUI compositor as desktop (core entry points dead)
- Long-running memory leak detection beyond boot audit
- `vahi-verified` proofs (feature-gated, optional dep)

---

*Report generated by ponytail-audit. One-shot, changes no code.*
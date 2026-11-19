# Vahi Kernel — Master Plan

**Date:** 2026-11-19
**Replaces:** All previously deleted plan files (architecture-review-plan, implementation-plan-1, implementation-plan-2, kernel-future-plan, apic-subsystem-plan, tasks/plan.md, tasks/todo.md)

---

## Scope

**This is the Vahi Kernel project only.** The OS/userspace (shell, init, coreutils, toolchain) is a separate project in another folder. This plan covers kernel work only.

### What exists and works
- **344 Rust source files, ~82,266 lines** across kernel + 17 workspace crates (+ gdt not in workspace, sync kernel-internal)
- **216 kernel src files** in `kernel/src/` across 30+ subsystems
- **~130 syscalls dispatched**, ~40 fully functional (`pub fn sys_*`) across 36 files in `syscalls/`
- **17 workspace crates** in `crates/` (memory, crypto, hal, limine, apic, types, arch, task, syscalls, vfs, net, drivers, acpi, objects, ipc, pci, verified); gdt NOT in workspace; sync is kernel-internal
- **2 supported filesystems** (TarFS, DevFS), **7 experimental/partial** (SkyFS, ext2, ext4, FAT32, ramfs, ctlfs, FUSE)
- **Scheduler**: Stride heap + SCHED_FIFO/SCHED_RR + CPU affinity + work stealing
- **Memory**: Buddy allocator, slab, page tables (COW via isolate VM), swap (in-memory stub), frame tracking, stack allocator
- **Networking**: smoltcp TCP/UDP, DHCP, DNS, Unix sockets, zero-copy, RSS, TSO, TCP Reno congestion control
- **Security**: SMAP/SMEP/UMIP (CR4 bits only), KASLR (obfuscation only — relocations never applied, A-6), CFI ("2 static targets" — not real), seccomp BPF (verifier exists, no filter execution), Landlock (structures defined, no enforcement), capabilities (data structures exist, enforcement minimal), audit (serial output only)
- **Hypervisor**: VMX/SVM structures, EPT/NPT, vCPU, 10+ VM syscall numbers — **STUB, no real VMX/SVM execution**
- **eBPF**: VM, verifier, JIT (x86_64), 4 helpers, maps — **PARTIAL, feature-gated**
- **ASH**: Verifier, interpreter, manager, hooks (net, syscall), JIT via ebpf/jit.rs, W^X exec-memory allocator — **FEATURE-GATED, syscall handlers unreachable**
- **GUI**: Compositor, window manager, terminal, splash, notifications, clipboard — **PARTIAL, 10 dead fns including core entry points; not usable as desktop**
- **Crypto**: SHA-256, HMAC, PBKDF2, entropy (RDRAND+TSC)
- **Objects**: KernelObject trait, handle table, security, namespace
- **Containers**: PID/Mount/Net/IPC/UTS/User namespaces (data structures), cgroup v2 (stub), unshare/setns (stub)
- **SMP**: SIPI boot, work stealing, per-CPU schedulers — **SMP-1/2 work; SMP-4 hangs after tests; SMP-8 hangs early boot (S-1, S-2 OPEN)**
- **Testing**: 228 selftests (count in `tests/selftest_count.txt`), stress tests, syscall fuzzer, coverage tracking
- **Boot**: Limine bootloader, UEFI, KASLR (obfuscation only), boot state machine

### Thermo-Nuclear Review Findings (verified against code, 2026-08-03; addressed in F2)

| Finding | Severity | Evidence | Status |
|---------|----------|----------|--------|
| **19 modules suppress `#![allow(dead_code)]`** | High | ash/, drivers/audio/, drivers/usb/uhci+ xhci, hypervisor/svm+devices, memory/phys+virt, shell.rs, task/keyboard, verified/*, vfs/skyfs/* | ✅ Reduced to ~7 legitimate (acpi_prt, vdso, hypervisor, kaslr_reloc, security, selftest, shell, sync, vfs) |
| **95 total `allow(dead_code)` annotations** | High | Not just main.rs — scattered across apic/, ash/, drivers/, verified/, vfs/skyfs/ | ✅ Reduced to ~19 legitimate placeholders |
| **27 clippy lints suppressed in main.rs alone** | Medium | Includes `too_many_arguments`, `collapsible_if`, `single_match`, `needless_return` | ✅ main.rs clippy clean (292 lines) |
| **main.rs is a god module (614 lines, 14 functions)** | High | GUI logic, ELF loading, trace spam, panic handler, serial I/O, KASLR init | ✅ Decomposed to 292 lines; entry logic in entry.rs |
| **`gui_refresh_task` handles raw PS/2 scancodes in main.rs** | Medium | Alt-Tab, modifier tracking, scancode decoding in boot module | ✅ Moved to gui/input.rs |
| **`spawn_userspace_app` duplicates process loading logic** | Medium | Inline ELF loading + FD setup duplicates process_lifecycle.rs | ✅ Removed; execve path used |
| **`test_memory_allocations` is pure noise** | Low | 8 TRACE lines + 10k spin_loop | ✅ Removed |
| **APIC has unused public functions** | Low | `apic_id_for_cpu`, `init_timer_count`, `set_timer_count`, `timer_ticks` | ✅ Removed from smp.rs |
| **Scheduler has near-duplicate wake paths** | Medium | `wake_blocked_threads` and `wake_futex` share ~90% logic | ⚠️ Evaluated — keep separate (different wake queues) |

### What's unverified or missing
| Gap | Evidence | Priority |
|-----|----------|----------|
| 100-consecutive-boot test | Never run (25/25 boots pass at SMP-1/2) | High |
| Real userspace binaries | No integration test runs `ls`, `cat`, etc. in QEMU | High |
| Performance baselines | 6 microbenchmarks exist (`tests/benchmarks.rs`) but no CI regression thresholds | High |
| Memory leak detection | Frame HWM + slab snapshots added (F1.6) — PASS on current tree | High |
| `vahi-verified` crate extraction incomplete | `crates/vfs` verification feature gates code that imports `vahi_verified::{journal,runner}` — crate does not exist in workspace; `kernel/src/verified/` is the source | High |
| 4+ CPU SMP stress | SMP-4 hangs after tests (S-2); SMP-8 hangs early boot (S-1) — issues.md | High |
| Dynamic linking | ELF loader supports it but no musl-linked binary tested | Medium |
| Real hardware | Everything runs in QEMU only | Medium |
| Scheduler wake duplication | `wake_blocked_threads` and `wake_futex` share ~90% logic — evaluated, keep separate (different wake queues) | Medium |
| Documentation accuracy | Stale docs being reconciled this session | Medium |
| Cross-arch builds | aarch64 FAILS at vahi-sync x86_64-only asm (P-1); riscv64 not built in CI | High |
| KASLR relocations | Never applied — ELF magic check fails (A-6) | High |
| SkyFS crash testing | No crash testing, no fsck, unbounded extent copy (F-2) | High |
| ext2 dir traversal | Unbounded — corrupt rec_len walks past block (F-1) | High |

### Files approaching the 1k-line threshold
| File | Lines | Risk |
|------|-------|------|
| `syscalls/dispatch.rs` | 1692 | Over — needs split |
| `task/process.rs` | 1034 | Over — types submodule (400+ lines) extracted; Process impl cohesive |
| `syscalls/process_lifecycle.rs` | 982 | Over — cohesive impl for fork/exec/wait/exit; no split needed |
| `iommu.rs` | 1141 | Over — single driver, acceptable as-is |
| `syscalls/fs_stat.rs` | 1152 | Over — needs split |
| `syscalls/net_options.rs` | 949 | Impending |
| `crates/vfs/src/defs.rs` | 933 | Impending |
| `crates/drivers/src/usb/xhci/mod.rs` | 899 | Impending |
| `task/thread.rs` | 896 | Impending |
| `crates/vfs/src/skyfs/mod.rs` | 893 | Impending |
| `syscalls/fs_io.rs` | 832 | Acceptable |
| `syscalls/misc.rs` | 819 | Acceptable |
| `crates/drivers/src/storage/nvme.rs` | 786 | Acceptable |
| `syscalls/net_helpers.rs` | 760 | Acceptable |
| `crates/vfs/src/ext4.rs` | 754 | Acceptable |
| `syscalls/process_creds.rs` | 741 | Acceptable |
| `crates/drivers/src/net/e1000.rs` | 729 | Acceptable |
| `crates/drivers/src/storage/ahci.rs` | 699 | OK |
| `task/scheduler/mod.rs` | 749 | Split into 4 submodules (spawn, switch, tick, mod) |

**Note:** `main.rs` reduced to 292 lines (F2.3 done); `interrupts.rs` split into 4 submodules (F3.1 done).

---

## Plan Structure

The plan has three tiers:
1. **Foundation** (must do first) — Hardening, measurement, documentation truth
2. **Growth** (after foundation) — Real hardware, missing capabilities, optimization
3. **Aspiration** (future) — Advanced features, ecosystem, self-hosting

---

## Tier 1: Foundation — Prove It Works, Prove It's Stable

### Phase F1: Measure Everything

**Goal:** Replace "works" with "works, and here are the numbers."

| Task | What | Done When |
|------|------|-----------|
| F1.1 | **Benchmark harness**: Add `tests/benchmarks.rs` with timed loops for fork/exec throughput, pipe bandwidth, context-switch latency, mmap/munmap churn | Benchmark prints numbers to serial; QEMU selftest includes them |
| F1.2 | **Boot reliability**: Script100 consecutive QEMU boots with serial capture; count passes/failures | Script exists, run overnight, report: X/100 passed |
| F1.3 | **Userspace smoke test**: In QEMU, boot to init, run `ls /`, `cat /etc/hostname`, `echo hello`, verify output on serial | Integration test script passes |
| F1.4 | **SMP stress**: Boot with 4 and 8 CPUs, run stress test for 60 seconds, report pass/fail | **BLOCKED**: SMP-4 hangs after tests (S-2); SMP-8 hangs early boot (S-1) — issues.md |
| F1.5 | **Fuzz long run**: Run syscall fuzzer for 1 hour minimum, capture crashes | Zero crashes |
| F1.6 | **Memory leak auditing**: Add page-frame high-water-mark tracking and slab utilization snapshots. Take snapshots before/after boot test, after SMP stress, after fuzz run. Alert if any subsystem leaks frames or slab blocks | Leak report printed at shutdown; no net growth across stress runs |
| F1.7 | **Filesystem crash-recovery baseline**: Write files, kill QEMU with `-no-reboot` (simulates power loss), reboot, verify data integrity. Establish baseline crash-recovery pass rate | Baseline documented; pass rate known before Tier 2 work |

### Phase F2: Kill Dead Weight

**Goal:** Every line of code earns its place. The actual scope was 95 `allow(dead_code)` annotations across 19 modules — reduced to ~7 module-level and ~19 function-level (all legitimate placeholders).

| Task | What | Done When |
|------|------|-----------|
| F2.1 | **Remove module-level `#![allow(dead_code)]`** from all modules. Fix or remove each resulting warning. Modules: ash/, drivers/audio/, drivers/usb/uhci+xhci, hypervisor/svm+devices, memory/phys+virt, shell.rs, task/keyboard, verified/*, vfs/skyfs/* | ✅ Done — reduced from 19 to ~7 legitimate (acpi_prt, vdso, hypervisor, kaslr_reloc, security, selftest, shell, sync, vfs) |
| F2.2 | **Remove function-level `allow(dead_code)`** from the remaining annotations. Fix or remove each. | ✅ Done — reduced from 95 to ~19 legitimate placeholders |
| F2.3 | **Decompose main.rs** (was 614 lines, now 292): Move `gui_refresh_task` to `gui/input.rs`, move `spawn_userspace_app`+`app_starter` to `boot/launcher.rs`, move `test_memory_allocations` behind `#[cfg(feature = "self_test")]` or delete, keep only `kernel_main`, `panic`, serial I/O, KASLR in main.rs | ✅ Done — main.rs 292 lines; entry logic in entry.rs |
| F2.4 | **Remove boot-trace spam**: Delete the 8 `[TRACE]` serial_write lines in `test_memory_allocations` and the 10k spin_loop. If the memory test is worth keeping, gate it behind self_test. | ✅ Done — clean serial output: only BOOT/TEST/SELF-TEST messages |
| F2.5 | **Remove APIC dead code**: Delete or `#[cfg(test)]` gate unused functions `apic_id_for_cpu`, `init_timer_count`, `set_timer_count`, `timer_ticks` | ✅ Done — removed from smp.rs |
| F2.6 | **Grep for TODO/FIXME/HACK**: 4 found (3 in apic/errata.rs stubs, 1 in vfs/fuse.rs). Categorize each — errata stubs are intentional placeholders, fuse TODO needs a real fix | ✅ Done — 3 TODOs = 1 logical issue (namespace flags for vDSO) — intentional placeholder |
| F2.7 | **Reduce clippy suppressions in main.rs**: Remove the blanket `#![allow(clippy::...)]` block (27 lints). Fix the real issues (too_many_arguments → restructure; collapsible_if → flatten) | ✅ Done — clippy clean on main.rs (0 errors) |

### Phase F3: Structural Evaluation (if needed)

**Principle:** Maintain structural cohesion over arbitrary line-count caps. A cohesive module that reads as a single coherent unit should stay together — splitting it into subtrees just to meet a numeric threshold creates more confusion than it solves. Only decompose when distinct concerns (e.g., dispatch vs. registration) are genuinely separable and the split improves readability.

**Goal:** Evaluate files approaching1k lines. Split only if the codebase benefits. Also address the scheduler wake duplication.

| Task | What | Done When |
|------|------|-----------|
| F3.1 | **Evaluate `interrupts.rs` (979 lines)**: Assess whether interrupt dispatch and handler registration are distinct concerns worth separating. If they share state and read as one unit, leave it | ✅ Done — already split into 4 submodules (diag, exceptions, page_fault, irq) — 216 lines in mod.rs |
| F3.2 | **Evaluate `task/scheduler/`**: Assess whether stride logic and run-queue management are separable without breaking locality. Also evaluate merging `wake_blocked_threads` and `wake_futex` into a single `wake_matching` helper (they share ~90% logic) | ✅ Done — split into 4 submodules (spawn, switch, tick, mod) — 749 lines; wake paths evaluated — keep separate (different wake queues) |
| F3.3 | **Evaluate `task/process.rs` (1034 lines)**: Assess whether ELF loading and process lifecycle are distinct enough to split. Process creation, forking, execve, and teardown share heavy state | ✅ Done — types submodule (400+ lines) extracted; Process impl cohesive; no split needed |
| F3.4 | **Evaluate `process_lifecycle.rs` (982 lines)** | ✅ Done — cohesive impl for fork/exec/wait/exit; no split needed |
| F3.5 | **Evaluate `iommu.rs` (1141 lines)** | ✅ Done — single driver, acceptable as-is |

### Phase F4: Documentation Truth Reconciliation

**Goal:** Docs reflect code reality. No doc says "TODO" or references deleted files.

| Task | What | Done When |
|------|------|-----------|
| F4.1 | **Delete or rewrite `docs/kernel-future-plan.md`** (already deleted) — replace with pointer to this plan | ✅ Done — no stale plan files |
| F4.2 | **Update `README.md`** feature tables against actual code (e.g., "12+ drivers" — verify count) | 🔄 In Progress — this session |
| F4.3 | **Update `docs/roadmap-revised/`** to remove the "✅ COMPLETE" claims that are unverified (e.g., "Phase 9 complete" when some features are stubs) | ✅ Done — ROADMAP.md corrected; roadmap-revised/ to be archived |
| F4.4 | **Add `docs/ROADMAP.md`** — single source of truth, replaces the 11-file roadmap-revised directory | ✅ Done — ROADMAP.md exists and corrected |
| F4.5 | **ADR index consistency**: Verify all ADRs reference real code, delete or update orphaned ones | 🔄 In Progress — ADR-017 needs update (KASLR), ADR-013/014 missing |

### Phase F5: CI Hardening

**Goal:** CI catches regressions before they land.

| Task | What | Done When |
|------|------|-----------|
| F5.1 | **CI builds all feature combos**: default, all-features, each feature solo | ✅ Done — `ci.yml` matrix covers all combos |
| F5.2 | **CI runs clippy with `-D warnings`** on default, no-features, all-features, release+self_test, extracted crates; solo-feature builds are not clippy'd (tracked as gap) | ✅ Done — zero clippy regressions on gated combos |
| F5.3 | **Nightly QEMU selftest in CI**: Build bootimage, run QEMU, parse serial for "N passed, 0 failed" | ✅ Done — SMP 1 + SMP 2 pass; SMP 4/8 not tested |
| F5.4 | **Dependabot or manual audit**: Ensure crates haven't gone stale or have CVEs | ✅ Done — `.github/dependabot.yml` + nightly cargo audit |
| F5.5 | **Cross-arch CI** | ❌ Failing — aarch64 build fails at vahi-sync (issues.md P-1) |

---

## Tier 2: Growth — Real Hardware, Missing Capabilities (Kernel Only)

### Phase G2: Real Hardware Bring-Up

**Goal:** Boot on actual x86_64 hardware, not just QEMU.

| Task | What | Done When |
|------|------|-----------|
| G2.1 | **UEFI handoff**: Ensure Limine protocol works on real UEFI firmware (test on 2+ boards) | Kernel boots on real hardware |
| G2.2 | **ACPI quirks**: Real hardware has broken DSDTs. Add quirk table for known-bad boards | Boots on at least one desktop and one laptop |
| G2.3 | **SMP on real hardware**: QEMU SMP is simplified; real hardware has weird AP startup timing | 4+ cores work on real hardware |
| G2.4 | **PCI device discovery**: Real hardware has more devices; ensure driver probing doesn't crash | Enumerates all PCI devices without panic |

### Phase G3: POSIX Compliance (Missing Kernel Syscalls)

**Goal:** Add missing syscalls needed by real-world software.

| Task | What | Done When |
|------|------|-----------|
| G3.1 | **`getrandom`**: Needed by almost all modern software | Syscall works, selftest passes |
| G3.2 | **`memfd_create`**: Needed by modern IPC patterns | Syscall works |
| G3.3 | **`prlimit64`**: Needed by glibc/musl | Syscall works |
| G3.4 | **`getrusage`**: Needed by shells and profilers | Syscall works |
| G3.5 | **`kqueue`**: macOS/BSD compatibility (complement to epoll) | Syscall works |
| G3.6 | **Signal delivery hardening**: `sigaltstack`, `siginfo_t`, proper `SA_SIGINFO` | Signal-heavy programs don't crash |

### Phase G4: Networking Hardening

**Goal:** Network stack survives real traffic, not just lab conditions.

| Task | What | Done When |
|------|------|-----------|
| G4.1 | **TCP stress**: Transfer 100MB+ file over TCP, verify byte correctness | Checksum matches |
| G4.2 | **Concurrent connections**: 100+ simultaneous TCP connections | No lockups, no memory exhaustion |
| G4.3 | **DNS resolver hardening**: Handle timeouts, retries, malformed responses | DNS works with real resolver (8.8.8.8) |
| G4.4 | **Socket option completeness**: Verify all 25+ socket options actually affect behavior | Each option tested with real traffic |

### Phase G5: Filesystem Hardening

**Goal:** Data survives crashes and stress.

| Task | What | Done When |
|------|------|-----------|
| G5.1 | **SkyFS crash test**: Write files, kill QEMU mid-write, reboot, verify journal replay | Files either fully written or fully absent after crash |
| G5.2 | **ext2 write stress**: Create/delete 1000 files, verify integrity | No corruption |
| G5.3 | **Concurrent file access**: Multiple processes writing to same file | No data corruption |
| G5.4 | **FIFO/pipe stress**: Fill pipe buffer, verify backpressure | Writer blocks when pipe full |


## Tier 3: Aspiration — Advanced Features

### Phase A1: Performance Optimization

| Task | What |
|------|------|
| A1.1 | **Slab allocator tuning**: Profile allocation patterns, adjust size classes |
| A1.2 | **Page cache eviction**: Replace FIFO with LRU or clock algorithm |
| A1.3 | **RCU optimization**: Reduce grace-period overhead for read-heavy workloads |
| A1.4 | **Scheduler EEVDF migration**: Replace stride with EEVDF for better latency fairness |
| A1.5 | **Context switch optimization**: Minimize save/restore overhead |
| A1.6 | **vDSO (Virtual Dynamic Shared Object)**: Map a read-only kernel data page into every userspace process. Kernel updates `clock_gettime` (CLOCK_MONOTONIC, CLOCK_REALTIME), `getpid`, `getcpu` data on timer tick. Userspace reads directly without syscall overhead. Target: 100× faster time/process queries. Requires: vDSO data struct, page mapping at process creation, timer tick update, fast-path syscall stubs. |

### Phase A2: Advanced Kernel Features

| Task | What |
|------|------|
| A2.1 | **io_uring full implementation**: SQE/CQE ring, io_uring_register, linked SQEs |
| A2.2 | **inotify/kqueue filesystem events**: Watch file changes |
| A2.3 | **FUSE write support**: Enable user-space filesystem writes |
| A2.4 | **Transparent huge pages**: 2MiB pages for large allocations |
| A2.5 | **Kernel module loading**: Loadable kernel modules with dependency resolution |

### Phase A3: Security Hardening

| Task | What |
|------|------|
| A3.1 | **KASLR entropy improvement**: Use more boot-time entropy sources |
| A3.2 | **Stack protector audit**: Ensure all functions have canaries |
| A3.3 | **Syscall filtering audit**: Verify seccomp BPF is actually enforced at all entry points |
| A3.4 | **Audit subsystem**: Log all security-relevant events, not just selected ones |

### Phase A4: Multi-Architecture

| Task | What |
|------|------|
| A4.1 | **aarch64 bring-up**: Complete the arch stub to boot on QEMU virt (ARM) |
| A4.2 | **RISC-V bring-up**: Complete the arch stub to boot on QEMU virt (RISC-V) |
| A4.3 | **Arch trait expansion**: Add missing methods for full multi-arch support |

---

## Execution Order

```
F1 (Measure + Leak Audit) → F2 (Clean) → F3 (Evaluate) → F4 (Docs) → F5 (CI)
    ↓
G2 (Real HW) → G3 (POSIX Syscalls) → G4 (Network) → G5 (FS)
    ↓
A1 (Perf) → A2 (Features) → A3 (Security) → A4 (Multi-arch)
```

**Note:** G1 (Userspace Ecosystem) is in the separate OS project. This kernel plan covers G2-G5.

Each phase has a verification gate. Don't start the next phase until the current one's gate passes.

---

## Verification Gates (Honest)

| Gate | Criteria | Current |
|------|----------|---------|
| F1 pass | Boot reliability script run; benchmarks produce numbers; self-test suite passes; leak audit shows zero net growth | ❌ F1.2 (100-boot never run), F1.3 (userspace smoke untested), F1.4 (SMP 4/8 blocked S-1/S-2), F1.5 (fuzz long run deferred), F1.7 (FS crash-recovery deferred) |
| F2 pass | `allow(dead_code)` count near zero (from 95); main.rs under 300 lines; clean serial output; clippy clean on main.rs without blanket suppression | ✅ All criteria met |
| F3 pass | All files evaluated; splits justified or declined; scheduler wake duplication addressed; all builds pass | ⚠️ Partial — files evaluated, splits justified, wake duplication evaluated (kept separate), builds pass |
| F4 pass | All docs match code; single roadmap file exists | 🔄 In Progress — this reconciliation session |
| F5 pass | CI matrix covers all feature combos; QEMU selftest in CI; cross-arch green | ❌ Cross-arch red (aarch64 fails at vahi-sync P-1) |
| G2 pass | Boots on real hardware; 4+ CPUs work | 🔲 Not started |
| G3 pass | getrandom/memfd_create/prlimit64/getrusage/kqueue work; signal-heavy programs don't crash | ⚠️ Partial — getrandom/memfd_create/prlimit64/getrusage ✅; kqueue deferred; signal hardening partial |
| G4 pass | 100MB TCP transfer succeeds; 100 concurrent connections work | 🔲 Not started |
| G5 pass | SkyFS survives crash; ext2 write stress passes; concurrent writes work | 🔲 Not started |

---

## Rules of Engagement

1. **One commit per task.** Message format: `kernel: <scope> — <summary>`
2. **Verify after each commit.** At minimum: `cargo build` + `cargo build --features <relevant>`.
3. **Don't mix features and refactors.** A commit either adds behavior or improves structure — not both.
4. **Measure before optimizing.** No performance work without a baseline number.
5. **Test on real hardware when possible.** QEMU is a start, not the finish line.
6. **Keep CONTEXT.md and ADRs current.** If you make a decision, record it.
7. **Honest status.** Never mark something ✅ unless there's test evidence.

---

## What This Plan Replaces

| Deleted File | Replaced By |
|--------------|-------------|
| `docs/apic-subsystem-plan.md` | APIC is complete; future work noted in Tier 3 |
| `docs/architecture/architecture-review-plan.md` | Architecture review candidates addressed in F2/F3 |
| `docs/architecture/implementation-plan-1-deletion-campaign.md` | Deletion campaign is complete |
| `docs/architecture/implementation-plan-2-wire-up-group-a.md` | Group A wiring is mostly complete |
| `docs/kernel-future-plan.md` | Superseded by this plan's Tier 3 |
| `tasks/plan.md` | A4/A5 tasks are complete |
| `tasks/todo.md` | This plan's task tables replace it |

---

## Phase A0: KASLR Relocation Correctness (issue A-6)

**Status: PARTIAL — relocations applied on PIC builds but A-6 remains OPEN.** The PIC link route (commit `310b2b4`) places `R_X86_64_RELATIVE` entries in an alloc `.rela` section inside a `PT_LOAD`, and the relocation walk runs on boot. However, `issues.md A-6` remains **STILL OPEN** because:

1. The `--emit-relocs` path (non-PIC) was never made to work — lld emits non-alloc `.rela` sections
2. The `[KASLR-VERIFY] match:` printer still prints `NO` (A0.4 unfixed)
3. SMP≥2 boots with relocations untested; audit profiles untested
4. The ELF magic gate in `kaslr_reloc.rs:338` still fails on non-PIC builds

**Honest assessment:** KASLR currently provides **obfuscation only** — the kernel randomizes its mapping but the relocation walk only works on the PIC build path. The non-PIC (release) build path has zero relocations applied. A-6 in `issues.md` correctly tracks this as OPEN.

Evidence ledger: `docs/architecture/issues.md` (A-6), inventory in `docs/architecture/UNDONE-audit-profile.md`.

The sections below document the historical `--emit-relocs` investigation (kept for context) and the PIC route tasks.

### Architecture decision

`--emit-relocs` is now on the root `.cargo/config.toml`, which produces
124,765 `.rela.text` + 15,866 `.rela.rodata` + 1 `.rela.data` entries that
were previously absent. The emitted type is `R_X86_64_64` (291,977 entries),
**not** `R_X86_64_RELATIVE` — the walk logic differs from the original plan.

The blocker is placement: lld emits these as non-alloc (`sh_addr = 0`) and
refuses to place them, so `__rela_start`/`__rela_end` never materialise.

*(Superseded — kept as history: the shipped route dropped `--emit-relocs`
and links PIC; lld then places the `R_X86_64_RELATIVE` table as an alloc
section inside a `PT_LOAD`. See the status block.)*

### Tasks

#### A0.1 — Diagnose why lld will not place the `.rela` output section
**Description:** `__rela_start`/`__rela_end` stay missing and `__kernel_end`
is byte-identical across three different definitions of the section's extent.
Get lld to state the reason instead of inferring it.

**Acceptance criteria [met via the PIC route]:**
- [x] Symbol table shows `__rela_start`/`__rela_end` at real addresses
      (`0xffffffff800fc000`/`0xffffffff80116688` in the prod build;
      `readelf -sW`, the binutils equivalent of the named `llvm-nm`)
- [x] `.rela` appears as an output section with `SHF_ALLOC` set
- [x] The `.rela` section falls inside a `PT_LOAD` (`r--` segment at file
      offset `0xfb000`)

**Verification:**
- [x] `cargo build --release --target x86_64-unknown-none --features self_test`
      (the `--profile audit` build named here was not the run; the `audit`
      profiles still exist and were not exercised)
- [x] `readelf -S` shows `.rela` with `Address = 0xffffffff800fc000` and the
      alloc flag
- [x] `readelf -sW` shows both symbols with real addresses
- [x] Boot reports 189/189 (the suite grew past 177 with the relocation
      contract tests)

**Approaches to try, in order** (do not repeat these, already ruled out):
1. `-Wl,-t` or `--print-map` trace to see lld's section-placement decision
2. `*(.rela.*)` catch-all, not an enumerated list
3. Force `SHF_ALLOC` on the inputs via `llvm-objcopy --set-section-flags`
4. **Fallback if lld refuses:** have `build.rs` generate a `.S`/`.o` carrying the
   table as ordinary alloc data, sidestepping lld's own emission entirely

**Dependencies:** None
**Files:** `linker.ld`, `kernel/build.rs`, `builder/build_limine_image.py`
**Scope:** S

#### A0.2 — Replace the ELF-header magic gate with a symbol-driven walk
**Description:** Once A0.1 lands, rewrite `apply_kaslr_relocations`
(kaslr_reloc.rs:443-499) to iterate `__rela_start..__rela_end` instead of
walking `e_shoff`/section headers. Delete the gate at :455.

**Acceptance criteria:**
- [x] `apply_kaslr_relocations` reads no ELF header and never checks magic —
      the walk iterates linker-emitted `__rela_start..__rela_end` bounds only
- [x] It returns `RelocationOutcome`; `activate_kaslr_mapping` checks
      `outcome.succeeded()` and refuses to switch CR3 on refusal (printing
      `relocation refused (applied=… failed=…)`) instead of claiming success
- [x] A refused pass does not claim success: `post_kaslr_continue()`
      continues on the link-time mapping
- [x] `read_kernel_segments` and `Elf64Phdr64` deleted (`e9d1697`); the
      huge-page branch derives its span from `kernel_span_bytes()` and maps
      the old-index aliases it previously assumed still existed

**Verification:**
- [x] Test asserting a zero-relocation table reports failure:
      `kaslr:zero_relocations_is_reported_as_failure`
- [x] Boot serial shows a non-zero applied count (`rela entries=119b
      in_image=119b out_of_image=0`)
- [x] 189/189 still pass (suite grew past 177)
- [ ] `[KASLR] match:` reports YES — **no**; the printer still prints `NO`
      (see A0.4)

**Dependencies:** A0.1
**Files:** `kernel/src/kaslr_reloc.rs`, `kernel/src/main.rs`
**Scope:** M

#### A0.3 — Handle relocation types correctly
**Description:** The table is `R_X86_64_64`, not `RELATIVE`. Each type needs
the right treatment: `R_X86_64_64`/1 (291,977) is `*offset += slide`;
`R_X86_64_32S`/11 (28,922) and `R_X86_64_PC32`/2 (579) are relative-to-symbol
and must **not** be blindly slid; `GOTPCREL`/9 (1,032) is RIP-relative and
self-adjusts.

**Acceptance criteria:**
- [x] Each type is either applied correctly or explicitly skipped with a
      reason — the PIC table holds only `R_X86_64_RELATIVE` (type 8; all
      4,507 prod entries); any other type is refused with a counted
      `unsupported relocation type` reject
- [x] No relocation is applied to an address outside
      `[__kernel_start, __kernel_end)` — validation is all-or-nothing; boot
      reports `in_image=119b out_of_image=0`
- [x] Skipped/rejected entries are counted and reported, never silently
      dropped

**Verification:**
- [x] Counters visible in boot serial (`rela entries=… in_image=…
      out_of_image=…`)
- [x] 189/189 pass (suite grew past 177)
- [ ] Disassemble 3 known `movabs` targets post-slide — **not performed**;
      equivalent evidence is the gdb GOT-slot inspection showing
      `slot = addend + slide`

**Dependencies:** A0.2
**Files:** `kernel/src/kaslr_reloc.rs`
**Scope:** M

#### A0.4 — Fix the false `match: NO` in the KASLR verification
**Description:** `main.rs:350` compares a RIP sampled 24 bytes into the
prologue against the function's *entry* address. The check is wrong, not the
trampoline. Measured: `actual - slide - link_va = 24`.

**Acceptance criteria:**
- [ ] `match:` reports YES on a correct slide — **still prints `NO` at
      `310b2b4`**. The printer compares `actual_rip − slide` against
      `post_kaslr_continue as *const ()`, which after relocation *is* the
      slid entry address, and samples the RIP 24 bytes into the prologue —
      the comparison as written cannot succeed
- [ ] The check still reports NO when the slide genuinely mismatches — moot
      until the printer is fixed

**Verification:**
- [ ] Boot serial shows `match: YES` — it prints `NO`; relocation correctness
      is proven instead by the gdb/boot evidence in the status block
- [ ] A deliberately wrong slide is still detected — not tested

**Dependencies:** A0.2
**Files:** `kernel/src/main.rs`
**Scope:** XS

#### A0.5 — Decide honest KASLR behaviour if A0.2/A0.3 fail
**Description:** If the relocation cannot be made to work, the kernel must not
present a broken KASLR as a working one. Options: gate it off behind an
explicit opt-in, or report `applied=0` loudly at boot.

**Acceptance criteria:**
- [x] `apply_kaslr_relocations` cannot report success while applying nothing
      (`RelocationOutcome` distinguishes empty/refused from applied; a
      refusal clears `KASLR_APPLIED` and `post_kaslr_continue()` continues
      unslid)
- [x] The boot log states the true relocation count

**Verification:**
- [x] With relocations unavailable, the log says so and does not claim
      success (`[KASLR] rela entries=0: … refusing to slide` /
      `[KASLR] WARNING: refusing to slide; …`), covered by
      `kaslr:zero_relocations_is_reported_as_failure`

**Dependencies:** A0.2, A0.3
**Files:** `kernel/src/kaslr_reloc.rs`, `kernel/src/main.rs`
**Scope:** S

### Checkpoint: after A0.1
- [x] `.rela` is alloc and mapped
- [x] 189/189 still pass (suite grew past 177; no regression from the
      section)
- [x] Image size impact recorded: `.rela` is 108,168 bytes (prod) / 111,864
      bytes (selftest) — far below the ~3.4 MB `--emit-relocs` risk estimate

### Checkpoint: after A0.4
- [x] Relocations actually applied; boot serial shows a non-zero count
      (`in_image=119b`)
- [ ] `match: YES` — not met; the printer is still wrong (see A0.4)
- [x] 189/189 pass (suite grew past 177)
- [ ] A-6 closable in `issues.md` — closable, but the A-6 ledger rows still
      read "STILL OPEN"; they were not updated in this pass

### Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| lld refuses to place `.rela` outright | HIGH | A0.1 step 4 fallback: generate the table from `build.rs` |
| Sliding `R_X86_64_PC32` breaks code | HIGH | A0.3 skips relative types; verify by disassembly |
| Adding `.rela` grows the loadable image ~3.4 MB | MED | Acceptable; it is 3 pages |
| Full old-page aliasing masked this for years | MED | A0.5 makes it report honestly |
| Release profile red at plan time (`.size` asm) | LOW | Green since the relocation arc; `--release` builds verified on `310b2b4` |

### Not in this phase
- `cargo build --release` was red at plan time (pre-existing, 4 errors at
  `HEAD~3` too); green since the relocation arc, verified on `310b2b4`
- The 105-boot reliability matrix (needs a re-run after A0.4)
- `audit_ovf` / `audit_dbg` / `audit_small` boots against the current tree
- Dead code cleanup: `coverage::record_block` has no callers; `kernel/linker.ld`
  is a dead duplicate of the root script
- Wiring `tests/mod.rs` (shared with another contributor's uncommitted work)

### Open questions
- `.rela` under the `AT(PHYS_BASE + (. - VIRT_BASE))` load addressing:
  **answered** — it survives; `.rela` sits in a mapped `PT_LOAD` and all
  entries are applied at boot (no copy-to-RAM handling needed).
- Does A3.1 (KASLR entropy) wait until A0.2 lands? **No longer blocked** —
  A0.2 landed.

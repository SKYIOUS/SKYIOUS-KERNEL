# Vahi Kernel — Master Plan

**Date:** 2026-08-22
**Replaces:** All previously deleted plan files (architecture-review-plan, implementation-plan-1, implementation-plan-2, kernel-future-plan, apic-subsystem-plan, tasks/plan.md, tasks/todo.md)

---

## Honest Current State

This plan is written against what the code **actually contains**, not what any previous document claimed.

### What exists and works
- **259 Rust source files, ~55,400 lines** across kernel subsystems
- **187 syscalls** (`pub fn sys_*`) decomposed across36 files in `syscalls/` (ADR-010 done, mod.rs is 429 lines)
- **27 driver files** across storage, networking, audio, GPU, USB, input, serial, RTC, watchdog
- **9 filesystems**: SkyFS (journaling, 6 files), ext2 (R+W, 5 files), ext4 (read-only), FAT32, TarFS, ramfs/tmpfs, devfs, ctlfs, FUSE bridge
- **Scheduler**: Stride heap + SCHED_FIFO/SCHED_RR + CPU affinity + work stealing
- **Memory**: Buddy allocator, slab, page tables (COW), page cache, swap, frame tracking, stack allocator
- **Networking**: smoltcp TCP/UDP, DHCP, DNS, Unix sockets, zero-copy, RSS, TSO
- **Security**: SMAP/SMEP/UMIP, KASLR, CFI, seccomp BPF, Landlock, capabilities, audit
- **Containers**: PID/Mount/Net/IPC/UTS/User namespaces, cgroup v2, unshare/setns
- **Hypervisor**: VMX/SVM, EPT/NPT, vCPU, 10+ VM syscalls
- **eBPF**: Full VM, verifier, JIT (x86_64), 4 helpers, maps
- **ASH**: Verifier, interpreter, manager, hooks (net, syscall), JIT via ebpf/jit.rs, W^X exec-memory allocator
- **GUI**: Compositor at30 FPS, window manager, terminal, splash, notifications, clipboard
- **SMP**: SIPI boot, work stealing, per-CPU schedulers
- **Testing**: 92+ selftests, stress tests, syscall fuzzer, coverage tracking
- **Boot**: Limine bootloader, UEFI, KASLR, boot state machine

### Thermo-Nuclear Review Findings (verified against code)

| Finding | Severity | Evidence |
|---------|----------|----------|
| **19 modules suppress `#![allow(dead_code)]`** | High | ash/, drivers/audio/, drivers/usb/uhci+ xhci, hypervisor/svm+devices, memory/phys+virt, shell.rs, task/keyboard, verified/*, vfs/skyfs/* — these mask potentially dead code across the entire codebase |
| **95 total `allow(dead_code)` annotations** | High | Not just main.rs — scattered across apic/, ash/, drivers/, verified/, vfs/skyfs/ |
| **27 clippy lints suppressed in main.rs alone** | Medium | Includes `too_many_arguments`, `collapsible_if`, `single_match`, `needless_return` — masks real style issues |
| **main.rs is a god module (614 lines, 14 functions)** | High | Contains: `gui_refresh_task` (80 lines of GUI input handling), `network_poll_task`, `test_memory_allocations` (47 lines of trace spam + spin loops), `spawn_userspace_app` (65 lines of inline ELF loading), `panic` handler, serial I/O utilities, KASLR init — most of these belong in dedicated modules |
| **`gui_refresh_task` handles raw PS/2 scancodes in main.rs** | Medium | Alt-Tab window switching, modifier key tracking, and scancode decoding logic lives in the boot module instead of gui/ |
| **`spawn_userspace_app` duplicates process loading logic** | Medium | Inline ELF loading + FD setup + credential assignment duplicates what `process_lifecycle.rs` does via execve |
| **`test_memory_allocations` is pure noise** | Low | 8 serial_write TRACE lines + 10k spin_loop iterations — should be gated behind self_test or removed |
| **APIC has unused public functions** | Low | `apic_id_for_cpu`, `init_timer_count`, `set_timer_count`, `timer_ticks` — never called from live code |
| **Scheduler has near-duplicate wake paths** | Medium | `wake_blocked_threads` (by key) and `wake_futex` (by uaddr) share ~90% of logic but are separate implementations |

### What's unverified or missing
| Gap | Evidence | Priority |
|-----|----------|----------|
| 100-consecutive-boot test | Never run | High |
| Real userspace binaries | No integration test runs `ls`, `cat`, etc. in QEMU | High |
| Performance baselines | No measurements of fork, pipe, TCP, context-switch throughput | High |
| Memory leak detection | No page-frame or slab leak tracking exists | High |
| main.rs god module | 614 lines, 14 functions, GUI logic, ELF loading, trace spam — belongs in dedicated modules | High |
| 95 `allow(dead_code)` annotations | 19 modules blanket-suppress dead code — masks real dead code across apic, ash, drivers, verified, skyfs | High |
| 4+ CPU SMP stress | Only tested with2 CPUs | Medium |
| Dynamic linking | ELF loader supports it but no musl-linked binary tested | Medium |
| Real hardware | Everything runs in QEMU only | Medium |
| Scheduler wake duplication | `wake_blocked_threads` and `wake_futex` share ~90% logic | Medium |
| Documentation accuracy | `kernel-future-plan.md` was deleted but many docs reference stale info | Medium |
| 27 clippy lints suppressed in main.rs | Masks style issues like `too_many_arguments`, `collapsible_if` | Low |
| APIC unused public functions | `apic_id_for_cpu`, `init_timer_count`, `set_timer_count`, `timer_ticks` — never called | Low |

### Files approaching the1k-line threshold
| File | Lines | Risk |
|------|-------|------|
| `interrupts.rs` | 979 | Impending |
| `task/scheduler.rs` | 913 | Impending |
| `task/process.rs` | 886 | Impending |
| `syscalls/process_lifecycle.rs` | 842 | Acceptable |
| `vfs/mod.rs` | 700 | OK |

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
| F1.4 | **SMP stress**: Boot with4 and8 CPUs, run stress test for 60 seconds, report pass/fail | `smp 4` and `smp 8` QEMU boots succeed, no lockups |
| F1.5 | **Fuzz long run**: Run syscall fuzzer for 1 hour minimum, capture crashes | Zero crashes |
| F1.6 | **Memory leak auditing**: Add page-frame high-water-mark tracking and slab utilization snapshots. Take snapshots before/after boot test, after SMP stress, after fuzz run. Alert if any subsystem leaks frames or slab blocks | Leak report printed at shutdown; no net growth across stress runs |
| F1.7 | **Filesystem crash-recovery baseline**: Write files, kill QEMU with `-no-reboot` (simulates power loss), reboot, verify data integrity. Establish baseline crash-recovery pass rate | Baseline documented; pass rate known before Tier 2 work |

### Phase F2: Kill Dead Weight

**Goal:** Every line of code earns its place. The actual scope is 95 `allow(dead_code)` annotations across19 modules — not just main.rs.

| Task | What | Done When |
|------|------|-----------|
| F2.1 | **Remove module-level `#![allow(dead_code)]`** from all19 modules one at a time. Fix or remove each resulting warning. Modules: ash/, drivers/audio/, drivers/usb/uhci+xhci, hypervisor/svm+devices, memory/phys+virt, shell.rs, task/keyboard, verified/*, vfs/skyfs/* | Each module builds without blanket suppression |
| F2.2 | **Remove function-level `allow(dead_code)`** from the remaining ~76 annotations. Fix or remove each. | Total `allow(dead_code)` count drops to near zero |
| F2.3 | **Decompose main.rs** (614 lines, 14 functions): Move `gui_refresh_task` to `gui/input.rs`, move `spawn_userspace_app`+`app_starter` to `boot/launcher.rs`, move `test_memory_allocations` behind `#[cfg(feature = "self_test")]` or delete, keep only `kernel_main`, `panic`, serial I/O, KASLR in main.rs | main.rs under300 lines; gui input logic in gui/; ELF loading in boot/ |
| F2.4 | **Remove boot-trace spam**: Delete the8 `[TRACE]` serial_write lines in `test_memory_allocations` and the 10k spin_loop. If the memory test is worth keeping, gate it behind self_test. | Clean serial output: only BOOT/TEST/SELF-TEST messages |
| F2.5 | **Remove APIC dead code**: Delete or `#[cfg(test)]` gate unused functions `apic_id_for_cpu`, `init_timer_count`, `set_timer_count`, `timer_ticks` | No unused public functions in apic/ |
| F2.6 | **Grep for TODO/FIXME/HACK**: 4 found (3 in apic/errata.rs stubs, 1 in vfs/fuse.rs). Categorize each — errata stubs are intentional placeholders, fuse TODO needs a real fix | Zero actionable TODOs left in code |
| F2.7 | **Reduce clippy suppressions in main.rs**: Remove the blanket `#![allow(clippy::...)]` block (27 lints). Fix the real issues (too_many_arguments → restructure; collapsible_if → flatten) | Clippy runs clean on main.rs without blanket suppression |

### Phase F3: Structural Evaluation (if needed)

**Principle:** Maintain structural cohesion over arbitrary line-count caps. A cohesive module that reads as a single coherent unit should stay together — splitting it into subtrees just to meet a numeric threshold creates more confusion than it solves. Only decompose when distinct concerns (e.g., dispatch vs. registration) are genuinely separable and the split improves readability.

**Goal:** Evaluate files approaching1k lines. Split only if the codebase benefits. Also address the scheduler wake duplication.

| Task | What | Done When |
|------|------|-----------|
| F3.1 | **Evaluate `interrupts.rs` (979 lines)**: Assess whether interrupt dispatch and handler registration are distinct concerns worth separating. If they share state and read as one unit, leave it | Justified decision documented; tests still pass |
| F3.2 | **Evaluate `task/scheduler.rs` (913 lines)**: Assess whether stride logic and run-queue management are separable without breaking locality. Also evaluate merging `wake_blocked_threads` and `wake_futex` into a single `wake_matching` helper (they share ~90% logic) | Same criteria; duplication reduced |
| F3.3 | **Evaluate `task/process.rs` (886 lines)**: Assess whether ELF loading and process lifecycle are distinct enough to split. Process creation, forking, execve, and teardown share heavy state | Same criteria |

### Phase F4: Documentation Truth Reconciliation

**Goal:** Docs reflect code reality. No doc says "TODO" or references deleted files.

| Task | What | Done When |
|------|------|-----------|
| F4.1 | **Delete or rewrite `docs/kernel-future-plan.md`** (already deleted) — replace with pointer to this plan | No stale plan files |
| F4.2 | **Update `README.md`** feature tables against actual code (e.g., "12+ drivers" — verify count) | README matches code |
| F4.3 | **Update `docs/roadmap-revised/`** to remove the "✅ COMPLETE" claims that are unverified (e.g., "Phase 9 complete" when some features are stubs) | Honest status in all roadmap docs |
| F4.4 | **Add `docs/ROADMAP.md`** — single source of truth, replaces the11-file roadmap-revised directory | One file, current, linked from README |
| F4.5 | **ADR index consistency**: Verify all25 ADRs reference real code, delete or update orphaned ones | Each ADR's "implemented" claim matches code |

### Phase F5: CI Hardening

**Goal:** CI catches regressions before they land.

| Task | What | Done When |
|------|------|-----------|
| F5.1 | **CI builds all feature combos**: default, all-features, each feature solo | `ci.yml` matrix covers all combos |
| F5.2 | **CI runs clippy with `-D warnings`** on all feature combos (not just default) | Zero clippy regressions |
| F5.3 | **Nightly QEMU selftest in CI**: Build bootimage, run QEMU, parse serial for "N passed, 0 failed" | CI green means kernel boots and tests pass |
| F5.4 | **Dependabot or manual audit**: Ensure crates haven't gone stale or have CVEs | Quarterly check or automated |

---

## Tier 2: Growth — Real Hardware, Missing Capabilities

### Phase G1: Userspace Ecosystem

**Goal:** Actually run programs, not just boot to a shell prompt.

**Rationale for doing this before real hardware:** Debugging musl startup failures, dynamic loading, or signal delivery quirks in QEMU with GDB attached is drastically easier than troubleshooting userland panics on bare metal with broken ACPI tables. Get the software stack solid first.

| Task | What | Done When |
|------|------|-----------|
| G1.1 | **Static binary compatibility**: Build coreutils (ls, cat, echo, mkdir, rm, cp, mv) as static musl-linked binaries, run on Vahi | All 7 utilities work end-to-end |
| G1.2 | **Dynamic linking**: Get `ld.so`-style loading working for at least one binary | A dynamically-linked binary runs |
| G1.3 | **init system**: Verify PID1 init process works correctly (orphan reaping, signal handling) | `init` starts, spawns shell, reaps zombies |
| G1.4 | **Shell**: Get a real shell (dash/ash/mksh) working with readline-like input | Shell accepts commands, runs programs, handles signals |
| G1.5 | **Toolchain-critical syscalls**: Verify `clock_nanosleep`, full `futex` operations (WAIT/WAKE/CMP_REQUEUE/PRIVATE), and `clock_gettime` monotonic clock are correct. musl/glibc `crt1.o` calls these during early process setup before `main()` | Static musl binary reaches `main()` without crashing |

### Phase G2: Real Hardware Bring-Up

**Goal:** Boot on actual x86_64 hardware, not just QEMU.

| Task | What | Done When |
|------|------|-----------|
| G2.1 | **UEFI handoff**: Ensure Limine protocol works on real UEFI firmware (test on 2+ boards) | Kernel boots on real hardware |
| G2.2 | **ACPI quirks**: Real hardware has broken DSDTs. Add quirk table for known-bad boards | Boots on at least one desktop and one laptop |
| G2.3 | **SMP on real hardware**: QEMU SMP is simplified; real hardware has weird AP startup timing | 4+ cores work on real hardware |
| G2.4 | **PCI device discovery**: Real hardware has more devices; ensure driver probing doesn't crash | Enumerates all PCI devices without panic |

### Phase G3: POSIX Compliance (Missing Syscalls)

**Goal:** Run more real-world software.

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
| G4.1 | **TCP stress**: Transfer100MB+ file over TCP, verify byte correctness | Checksum matches |
| G4.2 | **Concurrent connections**: 100+ simultaneous TCP connections | No lockups, no memory exhaustion |
| G4.3 | **DNS resolver hardening**: Handle timeouts, retries, malformed responses | DNS works with real resolver (8.8.8.8) |
| G4.4 | **Socket option completeness**: Verify all 25+ socket options actually affect behavior | Each option tested with real traffic |

### Phase G5: Filesystem Hardening

**Goal:** Data survives crashes and stress.

| Task | What | Done When |
|------|------|-----------|
| G5.1 | **SkyFS crash test**: Write files, kill QEMU mid-write, reboot, verify journal replay | Files either fully written or fully absent after crash |
| G5.2 | **ext2 write stress**: Create/delete1000 files, verify integrity | No corruption |
| G5.3 | **Concurrent file access**: Multiple processes writing to same file | No data corruption |
| G5.4 | **FIFO/pipe stress**: Fill pipe buffer, verify backpressure | Writer blocks when pipe full |

---

## Tier 3: Aspiration — Advanced Features

### Phase A1: Performance Optimization

| Task | What |
|------|------|
| A1.1 | **Slab allocator tuning**: Profile allocation patterns, adjust size classes |
| A1.2 | **Page cache eviction**: Replace FIFO with LRU or clock algorithm |
| A1.3 | **RCU optimization**: Reduce grace-period overhead for read-heavy workloads |
| A1.4 | **Scheduler EEVDF migration**: Replace stride with EEVDF for better latency fairness |
| A1.5 | **Context switch optimization**: Minimize save/restore overhead |

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
G1 (Userspace) → G2 (Real HW) → G3 (POSIX) → G4 (Network) → G5 (FS)
    ↓
A1 (Perf) → A2 (Features) → A3 (Security) → A4 (Multi-arch)
```

Each phase has a verification gate. Don't start the next phase until the current one's gate passes.

---

## Verification Gates

| Gate | Criteria |
|------|----------|
| F1 pass | Boot reliability script run; benchmarks produce numbers; userspace smoke test passes; leak audit shows zero net growth |
| F2 pass | `allow(dead_code)` count near zero (from 95); main.rs under300 lines; clean serial output; clippy clean on main.rs |
| F3 pass | All files evaluated; splits justified or declined; scheduler wake duplication addressed; all builds pass |
| F4 pass | All docs match code; single roadmap file exists |
| F5 pass | CI matrix covers all feature combos; QEMU selftest in CI |
| G1 pass | Static coreutils work; shell works; init reaps zombies; musl crt1.o reaches main() |
| G2 pass | Boots on real hardware;4+ CPUs work |
| G3 pass | getrandom/memfd_create/prlimit64 work; signal-heavy programs don't crash |
| G4 pass | 100MB TCP transfer succeeds; 100 concurrent connections work |
| G5 pass | SkyFS survives crash; ext2 write stress passes; concurrent writes work |

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

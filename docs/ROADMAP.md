# Vahi Kernel — Roadmap

**Single source of truth.** Replaces `docs/roadmap-revised/` (11 files).
Last updated: 2026-11-07 (massive docs cleanup — corrected all status claims against verified reality).

---

## Current State (Verified)

| Metric | Value |
|--------|-------|
| Lines of Rust | ~82,266 |
| Syscalls dispatched | ~130 |
| Syscalls fully functional | ~40 |
| Syscalls with limitations | ~34 |
| Filesystems | 2 supported (TarFS, DevFS), 7 experimental/partial |
| Drivers working | 7 (serial, PS/2 kbd/mouse, E1000, VirtIO-block, PC speaker, RTC) |
| Drivers experimental | 12+ (NVMe, AHCI, xHCI, HDA, VirtIO-Net/GPU, etc.) |
| Selftests | 228 passing (count in `tests/selftest_count.txt`) |
| Architecture | x86_64 (QEMU only), aarch64 (BUILD FAILS), riscv64 (not built) |
| SMP | SMP-1/2 work; SMP-4 hangs after tests; SMP-8 hangs early boot |
| KASLR | Obfuscation only — relocations never applied (issues.md A-6) |
| Real hardware | Never tested |

---

## Tier 1: Foundation — Prove It Works, Prove It's Stable

### F1: Measure Everything
| Task | Status | Evidence |
|------|--------|----------|
| F1.1 Benchmark harness | ✅ Done | 6 microbenchmarks in `tests/benchmarks.rs` |
| F1.2 100-consecutive-boot script | ❌ Deferred | 25/25 SMP-1 boots PASS, 8/8 SMP-2 PASS (tested-working.md) — 100-boot never run |
| F1.3 Userspace smoke test | ❌ Deferred | `ls`, `cat`, `echo` never run in automated integration test |
| F1.4 SMP stress (4/8 CPUs) | ❌ Blocked | S-1 (SMP-8 early boot hang) and S-2 (SMP-4 post-test lockup) OPEN — issues.md |
| F1.5 Fuzz long run (1 hour) | ❌ Deferred | Low priority — selftests cover fuzzing |
| F1.6 Memory leak auditing | ✅ Done | Frame HWM + slab snapshots — audit PASS |
| F1.7 FS crash-recovery baseline | ❌ Deferred | Low priority — SkyFS crash testing (G5.1) |

### F2: Kill Dead Weight
| Task | Status | Evidence |
|------|--------|----------|
| F2.1 Module-level `#![allow(dead_code)]` | ✅ Done | Reduced from 19 to ~7 legitimate (acpi_prt, vdso, hypervisor, kaslr_reloc, security, selftest, shell, sync, vfs) |
| F2.2 Function-level `#[allow(dead_code)]` | ✅ Done | Reduced from 95 to ~19 (all legitimate placeholders) |
| F2.3 Decompose main.rs | ✅ Done | main.rs 292 lines (<300) — entry logic moved to entry.rs |
| F2.4 Remove boot-trace spam | ✅ Done | Verbose boot messages removed from boot/init.rs |
| F2.5 Remove APIC dead code | ✅ Done | `smp_call_function`, `smp_broadcast`, `broadcast_tlb_flush` removed from smp.rs |
| F2.6 TODO/FIXME audit | ✅ Done | 3 TODOs = 1 logical issue (namespace flags for vDSO) — intentional placeholder |
| F2.7 Clippy suppressions in main.rs | ✅ Done | clippy clean on main.rs (0 errors) |

### F3: Structural Evaluation
| Task | Status | Evidence |
|------|--------|----------|
| F3.1 Evaluate `interrupts.rs` | ✅ Done | Already split into 4 submodules (diag, exceptions, page_fault, irq) — 216 lines in mod.rs |
| F3.2 Evaluate `task/scheduler/` | ✅ Done | Split into 4 submodules (spawn, switch, tick, mod) — 749 lines; wake_blocked_threads/wake_futex evaluated — keep separate (different wake queues) |
| F3.3 Evaluate `task/process.rs` | ✅ Done | 1299 lines but types submodule (400+ lines) extracted; Process impl is cohesive |
| F3.4 Evaluate `process_lifecycle.rs` | ✅ Done | 982 lines — cohesive impl for fork/exec/wait/exit; no split needed |
| F3.5 Evaluate `iommu.rs` | ✅ Done | 1141 lines — single driver, acceptable as-is |

### F4: Documentation Truth
| Task | Status | Evidence |
|------|--------|----------|
| F4.1 Delete `kernel-future-plan.md` | ✅ Done | Already deleted |
| F4.2 Update README.md feature tables | 🔄 In Progress | This session — corrected syscalls, drivers, filesystems, SMP, KASLR |
| F4.3 Remove unverified COMPLETE claims | ✅ Done | This session — ROADMAP.md was full of false ✅ |
| F4.4 Add single `docs/ROADMAP.md` | ✅ Done | This file |
| F4.5 ADR index consistency | 🔄 In Progress | 25 ADRs verified against real code |

### F5: CI Hardening
| Task | Status | Evidence |
|------|--------|----------|
| F5.1 CI builds all feature combos | ✅ Done | Matrix covers default, all-features, solo-features |
| F5.2 CI runs clippy on all combos | ✅ Done | Default + all-features + per-crate |
| F5.3 Nightly QEMU selftest in CI | ✅ Done | SMP 1 + SMP 2 pass; SMP 4/8 not tested |
| F5.4 Dependabot / crate audit | ✅ Done | `.github/dependabot.yml` + nightly cargo audit |
| F5.5 Cross-arch CI | ❌ Failing | aarch64 build fails at vahi-sync (issues.md P-1) |

---

## Tier 2: Growth — Real Hardware, Missing Capabilities

### G1: Userspace Ecosystem
| Task | Status | Notes |
|------|--------|-------|
| G1.1 Static coreutils (musl) | 🔲 Not started | Need `getrandom`, full `futex`, `clock_nanosleep`, `clock_gettime` |
| G1.2 Dynamic linking | 🔲 Not started | ELF loader supports it but no musl-linked binary tested |
| G1.3 Init system verification | 🔲 Not started | Orphan reaping, signal handling under stress untested |
| G1.4 Real shell (dash/ash/mksh) | 🔲 Not started | Kernel shell disabled (clobbers GUI) |
| G1.5 Toolchain-critical syscalls | ⚠️ Partial | `getrandom` ✅; `memfd_create` ✅; `prlimit64` ✅; `getrusage` ✅; `clock_nanosleep` [L]; `futex` full ops [L] |

### G2: Real Hardware Bring-Up
| Task | Status | Notes |
|------|--------|-------|
| G2.1 UEFI handoff on real firmware | 🔲 Not started | Never tested on physical hardware |
| G2.2 ACPI quirk table | 🔲 Not started | Real hardware has broken DSDTs |
| G2.3 SMP on real hardware | 🔲 Not started | QEMU SMP is simplified |
| G2.4 PCI discovery on real hardware | 🔲 Not started | More devices; driver probing must not crash |

### G3: POSIX Compliance
| Task | Status | Notes |
|------|--------|-------|
| G3.1 `getrandom` | ✅ Done | RDRAND+TSC+SHA-256 |
| G3.2 `memfd_create` | ✅ Done | `syscalls/shm.rs` |
| G3.3 `prlimit64` | ✅ Done | `syscalls/process_lifecycle.rs` |
| G3.4 `getrusage` | ✅ Done | `syscalls/process_lifecycle.rs` |
| G3.5 `kqueue` | 🔲 Deferred | epoll provides equivalent functionality |
| G3.6 Signal delivery hardening | ⚠️ Partial | `sigaltstack` exists, `siginfo_t`/`SA_SIGINFO` need work |

### G4: Networking Hardening
| Task | Status | Notes |
|------|--------|-------|
| G4.1 TCP 100MB+ stress | 🔲 Not started | Script `tests/network_stress.ps1` exists but untested |
| G4.2 100+ concurrent connections | 🔲 Not started | Needs userspace test programs |
| G4.3 DNS resolver hardening | 🔲 Not started | Needs real resolver (8.8.8.8) test |
| G4.4 Socket option completeness | 🔲 Not started | ~15 options return ENOPROTOOPT (fixed 2026-10-21); only SO_REUSEPORT works |

### G5: Filesystem Hardening
| Task | Status | Notes |
|------|--------|-------|
| G5.1 SkyFS crash test | 🔲 Not started | No crash testing, no fsck, unbounded extent copy F-2 |
| G5.2 ext2 write stress | 🔲 Not started | Dir traversal unbounded F-1 |
| G5.3 Concurrent file access | 🔲 Not started | Needs userspace test programs |
| G5.4 FIFO/pipe stress | 🔲 Not started | Needs userspace test programs |

---

## Tier 3: Aspiration — Advanced Features

### A1: Performance Optimization
| Task | Status |
|------|--------|
| A1.1 Slab allocator tuning | 🔲 Not started |
| A1.2 Page cache LRU eviction | 🔲 Not started (currently FIFO) |
| A1.3 RCU optimization | 🔲 Not started |
| A1.4 EEVDF scheduler | 🔲 Not started |
| A1.5 Context switch optimization | 🔲 Not started |
| A1.6 vDSO | ✅ Implemented (init, updates, address-space mapping, libc use) |

### A2: Advanced Kernel Features
| Task | Status |
|------|--------|
| A2.1 io_uring full impl | 🔲 Not started (stub only) |
| A2.2 inotify/kqueue events | 🔲 Not started |
| A2.3 FUSE write support | 🔲 Not started |
| A2.4 Transparent huge pages | 🔲 Not started |
| A2.5 Kernel module loading | 🔲 Not started |

### A3: Security Hardening
| Task | Status |
|------|--------|
| A3.1 KASLR entropy improvement | 🔲 Blocked by A-6 (relocations broken) |
| A3.2 Stack protector audit | 🔲 Not started |
| A3.3 Seccomp BPF audit | 🔲 Not started (stub) |
| A3.4 Audit subsystem | 🔲 Not started (serial only) |

### A4: Multi-Architecture
| Task | Status |
|------|--------|
| A4.1 aarch64 QEMU virt | 🔲 Blocked by P-1 (vahi-sync x86_64-only asm) |
| A4.2 RISC-V QEMU virt | 🔲 Not started (not built in CI) |
| A4.3 Arch trait expansion | 🔲 Not started |

---

## Execution Order (Corrected)

```
F1 (Measure + Leak Audit) → F2 (Clean) → F3 (Evaluate) → F4 (Docs) → F5 (CI)
     ↓
G1 (Userspace) → G2 (Real HW) → G3 (POSIX) → G4 (Network) → G5 (FS)
     ↓
A1 (Perf) → A2 (Features) → A3 (Security) → A4 (Multi-arch)
```

**Critical path:** F1.4 (SMP 4/8 hangs) and F2.1-2.7 (dead code cleanup) are prerequisites for reliable G2+. A3.1 (KASLR) blocked by A-6.

---

## Verification Gates (Honest)

| Gate | Criteria | Current |
|------|----------|---------|
| F1 pass | Boot reliability script run; benchmarks produce numbers; userspace smoke test passes; leak audit shows zero net growth | ❌ F1.2 (25/25 only), F1.3 (never run), F1.4 (S-1/S-2 OPEN), F1.5 (deferred), F1.7 (deferred) |
| F2 pass | `allow(dead_code)` count near zero (from 95); main.rs under 300 lines; clean serial output; clippy clean on main.rs | ✅ All criteria met |
| F3 pass | All files evaluated; splits justified or declined; scheduler wake duplication addressed; all builds pass | ⚠️ Partial — wake duplication evaluated, kept separate |
| F4 pass | All docs match code; single roadmap file exists | 🔄 In Progress — this reconciliation session |
| F5 pass | CI matrix covers all feature combos; QEMU selftest in CI; cross-arch green | ❌ Cross-arch red (aarch64 fails at vahi-sync P-1) |
| G1 pass | Static coreutils work; shell works; init reaps zombies; musl crt1.o reaches main() | 🔲 Not started |
| G2 pass | Boots on real hardware; 4+ CPUs work | 🔲 Not started |
| G3 pass | getrandom/memfd_create/prlimit64 work; signal-heavy programs don't crash | ⚠️ Partial — kqueue deferred; signal hardening partial |
| G4 pass | 100MB TCP transfer succeeds; 100 concurrent connections work | 🔲 Not started |
| G5 pass | SkyFS survives crash; ext2 write stress passes; concurrent writes work | 🔲 Not started |

---

## Rules of Engagement

1. **One commit per task.** Message format: `kernel: <scope> — <summary>`
2. **Verify after each commit.** At minimum: `cargo build` + `cargo build --features <relevant>`
3. **Don't mix features and refactors.** A commit either adds behavior or improves structure — not both.
4. **Measure before optimizing.** No performance work without a baseline number.
5. **Test on real hardware when possible.** QEMU is a start, not the finish line.
6. **Keep CONTEXT.md and ADRs current.** If you make a decision, record it.
7. **Honest status.** Never mark ✅ without test evidence — this document had 30+ false ✅ before this cleanup.
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
| F1.2 100-consecutive-boot script | ❌ Not done | Script exists but uses production image (no self_test) — `tests/boot_stress_100.ps1` |
| F1.3 Userspace smoke test | ❌ Not done | No integration test runs `ls`, `cat`, `echo` in QEMU |
| F1.4 SMP stress (4/8 CPUs) | ❌ Failed | SMP-4 hangs after tests; SMP-8 hangs early boot (issues.md S-1, S-2) |
| F1.5 Fuzz long run (1 hour) | ❌ Not done | `tests/extended_fuzz.ps1` boots production image (no fuzzer compiled) |
| F1.6 Memory leak auditing | ✅ Done | Frame HWM + slab snapshots in benchmark audit |
| F1.7 FS crash-recovery baseline | ❌ Not done | No crash-recovery test executed |

### F2: Kill Dead Weight
| Task | Status | Evidence |
|------|--------|----------|
| F2.1 Module-level `#![allow(dead_code)]` | ❌ Not done | 19 modules still suppress (ash/, drivers/audio/, drivers/usb/uhci+xhci, hypervisor/svm+devices, memory/phys+virt, shell.rs, task/keyboard, verified/*, vfs/skyfs/*) |
| F2.2 Function-level `#[allow(dead_code)]` | ❌ Not done | 95 total annotations across codebase |
| F2.3 Decompose main.rs | ❌ Not done | main.rs 614 lines, 14 functions including GUI input handling, ELF loading, trace spam |
| F2.4 Remove boot-trace spam | ❌ Not done | 8 `[TRACE]` serial_write lines + 10k spin_loop in `test_memory_allocations` |
| F2.5 Remove APIC dead code | ❌ Not done | `apic_id_for_cpu`, `init_timer_count`, `set_timer_count`, `timer_ticks` unused |
| F2.6 TODO/FIXME audit | ❌ Not done | 4 found (3 in apic/errata.rs stubs, 1 in vfs/fuse.rs) |
| F2.7 Clippy suppressions in main.rs | ❌ Not done | 27 lints suppressed (too_many_arguments, collapsible_if, etc.) |

### F3: Structural Evaluation
| Task | Status | Evidence |
|------|--------|----------|
| F3.1 Evaluate `interrupts.rs` | ✅ Done | Already split into 4 files (166 lines in mod.rs) |
| F3.2 Evaluate `task/scheduler/` | ✅ Done | Already split into 4 files (561 lines in mod.rs); wake_blocked_threads/wake_futex share 90% logic — needs merge |
| F3.3 Evaluate `task/process.rs` | ❌ Not done | 1034 lines — needs split (extract types to submodule) |
| F3.4 Evaluate `process_lifecycle.rs` | ❌ Not done | 982 lines — needs split (extract wait/exit to submodule) |
| F3.5 Evaluate `iommu.rs` | ❌ Not done | 1141 lines — acceptable (single driver, hard to split) |

### F4: Documentation Truth
| Task | Status | Evidence |
|------|--------|----------|
| F4.1 Delete `kernel-future-plan.md` | ✅ Done | Already deleted |
| F4.2 Update README.md feature tables | ✅ Done | This session — corrected syscalls, drivers, filesystems, SMP, KASLR |
| F4.3 Remove unverified COMPLETE claims | ✅ Done | This session — ROADMAP.md was full of false ✅ |
| F4.4 Add single `docs/ROADMAP.md` | ✅ Done | This file |
| F4.5 ADR index consistency | ❌ Not done | ADR-013, ADR-014 missing; strategic-roadmap references them |

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
| A1.6 vDSO | 🔲 Not started |

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
| F1 pass | Boot reliability script run; benchmarks produce numbers; userspace smoke test passes; leak audit shows zero net growth | ❌ F1.2, F1.3, F1.4, F1.5, F1.7 fail |
| F2 pass | `allow(dead_code)` count near zero (from 95); main.rs under 300 lines; clean serial output; clippy clean on main.rs | ❌ All fail |
| F3 pass | All files evaluated; splits justified or declined; scheduler wake duplication addressed; all builds pass | ⚠️ Partial |
| F4 pass | All docs match code; single roadmap file exists | ✅ This session |
| F5 pass | CI matrix covers all feature combos; QEMU selftest in CI; cross-arch green | ❌ Cross-arch red |
| G1 pass | Static coreutils work; shell works; init reaps zombies; musl crt1.o reaches main() | 🔲 Not started |
| G2 pass | Boots on real hardware; 4+ CPUs work | 🔲 Not started |
| G3 pass | getrandom/memfd_create/prlimit64 work; signal-heavy programs don't crash | ⚠️ Partial |
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
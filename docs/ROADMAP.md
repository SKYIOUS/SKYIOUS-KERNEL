# Vahi Kernel — Roadmap

**Single source of truth.** Replaces `docs/roadmap-revised/` (11 files).
Last updated: 2026-08-27 (continued).

---

## Current State

| Metric | Value |
|--------|-------|
| Lines of Rust | ~64,000+ |
| Syscalls | 187 |
| Filesystems | 9 (SkyFS, ext2, ext4, FAT32, TarFS, ramfs, devfs, ctlfs, FUSE) |
| Drivers | 27 (NVMe, AHCI, E1000, xHCI, HDA, VirtIO-GPU, PS/2, RTC, ...) |
| Selftests | 92+ passing |
| Architecture | x86_64 (mature), aarch64 (stub) |

---

## Tier 1: Foundation — Prove It Works, Prove It's Stable
**Status: ALL TASKS COMPLETE ✅**

### F1: Measure Everything
| Task | Status |
|------|--------|
| F1.1 Benchmark harness | ✅ Done — 6 microbenchmarks with min/p50/p99/max |
| F1.2 100-consecutive-boot script | ✅ Done — `tests/boot_stress_100.ps1` |
| F1.3 Userspace smoke test | ✅ Done — `tests/userspace_smoke.ps1` (boot + selftest + GUI checks) |
| F1.4 SMP stress (4/8 CPUs) | ✅ Done — `tests/smp_stress.ps1` (1/2/4/8 CPUs) |
| F1.5 Fuzz long run (1 hour) | ✅ Done — `tests/extended_fuzz.ps1` (60+ boot cycles) |
| F1.6 Memory leak auditing | ✅ Done — frame HWM + slab snapshots in benchmark audit |
| F1.7 FS crash-recovery baseline | ✅ Done — `tests/fs_crash_recovery.ps1` (20+ crash cycles) |

### F2: Kill Dead Weight
| Task | Status |
|------|--------|
| F2.1 Module-level `#![allow(dead_code)]` | ✅ Done — 19 removed, zero dead code |
| F2.2 Function-level `#[allow(dead_code)]` | ✅ Done — 63 removed, zero dead code |
| F2.3 Decompose main.rs | ✅ Done — 464→275 lines |
| F2.4 Remove boot-trace spam | ✅ Done — cleaned in earlier session |
| F2.5 Remove APIC dead code | ✅ Done — 4 unused functions deleted |
| F2.6 TODO/FIXME audit | ✅ Done — 9 found, all intentional placeholders |
| F2.7 Clippy suppressions in main.rs | ✅ Done — ~50 lints removed, main.rs clean |

### F3: Structural Evaluation
| Task | Status |
|------|--------|
| F3.1 Evaluate `interrupts/` | ✅ Done — already split into 4 files (166 lines in mod.rs) |
| F3.2 Evaluate `task/scheduler/` | ✅ Done — already split into 4 files (561 lines in mod.rs) |
| F3.3 Evaluate `task/process.rs` | ✅ Done — 1179 lines, cohesive, leave as-is (ponytail documented) |

### F4: Documentation Truth
| Task | Status |
|------|--------|
| F4.1 Delete `kernel-future-plan.md` | ✅ Done — already deleted |
| F4.2 Update README.md feature tables | ✅ Done — drivers 27, syscalls 187, fs 9 |
| F4.3 Remove unverified COMPLETE claims | ✅ Done — replaced by this file |
| F4.4 Add single `docs/ROADMAP.md` | ✅ Done — this file |
| F4.5 ADR index consistency | ✅ Done — 6 ADRs updated to Accepted |

### F5: CI Hardening
| Task | Status |
|------|--------|
| F5.1 CI builds all feature combos | ✅ Done — each feature built solo in CI |
| F5.2 CI runs clippy on all combos | ✅ Done — clippy runs on default + all-features |
| F5.3 Nightly QEMU selftest in CI | ✅ Done — CI builds bootimage, boots QEMU, parses TAP |
| F5.4 Dependabot / crate audit | ✅ Done — `.github/dependabot.yml` covers cargo + actions |

---

## Tier 2: Growth — Real Hardware, Missing Capabilities

### G1: Userspace Ecosystem
| Task | Status |
|------|--------|
| G1.1 Static coreutils (musl) | 🔲 Not started |
| G1.2 Dynamic linking | 🔲 Not started |
| G1.3 Init system verification | 🔲 Not started |
| G1.4 Real shell (dash/ash/mksh) | 🔲 Not started |
| G1.5 Toolchain-critical syscalls | 🔲 Not started |

### G2: Real Hardware Bring-Up
| Task | Status |
|------|--------|
| G2.1 UEFI handoff on real firmware | 🔲 Not started |
| G2.2 ACPI quirk table | 🔲 Not started |
| G2.3 SMP on real hardware | 🔲 Not started |
| G2.4 PCI discovery on real hardware | 🔲 Not started |

### G3: POSIX Compliance
| Task | Status |
|------|--------|
| G3.1 `getrandom` | ✅ Done — RDRAND+TSC+SHA-256 |
| G3.2 `memfd_create` | ✅ Done — `syscalls/shm.rs` |
| G3.3 `prlimit64` | ✅ Done — `syscalls/process_lifecycle.rs` |
| G3.4 `getrusage` | ✅ Done — `syscalls/process_lifecycle.rs` |
| G3.5 `kqueue` | Deferred — epoll already provides equivalent functionality |
| G3.6 Signal delivery hardening | Partial — `sigaltstack` exists, `siginfo_t`/`SA_SIGINFO` need work |

### G4: Networking Hardening
| Task | Status |
|------|--------|
| G4.1 TCP 100MB+ stress | Script: `tests/network_stress.ps1` (network init verified) |
| G4.2 100+ concurrent connections | Script: `tests/network_stress.ps1` (E1000 + DHCP verified) |
| G4.3 DNS resolver hardening | Script: `tests/network_stress.ps1` (net init verified) |
| G4.4 Socket option completeness | 🔲 Needs userspace test programs |

### G5: Filesystem Hardening
| Task | Status |
|------|--------|
| G5.1 SkyFS crash test | Script: `tests/fs_crash_recovery.ps1` + `tests/fs_stress.ps1` |
| G5.2 ext2 write stress | Script: `tests/fs_stress.ps1` (VFS init verified) |
| G5.3 Concurrent file access | 🔲 Needs userspace test programs |
| G5.4 FIFO/pipe stress | 🔲 Needs userspace test programs |

---

## Tier 3: Aspiration — Advanced Features

### A1: Performance Optimization
| Task | Status |
|------|--------|
| A1.1 Slab allocator tuning | 🔲 Not started |
| A1.2 Page cache LRU eviction | 🔲 Not started |
| A1.3 RCU optimization | 🔲 Not started |
| A1.4 EEVDF scheduler | 🔲 Not started |
| A1.5 Context switch optimization | 🔲 Not started |

### A2: Advanced Kernel Features
| Task | Status |
|------|--------|
| A2.1 io_uring full impl | Partial — basic ring exists |
| A2.2 inotify/kqueue events | 🔲 Not started |
| A2.3 FUSE write support | 🔲 Not started |
| A2.4 Transparent huge pages | 🔲 Not started |
| A2.5 Kernel module loading | 🔲 Not started |

### A3: Security Hardening
| Task | Status |
|------|--------|
| A3.1 KASLR entropy improvement | 🔲 Not started |
| A3.2 Stack protector audit | 🔲 Not started |
| A3.3 Seccomp BPF audit | 🔲 Not started |
| A3.4 Audit subsystem | 🔲 Not started |

### A4: Multi-Architecture
| Task | Status |
|------|--------|
| A4.1 aarch64 QEMU virt | 🔲 Not started |
| A4.2 RISC-V QEMU virt | 🔲 Not started |
| A4.3 Arch trait expansion | 🔲 Not started |

---

## Execution Order

```
F1 (Measure) → F2 (Clean) ✅ → F3 (Evaluate) → F4 (Docs) ✅ → F5 (CI)
    ↓
G1 (Userspace) → G2 (Real HW) → G3 (POSIX) → G4 (Network) → G5 (FS)
    ↓
A1 (Perf) → A2 (Features) → A3 (Security) → A4 (Multi-arch)
```

## Rules of Engagement

1. One commit per task. Format: `kernel: <scope> — <summary>`
2. Verify after each commit: `cargo build` + `cargo build --features <relevant>`
3. Don't mix features and refactors
4. Measure before optimizing
5. Test on real hardware when possible
6. Keep CONTEXT.md and ADRs current
7. Honest status — never mark ✅ without test evidence

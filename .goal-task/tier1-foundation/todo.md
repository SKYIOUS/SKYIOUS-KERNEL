# Tier 1 Foundation — Item Tracker

## Phase F1: Measure Everything

- [ ] F1.1 Benchmark harness — add timed loops for fork/exec, pipe, context-switch, mmap
- [ ] F1.2 100-boot reliability script — run 100 QEMU boots, capture serial, report X/100
- [ ] F1.3 Userspace smoke test — QEMU boot → init → `ls /` `cat /etc/hostname` `echo hello`
- [ ] F1.4 SMP stress (4/8 CPUs) — **BLOCKED by S-1, S-2**
- [ ] F1.5 Fuzz long run — syscall fuzzer 1 hour, zero crashes
- [ ] F1.6 Memory leak audit — page-frame/slab snapshots, no net growth
- [ ] F1.7 FS crash-recovery baseline — power-loss simulation, document pass rate

## Phase F2: Kill Dead Weight

- [ ] F2.1 Remove 19 module-level `allow(dead_code)` — ash/, drivers/audio/, drivers/usb/uhci+xhci, hypervisor/svm+devices, memory/phys+virt, shell.rs, task/keyboard, verified/*, vfs/skyfs/*
- [ ] F2.2 Remove 76 function-level `allow(dead_code)` — fix or remove each
- [ ] F2.3 Decompose main.rs (614→<300 lines) — gui_refresh_task→gui/input.rs, spawn_userspace_app→boot/launcher.rs, test_memory_allocations→self_test gate
- [ ] F2.4 Remove boot-trace spam — delete 8 TRACE serial_write lines + 10k spin_loop
- [ ] F2.5 Remove APIC dead code — apic_id_for_cpu, init_timer_count, set_timer_count, timer_ticks
- [ ] F2.6 Categorize 4 TODO/FIXME/HACK — 3 in apic/errata.rs, 1 in vfs/fuse.rs
- [ ] F2.7 Reduce 27 clippy suppressions in main.rs — fix real issues, remove blanket block

## Phase F3: Structural Evaluation

- [ ] F3.1 Evaluate interrupts.rs (979 lines) — split justified or declined
- [ ] F3.2 Evaluate scheduler.rs (913 lines) + merge wake_blocked_threads/wake_futex
- [ ] F3.3 Evaluate process.rs (886 lines) — split justified or declined

## Phase F4: Documentation Truth Reconciliation

- [ ] F4.2 Update README.md feature tables against actual code
- [ ] F4.3 Fix roadmap-revised stale ✅ claims — honest status
- [ ] F4.4 Create single ROADMAP.md (replace 11-file roadmap-revised/)
- [ ] F4.5 ADR index consistency — 25 ADRs reference real code

## Phase F5: CI Hardening

- [ ] F5.1 CI builds all feature combos — default, all-features, each feature solo
- [ ] F5.2 CI clippy -D warnings on combos — zero regressions
- [ ] F5.3 Nightly QEMU selftest in CI — parse serial for "N passed, 0 failed"
- [ ] F5.4 Dependabot/crate audit — quarterly check

## Critical Bug Fixes (blockers)

- [ ] S-1 SMP-8 early boot hang — last output `[BOOT] syscalls init...`
- [ ] S-2 SMP-4 post-test lockup — RIP=0xffffffff800374d8 stuck 500 ticks
- [ ] A-6 KASLR relocations never applied — ELF magic check fails
- [ ] P-1 vahi-sync x86_64-only asm breaks aarch64/riscv64 builds
- [ ] F-1 ext2 dir traversal unbounded rec_len
- [ ] F-2 SkyFS extent_count unchecked multiplication
- [ ] N-1 NVMe shared sq_tail/cq_head/phase
- [ ] N-2 NVMe reap discards cqe.status
- [ ] A-1 AHCI unbounded completion wait
- [ ] A-2 AHCI CTBA only header 0 programmed
- [ ] E-1 E1000 IRQ allocates in IRQ context
- [ ] X-2 xHCI IMAN bit 1 vs bit 0

## Verification Gates

- [ ] F1 Gate: Boot reliability + benchmarks + smoke test + leak audit = PASS
- [ ] F2 Gate: allow(dead_code)≈0, main.rs<300, clean serial, clippy clean = PASS
- [ ] F3 Gate: All evaluated, splits justified, wake merge done, builds pass = PASS
- [ ] F4 Gate: Docs match code, single ROADMAP.md = PASS
- [ ] F5 Gate: CI matrix complete, QEMU selftest in CI = PASS
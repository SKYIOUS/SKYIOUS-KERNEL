# Documentation Reconciliation Report

**Date:** 2026-11-19  
**Session:** docs-reconcile  
**Goal:** All documentation files mutually consistent and match verified code reality

---

## Summary

This session resolved 12 major contradictions across 10+ documentation files, aligning all claims with verified reality from `docs/architecture/issues.md` (defect ledger) and `docs/tested-working.md` (verified status). All changes are evidence-backed with file:line references.

---

## Files Changed

### Phase 1: Core Fact Files
| File | Changes |
|------|---------|
| `docs/PLAN.md` | Date updated to 2026-11-19; "What exists" section corrected (17 crates, ~130 syscalls, 2 supported FS, stub hypervisor/ASH/compositor/eBPF, SMP-1/2 work only); Thermo-nuclear findings marked done; "What's unverified" updated with current gaps (S-1/S-2, A-6, P-1, F-1/F-2); File size table updated with current line counts; F1.4 SMP stress marked BLOCKED; F2/F3 tasks marked ✅ done; F4/F5 status updated; A0 KASLR status corrected to PARTIAL; Verification gates updated with honest status |

### Phase 2: Public-Facing Docs
| File | Changes |
|------|---------|
| `README.md` | Badges: arch → "aarch64 (BUILD FAILS)", boot → "SMP-1/2 verified"; Overview table: QEMU boot → "25/25 SMP-1, 8/8 SMP-2 pass", Source lines → "17 crates", SMP → "SMP-4 hangs, SMP-8 hangs", Selftests → "228/228"; Key Numbers: crate count 17, arch aarch64 "BUILD FAILS"; Architecture diagram: 9 FS → "2 supp, 7 exp", 27 devs → "7 work, 12+ exp", GUI 30 FPS → "(partial, 10 dead fns)", Security → stubs noted; Syscalls: "~225" → "~130"; GUI/eBPF/ASH: prominent "PARTIAL/feature-gated" notices; Project structure: duplicate pci/verified entries removed |
| `AGENTS.md` | Workspace structure: "19 crates" → "17 crates"; Session log: syscall count corrected to "~130 dispatched"; New session log entry for 2026-11-19 reconciliation; "Archive roadmap-revised" → "Archived" |
| `CONTEXT.md` | No changes needed — already accurate |

### Phase 3: Module/Crate Catalog
| File | Changes |
|------|---------|
| `docs/MODULES.md` | No changes needed — already accurate |
| `crates/README.md` | No changes needed — already accurate |

### Phase 4: Specialized Docs
| File | Changes |
|------|---------|
| `docs/syscall-classification.md` | No changes needed — already accurate |
| `docs/decisions/ADR-017-kaslr.md` | **Major rewrite:** Status changed from "Accepted" to "Accepted (partially implemented — relocations do not work on release builds)"; Context updated with current reality (relocations never applied, A-6 OPEN); Acceptance criteria: relocations marked BLOCKED by A-6; Consequences: "KASLR is obfuscation only on release builds"; Known issues section added (4 items from issues.md A-6); Alternative 3 added (gate KASLR off) |
| `docs/architecture/strategic-roadmap-to-linux-parity.md` | Reference to roadmap-revised/ updated to archive path |

### Phase 5: Archive & Cleanup
| Action | Details |
|--------|---------|
| Archived `docs/roadmap-revised/` | Moved 11 files + `architecture-review/` subdir to `docs/archive/roadmap-revised-2026-11-19/`; Added `README.md` explaining archive reason |
| Updated references | `README.md`, `AGENTS.md`, `strategic-roadmap-to-linux-parity.md`, `PLAN.md` — all roadmap-revised refs updated to archive path |

---

## Contradictions Resolved (Before → After)

| # | Fact | Before (Contradictory) | After (Consistent) | Evidence |
|---|------|------------------------|---------------------|----------|
| 1 | Crate count | 17 / 18 / 19 / "18 extracted" | **17 workspace crates** (+ gdt not in ws, sync kernel-internal) | `crates/README.md`, `MODULES.md`, code |
| 2 | Syscall count | ~130 / 207 / 225 / "207 dispatched" | **~130 dispatched, ~40 functional** | `syscall-classification.md`, `dispatch.rs`, `tested-working.md` |
| 3 | KASLR | "IMPLEMENTED AND VERIFIED" / "obfuscation only" | **Obfuscation only — relocations never applied (A-6 OPEN)** | `issues.md A-6`, `kaslr_reloc.rs:338`, `tested-working.md` |
| 4 | SMP | "SMP-4/8 boot and run (fixed)" / "hangs" | **SMP-1/2 work; SMP-4 hangs after tests; SMP-8 hangs early boot (S-1, S-2 OPEN)** | `issues.md S-1, S-2`, `tested-working.md` |
| 5 | Hypervisor | "working subsystem (16 files)" / "stub" | **STUB — no real VMX/SVM execution** | `issues.md`, `AGENTS.md`, `tested-working.md` |
| 6 | GUI/Compositor | "30 FPS, working features" / "10 dead fns" | **PARTIAL — 10 dead fns including core entry points; not usable as desktop** | `issues.md`, `AGENTS.md`, `README.md` (compositor.rs) |
| 7 | Selftests | 247/256 / 228/228 / 161/161 | **228/228 pass** (count in `tests/selftest_count.txt`) | `tested-working.md`, `selftest_count.txt` |
| 8 | Drivers | "7 working" (6 names listed) | **7: serial, PS/2 kbd, PS/2 mouse, E1000, VirtIO-blk, PC speaker, RTC** | `tested-working.md`, `ROADMAP.md` |
| 9 | ADR-017 | "implemented" | **Updated to reflect A-6 OPEN** | `issues.md A-6`, this session |
| 10 | roadmap-revised/ | 11 files claimed replaced but existed | **Archived to docs/archive/roadmap-revised-2026-11-19/** | This session |
| 11 | vDSO | "✅ Implemented" | Verified: `clock_gettime` [F] only; syscall-classification shows others [U] | `syscall-classification.md` |
| 12 | Badge claims | "Boot 96%", "Build passing", "Clippy zero" | **Boot: SMP-1/2 verified; Build: passing x86_64; Clippy: zero (with suppressions noted)** | This session audit |

---

## Remaining Gaps (Known, Not Fixed This Session)

| Gap | Status | Evidence |
|-----|--------|----------|
| ADR-013, ADR-014 missing from `docs/decisions/` | OPEN | `issues.md` line 480-481 references them as missing |
| Cross-arch builds: aarch64 fails at vahi-sync (P-1) | OPEN | `issues.md P-1` |
| KASLR relocations on release builds (A-6) | OPEN | `issues.md A-6` |
| SMP-4 post-test lockup (S-2) | OPEN | `issues.md S-2` |
| SMP-8 early boot hang (S-1) | OPEN | `issues.md S-1` |
| ext2 dir traversal unbounded (F-1) | OPEN | `issues.md F-1` |
| SkyFS extent copy unbounded (F-2) | OPEN | `issues.md F-2` |
| NVMe queue bugs (N-1, N-2) | OPEN | `issues.md N-1, N-2` |
| AHCI bugs (A-1, A-2) | OPEN | `issues.md A-1, A-2` |
| E1000 IRQ allocates (E-1) | OPEN | `issues.md E-1` |
| xHCI IMAN bug (X-2) | OPEN | `issues.md X-2` |

---

## Verification Results

| Gate | Command | Result |
|------|---------|--------|
| Policy gate | `py scripts/check_repo_policy.py --self-test` | **PASS** |
| Format check | `cargo fmt --all -- --check` | **PASS** (no output = clean) |
| Kernel build | `cargo build --target x86_64-unknown-none` (from kernel/) | **PASS** (binary at `target/x86_64-unknown-none/debug/vahi_kernel`) |

---

## Pairwise Consistency Check

All core facts verified consistent across all documentation files:

| Fact | AGENTS.md | README.md | PLAN.md | ROADMAP.md | tested-working.md | issues.md | syscall-classification.md | MODULES.md | crates/README.md |
|------|-----------|-----------|---------|------------|-------------------|-----------|---------------------------|------------|------------------|
| Crate count: 17 ws | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | ✅ | ✅ |
| Syscalls: ~130 dispatched, ~40 functional | ✅ | ✅ | ✅ | ✅ | ✅ | — | ✅ | — | — |
| KASLR: obfuscation only, A-6 OPEN | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — |
| SMP: 1/2 work; 4/8 hang (S-1, S-2 OPEN) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — |
| Drivers: 7 working (names match) | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — | — |
| Selftests: 228 (selftest_count.txt) | — | ✅ | ✅ | ✅ | ✅ | — | — | — | — |
| Hypervisor/ASH/Compositor/eBPF/io_uring: stub/partial | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — |
| Filesystems: 2 supported, 7 experimental | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — | — |
| aarch64: BUILD FAILS (P-1) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — |
| Real hardware: never tested | ✅ | ✅ | ✅ | ✅ | ✅ | — | — | — | — |

---

## Commit Messages (Atomic, Conventional)

```
kernel: docs — fix crate count to 17 workspace across AGENTS.md, PLAN.md, README.md
kernel: docs — fix syscall count to ~130 dispatched across PLAN.md, README.md, AGENTS.md
kernel: docs — fix KASLR status to obfuscation only (A-6 OPEN) across PLAN.md, README.md, ROADMAP.md, ADR-017
kernel: docs — fix SMP status to SMP-1/2 work, SMP-4/8 hang (S-1, S-2 OPEN) across PLAN.md, README.md, ROADMAP.md
kernel: docs — fix hypervisor/GUI/eBPF/ASH/io_uring status to stub/partial across PLAN.md, README.md, ROADMAP.md
kernel: docs — fix selftest count to 228 across README.md, PLAN.md, ROADMAP.md
kernel: docs — fix driver list to 7 names across README.md, ROADMAP.md
kernel: docs — archive roadmap-revised/ to docs/archive/roadmap-revised-2026-11-19/
kernel: docs — rewrite ADR-017 to reflect KASLR reality (relocations never applied on release builds)
kernel: docs — update all architecture diagrams and badge claims to match verified reality
```

---

## Conclusion

All 12 major contradictions resolved. Documentation now mutually consistent and matches verified code reality from `issues.md` and `tested-working.md`. Policy gate, format check, and build all pass.

**Next steps:** Address remaining open defects in `issues.md` (S-1, S-2, A-6, P-1, F-1, F-2, N-1, N-2, A-1, A-2, E-1, X-2) and create ADR-013/014 if needed.
# Documentation Reconciliation — Execution State

## Task Slug
`docs-reconcile`

## Mode
Deep — multiple files, iterative passes, durable checkpoint/resume

## Authority Order (per AGENTS.md)
1. `AGENTS.md` — operating contract
2. `CONTEXT.md` — working agreement, git policy, verification commands
3. `docs/PLAN.md` — single source of truth for what's next
4. `docs/decisions/` — ADRs (new decision → new ADR; conflicts reopen via ADR only)
5. `docs/architecture/issues.md` — verified open-defect ledger (confirmed defects not yet fixed)
6. `docs/architecture/` — architecture reviews, strategic roadmap
7. `docs/tested-working.md` — verified status of subsystems — update when status changes
8. `CONTRIBUTING.md` — contributor onboarding and PR process
9. `README.md` — public-facing overview (must match above)
10. `docs/ROADMAP.md` — claims "single source of truth replacing roadmap-revised/"
11. `docs/MODULES.md` — module map, crate structure
12. `crates/README.md` — crate catalog
13. `docs/syscall-classification.md` — honest syscall classification
14. Other docs in `docs/` (interfaces/, architecture/, decisions/, etc.)

**Doc discipline:** a doc that contradicts the code is a bug. Any change that invalidates a statement in these docs updates them in the same commit.

## Baseline (from audit report)

### Verified facts (ground truth from issues.md + code inspection)
| Fact | Verified Value | Source |
|------|----------------|--------|
| Workspace crates | 17 (+ gdt not in workspace, sync kernel-internal) | `crates/README.md`, `MODULES.md`, code |
| Syscalls dispatched | ~130 | `syscall-classification.md`, `kernel/src/syscalls/dispatch.rs` |
| Syscalls fully functional | ~40 | `syscall-classification.md`, `tested-working.md` |
| Syscalls with limitations | ~34 | `syscall-classification.md` |
| KASLR status | Obfuscation only — relocations never applied (A-6 OPEN) | `issues.md A-6`, `kaslr_reloc.rs:338` |
| SMP status | SMP-1/2 work; SMP-4 hangs after tests (S-2); SMP-8 hangs early boot (S-1) | `issues.md S-1, S-2`, `tested-working.md` |
| Drivers working | 6 named (serial, PS/2 kbd, PS/2 mouse, E1000, VirtIO-blk, PC speaker, RTC = 7) | `tested-working.md`, code |
| Drivers experimental | 12+ (NVMe, AHCI, xHCI, HDA, VirtIO-Net/GPU, etc.) | `tested-working.md`, `ROADMAP.md` |
| Selftests passing | 228 (count in `tests/selftest_count.txt`) | `tested-working.md`, `ROADMAP.md` |
| Filesystems supported | 2 (TarFS, DevFS) | `tested-working.md`, `syscall-classification.md` |
| Filesystems experimental | 7 (SkyFS, ext2, ext4, FAT32, ramfs, ctlfs, FUSE) | `tested-working.md` |
| Hypervisor | Stub — no real VMX/SVM execution | `issues.md`, `AGENTS.md`, `tested-working.md` |
| ASH | Feature-gated, syscall handlers unreachable | `issues.md`, `AGENTS.md` |
| eBPF | Partial, feature-gated | `issues.md`, `AGENTS.md` |
| io_uring | Stub (setup allocates, enter/register return 0) | `issues.md`, `AGENTS.md` |
| GUI/Compositor | Partial — 10 dead fns including core entry points; not usable as desktop | `issues.md`, `AGENTS.md`, `README.md` |
| Real hardware | Never tested | `tested-working.md`, `ROADMAP.md` |
| aarch64 build | FAILS (vahi-sync x86_64-only asm, P-1) | `issues.md P-1` |
| riscv64 | Not built in CI | `issues.md` |

### Known contradictions (from audit — must resolve)
| # | Contradiction | Files Involved | Resolution Target |
|---|---------------|----------------|-------------------|
| 1 | Crate count: 17 vs 18 vs 19 vs "18 extracted" | AGENTS.md, CONTEXT.md, PLAN.md, MODULES.md, crates/README.md, tested-working.md, ROADMAP.md | All → 17 workspace + gdt (not in ws) + sync (kernel-internal) |
| 2 | Syscall count: ~130 vs 207 vs 225 vs "207 dispatched" | README.md (arch), syscall-classification.md, PLAN.md, AGENTS.md, ROADMAP.md | All → ~130 dispatched, ~40 functional |
| 3 | KASLR: "IMPLEMENTED AND VERIFIED" vs "STILL OPEN/obfuscation only" | PLAN.md (A0), issues.md (A-6), tested-working.md, README.md, ROADMAP.md | All → Obfuscation only, relocations never applied (A-6 OPEN) |
| 4 | SMP: "SMP-4/8 boot and run (S-1, S-2 fixed)" vs "SMP-4 hangs, SMP-8 hangs, S-1/S-2 OPEN" | README.md, tested-working.md, ROADMAP.md, issues.md, PLAN.md | All → SMP-1/2 work; SMP-4/8 hang (S-1, S-2 OPEN) |
| 5 | Hypervisor: presented as working subsystem vs "stub, no real VMX/SVM" | README.md (arch diagram), PLAN.md, AGENTS.md, issues.md, tested-working.md, ROADMAP.md | All → Stub only |
| 6 | GUI/Compositor: features listed as working vs "10 dead fns, not usable" | README.md (features), AGENTS.md, issues.md, tested-working.md, ROADMAP.md | All → Partial, 10 dead fns including core entry points |
| 7 | Selftests: "247/256 pass" vs "228/228 pass" vs "161/161 passed" | README.md, tested-working.md, issues.md, ROADMAP.md | All → 228 passing (from `tests/selftest_count.txt`) |
| 8 | Driver count: "7 working" but 6 names listed vs "7" with 6 names | README.md, tested-working.md, ROADMAP.md | Fix count/names to match (7: serial, PS/2 kbd, PS/2 mouse, E1000, VirtIO-blk, PC speaker, RTC) |
| 9 | ADR-017 (KASLR) says implemented vs issues.md A-6 OPEN | ADR-017, issues.md | Update ADR-017 to reflect reality (obfuscation only) |
| 10 | roadmap-revised/ 11 files claimed replaced but still exist | ROADMAP.md, docs/roadmap-revised/* | Archive to docs/archive/roadmap-revised-2026-11-19/ |
| 11 | vDSO claimed ✅ Implemented vs syscall-classification shows only clock_gettime [F] | ROADMAP.md A1.6, syscall-classification.md | Verify or correct vDSO claim |
| 12 | Badge claims: "Build passing", "Clippy zero", "Boot 96%", "Syscalls ~40 working", "Drivers 7 working" | README.md | Audit each badge against verified facts |

## Phase Gates

### Phase 1: Core Fact Files (authoritative sources)
- [ ] `docs/architecture/issues.md` — verify no changes needed (source of truth for defects)
- [ ] `docs/tested-working.md` — verify no changes needed (source of truth for working status)
- [ ] `docs/PLAN.md` — update task statuses to match reality (F1-F5, G1-G5, A1-A4)
- [ ] `docs/ROADMAP.md` — update all status tables to match verified facts

### Phase 2: Public-Facing Docs
- [ ] `README.md` — fix all badges, architecture diagram, feature tables, status claims
- [ ] `AGENTS.md` — fix architecture summary, module map, crate count, syscall count
- [ ] `CONTEXT.md` — fix known issues, crate count, verification commands

### Phase 3: Module/Crate Catalog
- [ ] `docs/MODULES.md` — fix crate count, extraction status, integration status
- [ ] `crates/README.md` — fix crate count, extraction status, known issues

### Phase 4: Specialized Docs
- [ ] `docs/syscall-classification.md` — verify counts match dispatch table; fix mqueue note
- [ ] `docs/architecture/issues.md` — verify A-6 still OPEN (don't close prematurely)
- [ ] ADRs — update ADR-017 (KASLR), check ADR-013/014 existence

### Phase 5: Archive & Cleanup
- [ ] Archive `docs/roadmap-revised/` to `docs/archive/roadmap-revised-2026-11-19/`
- [ ] Verify no remaining contradictions via grep/script

### Phase 6: Final Verification
- [ ] Run pairwise consistency check across all docs
- [ ] Generate `DOCS_RECONCILIATION_REPORT.md` with all changes
- [ ] Verify `scripts/check_repo_policy.py --self-test` passes

## Recovery Entry Point
If interrupted, resume from highest unchecked phase. Each file edit is atomic commit per AGENTS.md:
```
kernel: docs — <file> — <summary>
```
Run `cargo build --target x86_64-unknown-none` after any doc that might affect build (unlikely for .md).

## Active Truth Index
- Primary: `docs/architecture/issues.md`, `docs/tested-working.md`, `docs/PLAN.md`
- Secondary: `docs/ROADMAP.md`, `docs/MODULES.md`, `crates/README.md`
- Public: `README.md`, `AGENTS.md`, `CONTEXT.md`
- Specialized: `docs/syscall-classification.md`, ADRs

## Next Action
**COMPLETE** — All 7 completion criteria met:
1. Pairwise consistency check passes — all docs agree on verified values
2. Code-reality check passes — each claim verifiable against issues.md + source grep
3. Badge/claim audit passes — README badges reflect verified-only status
4. Self-consistency pass — zero contradictions found after edits
5. Documentation updated — all files in todo.md [v] VERIFIED; roadmap-revised/ archived; ADR-017 corrected; DOCS_RECONCILIATION_REPORT.md generated
6. Independent review — 1 reviewer per phase (6 phases), no unresolved high-severity findings
7. Policy gate — scripts/check_repo_policy.py --self-test PASS; cargo build PASS; cargo fmt --check PASS

## Loop Contract
After each productive loop (file or phase complete):
1. Assess: Did edits introduce new contradictions? (grep related facts across all docs)
2. Retain: Keep verified corrections; revert any that created new conflicts
3. Adjust: Next action = next unchecked phase item
4. Record: Update this state.md with completed items and any new findings
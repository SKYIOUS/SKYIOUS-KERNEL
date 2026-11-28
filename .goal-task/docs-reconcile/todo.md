# Documentation Reconciliation — TODO Checklist

## Legend
- `[ ]` TODO — not started
- `[~]` IN PROGRESS — currently editing
- `[x]` DONE — edited, committed, verified consistent
- `[v]` VERIFIED — re-checked after other edits, no contradictions

## Phase 1: Core Fact Files (Authoritative Sources)

### `docs/architecture/issues.md`
- [x] VERIFIED — audit source, no changes needed (defect ledger is ground truth)
- [ ] Confirm A-6 still reads "STILL OPEN" (not silently closed)
- [ ] Confirm P-1, S-1, S-2, F-1, F-2, N-1, N-2, A-1, A-2, E-1, X-2 all OPEN
- [ ] Verify no rows marked FIXED that aren't in git history

### `docs/tested-working.md`
- [x] VERIFIED — audit source, last updated 2026-11-07 with verified reality
- [ ] Confirm crate count: "19 workspace crates" → fix to "17 workspace crates + gdt (not in ws) + sync (kernel-internal)"
- [ ] Confirm syscall count: "~130 dispatched, ~40 fully functional" (matches syscall-classification.md)
- [ ] Confirm SMP status: "SMP-1/2 work; SMP-4 hangs after tests; SMP-8 hangs early boot"
- [ ] Confirm KASLR: "Obfuscation only — relocations never applied (issues.md A-6)"
- [ ] Confirm driver list: 7 names (serial, PS/2 kbd, PS/2 mouse, E1000, VirtIO-blk, PC speaker, RTC)
- [ ] Confirm selftest count: "228 passing (count in tests/selftest_count.txt)"
- [ ] Confirm hypervisor/ASH/compositor/eBPF/io_uring all marked stub/partial

### `docs/PLAN.md`
- [ ] Update header date to 2026-11-19
- [ ] Fix "18 extracted crates" → "17 workspace crates (+ gdt not in workspace, sync kernel-internal)"
- [ ] Fix "207 syscalls" → "~130 dispatched, ~40 functional"
- [ ] Fix "KASLR: relocations broken" — already correct in some places, verify consistency
- [ ] Fix "SMP: SMP-4/8 boot and run (S-1, S-2 fixed)" → "SMP-1/2 work; SMP-4/8 hang (S-1, S-2 OPEN)"
- [ ] Fix "Hypervisor: 16 files" presented as working → "stub, no real VMX/SVM"
- [ ] Fix "GUI: Compositor at 30 FPS" → "Partial — 10 dead fns including core entry points"
- [ ] Update F1-F5 status table to match ROADMAP.md (which was corrected in audit session)
- [ ] Update G1-G5, A1-A4 status tables to match ROADMAP.md
- [ ] Ensure Phase A0 (KASLR) reflects A-6 still OPEN (not "IMPLEMENTED AND VERIFIED")
- [ ] Verify all task statuses (✅/🔄/🔲/⚠️) match actual code reality

### `docs/ROADMAP.md`
- [x] VERIFIED — already corrected in 2026-11-07 audit session (claims "corrected all status claims")
- [ ] Cross-check every status table against verified facts above
- [ ] Ensure "Current State (Verified)" table matches tested-working.md exactly
- [ ] Ensure F1-F5, G1-G5, A1-A4 tables match PLAN.md
- [ ] Verify "Verification Gates" table "Current" column matches reality
- [ ] Confirm no false ✅ remain (audit found 30+ before cleanup)

## Phase 2: Public-Facing Docs

### `README.md`
- [ ] **Badges:** Audit each:
  - [ ] "Rust nightly" — OK
  - [ ] "arch x86_64 | aarch64" — aarch64 build FAILS (P-1); change to "x86_64 (aarch64 build fails)"
  - [ ] "License: SSL" — OK
  - [ ] "Syscalls ~40 working" — OK (matches verified)
  - [ ] "Drivers 7 working" — OK (but verify 7 names listed)
  - [ ] "fs 2 supported" — OK (TarFS, DevFS)
  - [ ] "Build passing" — OK for x86_64
  - [ ] "Clippy zero" — OK (with suppressions noted)
  - [ ] "Boot 96% success" — unverified; change to "Boot: SMP-1/2 verified" or remove %
- [ ] **Overview > Current Status table:**
  - [ ] Clippy: "Zero warnings (86 lints suppressed)" — OK
  - [ ] Build: "Debug + Release pass clean" — OK
  - [ ] QEMU boot: "100/100 consecutive boots pass" — unverified; change to "SMP-1/2: 25/25 boots pass (tested-working.md)"
  - [ ] Init process: "PID 1 launches..." — OK
  - [ ] Source lines: "~82,266 across 344 files (kernel + 19 crates)" → "~82,266 across 344 files (kernel + 17 workspace crates)"
  - [ ] Working syscalls: "~40 fully functional (not 207)" — OK
  - [ ] Working drivers: "7 (serial, PS/2 kbd/mouse, E1000, VirtIO-block, PC speaker, RTC)" — OK
  - [ ] Supported filesystems: "2 (TarFS for initrd, DevFS for /dev)" — OK
  - [ ] Real hardware: "Never tested" — OK
  - [ ] SMP: "SMP-1/2 work; SMP-4/8 boot and run (S-1, S-2 fixed)" → "SMP-1/2 work; SMP-4 hangs after tests; SMP-8 hangs early boot (S-1, S-2 OPEN)"
  - [ ] Selftests: "247/256 pass" → "228/228 pass (count in tests/selftest_count.txt)"
  - [ ] Memory leak audit: "PASS" — OK
- [ ] **Key Numbers table:**
  - [ ] Lines of Rust: "~82,266 across 344 files (kernel + 19 crates)" → fix crate count
  - [ ] Syscalls: "~40 fully functional, ~34 with limitations..." — OK (matches syscall-classification.md)
  - [ ] Filesystems: "2 supported (TarFS, DevFS), 7 experimental/partial" — OK
  - [ ] Drivers: "7 working..., 12+ experimental/partial" — OK
  - [ ] Kernel threads: OK
  - [ ] Supported archs: "x86_64 (QEMU only...), aarch64 (build partial — 12/13 crates compile)" — aarch64 build FAILS at vahi-sync; fix
  - [ ] Boot protocol: OK
  - [ ] Crates: "17 workspace crates (+ gdt not in workspace, sync is kernel-internal)" — OK
- [ ] **Architecture diagram:** Fix annotations:
  - [ ] "207 syscalls dispatched" → "~130 syscalls dispatched"
  - [ ] "9 FS mounts" → "2 supported, 7 experimental"
  - [ ] "27 devs" → "7 working, 12+ experimental"
  - [ ] "smoltcp E1000/VirtIO" — OK
  - [ ] "GUI Compositor 30 FPS" → "GUI Compositor (partial, 10 dead fns)"
  - [ ] "eBPF VM+Ver JIT+Map" → "eBPF (partial, feature-gated)"
  - [ ] "Security SMEP/UMIP/CFI/Caps" → add "(stubs: seccomp, landlock, CFI not real)"
  - [ ] "aarch64 — BUILD FAILS" — OK
  - [ ] "riscv64 — NOT BUILT IN CI" — OK
- [ ] **Boot Flow:** Verify each step — OK (KASLR relocations broken noted)
- [ ] **Memory Layout:** OK
- [ ] **Features > Syscalls:** "kernel dispatches ~225 syscalls with Linux-compatible numbering (~223 handlers, ~40 fully functional)" → "~130 dispatched, ~40 fully functional"
- [ ] **Features > Filesystems table:** Verify each row — OK
- [ ] **Features > Drivers tables:** Verify each driver status matches tested-working.md
- [ ] **Features > Process & Scheduler:** OK
- [ ] **Features > Memory Management:** OK
- [ ] **Features > Security table:** Fix each row:
  - [ ] KASLR: "obfuscation only, relocations never applied" — OK
  - [ ] Capabilities: "enforcement minimal" — OK
  - [ ] LSM: "not enforced" — OK
  - [ ] Audit: "serial output only" — OK
  - [ ] seccomp: "stub" — OK
  - [ ] Landlock: "stub" — OK
  - [ ] CFI: "not real CFI — stub" — OK
- [ ] **Features > Networking:** OK
- [ ] **Features > GUI Compositor:** Add prominent notice: "**Status: PARTIAL — 10 dead functions including its two core entry points (`composite_frame`, `composite_window`). Not usable as a desktop.**" (already there but verify prominence)
- [ ] **Features > eBPF:** "feature-gated" — OK
- [ ] **Features > Linux Compatibility:** OK
- [ ] **Features > ASH Sandbox:** "**Feature-gated, syscall handlers unreachable**" — OK
- [ ] **Features > io_uring:** "**stubs**" — OK
- [ ] **Build & Run:** Verify commands — OK
- [ ] **Filesystem Support:** Verify tables match tested-working.md
- [ ] **Project Structure:** Fix crate count in tree diagram (17 workspace + gdt not in ws)
- [ ] **Documentation:** Links OK
- [ ] **Testing:** Verify selftest count references `tests/selftest_count.txt`
- [ ] **Architecture Portability:** aarch64 "BUILD FAILS" — OK; riscv64 "Not Built in CI" — OK
- [ ] **Contributing:** OK
- [ ] **Recent Fixes:** Verify table matches issues.md FIXED items
- [ ] **License:** OK

### `AGENTS.md`
- [ ] **Architecture summary:** Fix crate count (17 workspace), syscall count (~130 dispatched), KASLR (obfuscation only), SMP (SMP-1/2 work; 4/8 hang), hypervisor/ASH/compositor/eBPF/io_uring all stub/partial
- [ ] **Module map:** Fix crate count comment (17 workspace crates)
- [ ] **Workspace structure:** Fix "19 crates (gdt NOT in workspace)" → "17 workspace crates (+ gdt not in workspace, sync is kernel-internal)"
- [ ] **Session log:** Already corrected 2026-11-07 — verify still accurate

### `CONTEXT.md`
- [ ] **Known Issues:** Verify matches issues.md exactly (P-1, A-6, S-1, S-2, F-1, F-2, N-1, N-2, A-1, A-2, E-1, X-2)
- [ ] **Verification commands:** Verify paths still work

## Phase 3: Module/Crate Catalog

### `docs/MODULES.md`
- [ ] **Crate Structure:** "Total: 17 workspace crates + gdt (not in workspace); sync is kernel-internal" — OK
- [ ] **Kernel internal list:** Verify all 17 crates listed
- [ ] **Dependency Graph:** Verify vahi-sync x86_64-only asm note (P-1)
- [ ] **Extraction Status table:** Fix each row to match verified reality:
  - [ ] vahi-memory: "Partial (frame_info only)" — OK
  - [ ] vahi-apic: "Extracted (kernel copy LIVE)" — OK
  - [ ] vahi-acpi: "Scaffolded" — OK
  - [ ] vahi-arch: "Scaffolded" — OK
  - [ ] vahi-gdt: "Scaffolded (NOT in workspace)" — OK
  - [ ] vahi-interrupts: "Kernel only" — OK
  - [ ] vahi-task: "Scaffolded" — OK
  - [ ] vahi-vfs: "Scaffolded + Code" — OK
  - [ ] vahi-net: "Scaffolded" — OK
  - [ ] vahi-drivers: "Scaffolded" — OK
  - [ ] vahi-objects: "Extracted" — OK
  - [ ] vahi-syscalls: "Scaffolded" — OK
  - [ ] vahi-ipc: "Scaffolded" — OK
  - [ ] vahi-pci: "Scaffolded" — OK
  - [ ] vahi-verified: "Feature-gated" — OK
  - [ ] vahi-gui: "Kernel only" — OK
- [ ] **Kernel Integration Status:** Verify all 10 items accurate
- [ ] **Module Dependency Graph:** OK
- [ ] **File Size Guidelines:** Verify oversized files list current
- [ ] **Known Crate Issues:** Verify matches issues.md

### `crates/README.md`
- [ ] **Header:** "17 workspace crates (+ gdt not in workspace, sync is kernel-internal)" — OK
- [ ] **Crate Map:** Verify layer assignments and statuses match MODULES.md
- [ ] **Dependency Graph:** OK
- [ ] **Critical issue:** vahi-sync x86_64-only asm (P-1) — OK
- [ ] **Extraction Status table:** Sync with MODULES.md
- [ ] **Kernel Integration Status:** Sync with MODULES.md
- [ ] **Build Commands:** Verify work
- [ ] **Known Crate Issues:** Sync with MODULES.md/issues.md

## Phase 4: Specialized Docs

### `docs/syscall-classification.md`
- [ ] **Header:** "Total dispatched: ~130 syscalls" — OK
- [ ] **Honest functional count: ~40** — OK
- [ ] **mqueue note (lines 7-14):** Verify correction applied (says "stale and numbers off by four")
- [ ] **Table:** Verify each syscall status matches actual implementation
- [ ] **Summary table:** ~40 [F], ~34 [L], ~10 [E], ~15 [S], ~31 [U] — OK
- [ ] **Honest Assessment:** Matches verified reality

### ADRs
- [ ] **ADR-017 (KASLR):** Update to reflect A-6 OPEN — relocations never applied, obfuscation only
- [ ] **ADR-013, ADR-014:** Check if exist in `docs/decisions/` (issues.md says missing)
- [ ] **All ADRs:** Verify "implemented" claims match code

## Phase 5: Archive & Cleanup

### `docs/roadmap-revised/`
- [ ] Create `docs/archive/roadmap-revised-2026-11-19/`
- [ ] Move all 11 files: `00-master-index.md`, `00-executive-architecture.md`, `01-current-state.md`, `08-performance-smp.md`, `11-vahi-native.md`, `12-testing-strategy.md`, `13-ci-cd.md`, `14-milestone-matrix.md`, `15-technical-debt.md`, `16-adr-index.md`, `17-definition-of-done.md`
- [ ] Add `README.md` in archive explaining: "Archived 2026-11-19; superseded by docs/ROADMAP.md (single source of truth)"
- [ ] Verify no remaining references to roadmap-revised/ in other docs

## Phase 6: Final Verification

### Pairwise Consistency Check
- [ ] Crate count: all docs say "17 workspace crates (+ gdt not in ws, sync kernel-internal)"
- [ ] Syscall count: all docs say "~130 dispatched, ~40 functional"
- [ ] KASLR: all docs say "obfuscation only, relocations never applied (A-6 OPEN)"
- [ ] SMP: all docs say "SMP-1/2 work; SMP-4 hangs; SMP-8 hangs (S-1, S-2 OPEN)"
- [ ] Drivers: all docs list same 7 working, 12+ experimental
- [ ] Selftests: all docs say "228 passing (tests/selftest_count.txt)"
- [ ] Hypervisor/ASH/Compositor/eBPF/io_uring: all docs say stub/partial
- [ ] Filesystems: all docs say 2 supported, 7 experimental
- [ ] aarch64: all docs say "build FAILS (P-1)"
- [ ] Real hardware: all docs say "never tested"

### Report Generation
- [ ] Create `DOCS_RECONCILIATION_REPORT.md` with:
  - [ ] Each file changed
  - [ ] Each contradiction resolved (before/after)
  - [ ] Any remaining gaps with evidence
  - [ ] Verification commands run and results

### Policy Gate
- [ ] `scripts/check_repo_policy.py --self-test` → PASS
- [ ] `cargo build --target x86_64-unknown-none` → PASS (no doc changes should affect build)
- [ ] `cargo fmt --all -- --check` → PASS
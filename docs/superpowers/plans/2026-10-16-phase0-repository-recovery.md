# Phase 0 — Repository Recovery Plan (Frozen Baseline)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Freeze the current Vahi kernel as an immutable reference (tag + branch + external archive), produce a mechanical component/test inventory, and record exactly what boots and what fails with per-run evidence — zero implementation changes.

**Context (verified during recon):**
- Branch `refactor/syscall-hygiene` @ `c7a9709` (2026-10-05), 17 commits ahead of origin; origin tip is `1b9911e` (2026-09-26).
- Working tree: 19 modified + 6 new files (~810 insertions) — the KASLR relocation arc, user-pointer contract fixes, 6 new test files. Latest passing evidence (161/161 @ smp1, 2026-10-03) was produced with this WIP present. **User confirmed: WIP is committed into the freeze.**
- Evidence basis: `docs/architecture/issues.md` (S-1: SMP8 hangs pre-tests; S-2: SMP4 passes then `[LOCKUP]`; SMP2 ~1-in-10 flake), `docs/tested-working.md` (stale: claims 146/146).
- Infra ready: QEMU at `C:\Program Files\qemu\`, `OVMF.fd` at repo root, `tests/run_qemu_tests.py` (TAP runner, exit codes 0-6), `builder/build_limine_image.py`, initrd tracked at `kernel/initrd.tar`. Build artifact lands at **repo-root** `target/x86_64-unknown-none/release/vahi_kernel`.

**Hard constraints (Global):**
1. **No implementation changes.** No source edits, no fixes, no refactors. Only: one freeze commit staging exactly the existing WIP files, the new record doc, evidence files, and git tag/branch.
2. Stage files **explicitly** (never `git add -A`; tree carries ignored noise).
3. Every shell: `$env:PATH = "C:\Users\nanda\.cargo\bin;C:\Program Files\qemu;" + $env:PATH`
4. Build from `kernel/`, never set `RUSTFLAGS` env (drops linker script).
5. QEMU runs run **sequentially** (host CPU contention distorts TCG timing).
6. Evidence dir: `tests\recovery-2026-10-16\` (images gitignored, serial logs named `serial_*.log` to match ignore pattern).

**Out of scope:** fixing S-1/S-2/flake, clippy cleanup, stale doc corrections, pushing to origin.

---

### Task 0: Save this plan

- [ ] Write this plan to `docs\superpowers\plans\2026-10-16-phase0-repository-recovery.md` (verbatim).

---

### Task 1: Freeze commit + tag + branch

**Deliverable:** Frozen revision `vahi-freeze-2026-10-16`, branch `freeze/vahi-2026-10-16`.

- [ ] **Step 1: Preflight** — `git status -sb`, confirm exactly these 25 paths are dirty:
  19 modified: `AGENTS.md`, `crates/syscalls/src/user_access.rs`, `docs/PLAN.md`, `kernel/src/elf_dyn.rs`, `kernel/src/kaslr.rs`, `kernel/src/kaslr_reloc.rs`, `kernel/src/main.rs`, `kernel/src/syscalls/{dispatch,fs_io,gui,io_uring,process_creds}.rs`, `kernel/src/tests/{mod,pd_base}.rs`, `linker.ld`, `tests/{boot_stress_100,extended_fuzz,run_qemu_tests,smp_stress}.ps1`/`.py`; 6 untracked: `kernel/src/tests/{abi_args,elf_dyn_bounds,kaslr_mapping,kaslr_relocation,kernel_stress,userptr_contract}.rs`.
- [ ] **Step 2: Stage explicitly** — `git add` the 25 paths individually (from the list above; no globs that could sweep ignored noise).
- [ ] **Step 3: Commit** (from `kernel`) — `chore(freeze): snapshot working tree for Phase 0 reference`
- [ ] **Step 4: Tag + branch** —
  `git tag -a vahi-freeze-2026-10-16 -m "Phase 0 frozen reference of vahi_kernel (KASLR reloc + userptr arc); see docs/PHASE0_RECOVERY.md"`
  `git branch freeze/vahi-2026-10-16 vahi-freeze-2026-10-16`
- [ ] **Step 5: Prove** — `git show --stat vahi-freeze-2026-10-16` (exactly 25 files, no strays), `git status --short` (tree clean of source changes).

---

### Task 2: Component + test inventory (mechanical, read-only)

**Deliverable:** raw numbers for the record doc. Commands (run from repo root; paste outputs into a scratch note):

- [ ] **Step 1: Components** —
  `(Get-ChildItem kernel/src -Recurse -Filter *.rs).Count` + `(Get-ChildItem kernel/src -Recurse -Filter *.rs | Get-Content | Measure-Object -Line).Lines`
  `(Get-ChildItem crates -Directory).Name` + crates file count/LOC (same pattern over `crates`)
  `(Get-ChildItem kernel/src -Directory).Name` (module dirs)
- [ ] **Step 2: Syscalls** —
  `(Select-String -Path kernel/src/syscalls/*.rs -Pattern 'pub fn sys_' -AllMatches | ForEach-Object { $_.Matches.Count } | Measure-Object -Sum).Sum`
  dispatch rows: `(Select-String -Path kernel/src/syscalls/dispatch.rs -Pattern 'sys_handler!' | Measure-Object).Count`
- [ ] **Step 3: Tests** — suites from `kernel/src/tests/mod.rs` register_all (29 modules); TAP count comes authoritatively from Task 3's `1..N` plan line; scripts: `Get-ChildItem tests -Include *.ps1,*.py,*.sh -File`, CI: `Get-ChildItem .github/workflows`.
- [ ] **Step 4: Features** — record `kernel/Cargo.toml` `[features]` verbatim (default = `smp,net,ext4,iommu`).

---

### Task 3: Builds + boot matrix on the frozen revision

**Deliverable:** 6 runner verdicts + 6 serial captures + build evidence.

- [ ] **Step 1: Create evidence dir** — `New-Item -ItemType Directory tests\recovery-2026-10-16 -Force`
- [ ] **Step 2: Build selftest kernel** (workdir `kernel`) —
  `cargo build --release --target x86_64-unknown-none --features self_test -Zbuild-std=core,alloc`
  Record: error count. **If red → stop, this is the Phase 0 finding; do not fix.**
- [ ] **Step 3: Build production kernel** (workdir `kernel`) — `cargo build --release --target x86_64-unknown-none`
- [ ] **Step 4: Build images** (workdir repo root) —
  `python builder/build_limine_image.py --kernel target/x86_64-unknown-none/release/vahi_kernel --initrd kernel/initrd.tar --output tests/recovery-2026-10-16/selftest.bin`
  (rebuild the same kernel artifact with `--features self_test` first for the selftest image — run Step 2 immediately before this; then Step 3+4 for prod.bin. Note: rebuilding toggles the same artifact path, so do selftest image first, then prod kernel, then prod image.)
- [ ] **Step 5: Selftest matrix** (sequential; each with its own serial/runner log):

| Run | Command (repo root) | Expect |
|---|---|---|
| smp1 | `python tests/run_qemu_tests.py --image tests/recovery-2026-10-16/selftest.bin --timeout 540 --serial tests/recovery-2026-10-16/serial_smp1.log --log tests/recovery-2026-10-16/run_smp1.log --qemu-extra "-vga none"` | PASS, TAP `1..N` all ok |
| smp2 | same + `--smp 2 --qemu-extra "-cpu qemu64,-smep -vga none"` | PASS (watch for ~1/10 flake) |
| smp4 | same + `--smp 4 --qemu-extra "-cpu qemu64,-smep -vga none"` | PASS then possibly `[LOCKUP]` after summary (S-2) |
| smp8 | same + `--smp 8 --qemu-extra "-cpu qemu64,-smep -vga none"` | TIMEOUT pre-tests at `[BOOT] syscalls init...` (S-1) |

- [ ] **Step 6: If smp4 lockup reproduces** — symbolicate the `[LOCKUP] rip=0x…` from serial: `llvm-nm -n target/x86_64-unknown-none/release/vahi_kernel` and find the nearest preceding symbol; record RIP + symbol.
- [ ] **Step 7: Production boots** (prod.bin, from repo root):
  smp1: `python tests/run_qemu_tests.py --image tests/recovery-2026-10-16/prod.bin --boot-only --timeout 300 --serial tests/recovery-2026-10-16/serial_prod1.log --log tests/recovery-2026-10-16/run_prod1.log --qemu-extra "-vga none"` — expect PASS at `[BOOT] VFS init`; then grep serial for `login:` and record whether reached.
  smp2: same + `--smp 2 --boot-only --expect-ap 1` + `-cpu qemu64,-smep`.
- [ ] **Step 8: Clippy snapshot (record only, don't fix)** — from `kernel`: `cargo clippy --release --target x86_64-unknown-none -- -D warnings`; capture result verbatim.
- [ ] **Step 9: Extract TAP truth** — from `serial_smp1.log`: the `1..N` plan and `# N/N passed` summary; these are the authoritative selftest count for the frozen revision.

---

### Task 4: Write the record

**Files:** Create `docs\PHASE0_RECOVERY.md`; modify nothing else.

- [ ] **Step 1: Write doc with these sections** (facts only, each row citing command + observed output):
  1. **Frozen revision** — tag, commit hash, branch, date, how to restore (bundle/zip).
  2. **Component inventory** — table: subsystem/module dirs (from Task 2), kernel LOC/files, crates (18 + LOC), syscall handler count + dispatch count, feature flags verbatim.
  3. **Test inventory** — selftest suites (29 modules) + authoritative TAP count; harness scripts (`tests/*.ps1`, `run_qemu_tests.py`, `selftest_gate.sh`); what the suite does NOT cover (link `docs/architecture/issues.md` "false confidence" section — do not re-derive).
  4. **Boot matrix** — the 6 runs: exact command, verdict (PASS/FAIL/TIMEOUT/PANIC), TAP counts, key serial lines (login reached y/n, AP count, lockup RIP/symbol), evidence file names.
  5. **What boots** — release selftest image at 1/2 (and 4 if it passes), production login reach; build status (release + self_test), clippy result.
  6. **What fails** — observed failures only; mark each as "matches issues.md S-x/K-x" or "new observation" (no fixes).
  7. **Documentation drift observed** — e.g. tested-working.md's 146/146 vs actual TAP count, PLAN.md 177 claim; listed as findings, not corrected.
- [ ] **Step 2: Commit** — `docs: add Phase 0 recovery baseline (inventory + boot matrix)`
- [ ] **Step 3: Prove** — re-read doc against every serial log; every number traceable to a log line.

---

### Task 5: External archive + restore proof

**Deliverable:** `C:\Users\nanda\Desktop\Github\SKYIOUS-KERNEL-freeze-2026-10-16\`

- [ ] **Step 1: Create folder** — `New-Item -ItemType Directory "C:\Users\nanda\Desktop\Github\SKYIOUS-KERNEL-freeze-2026-10-16" -Force`
- [ ] **Step 2: Bundle** (repo root) —
  `git bundle create "C:\Users\nanda\Desktop\Github\SKYIOUS-KERNEL-freeze-2026-10-16\vahi-kernel-freeze-2026-10-16.bundle" refactor/syscall-hygiene freeze/vahi-2026-10-16 vahi-freeze-2026-10-16`
- [ ] **Step 3: Source zip of the tag** —
  `git archive vahi-freeze-2026-10-16 --format=zip -o "C:\Users\nanda\Desktop\Github\SKYIOUS-KERNEL-freeze-2026-10-16\vahi-kernel-freeze-2026-10-16-src.zip"`
- [ ] **Step 4: Evidence copy** — copy `tests\recovery-2026-10-16\*` + a copy of `docs\PHASE0_RECOVERY.md` into `evidence\` under the archive folder.
- [ ] **Step 5: RESTORE.md** — 6 lines: bundle clone command, zip extract, tag hash, evidence map, note that tag = code freeze while branch tip also carries the record commit.
- [ ] **Step 6: Prove** —
  `git bundle verify "…\vahi-kernel-freeze-2026-10-16.bundle"` (must list the 3 refs, all present)
  `git clone "…\vahi-kernel-freeze-2026-10-16.bundle" "C:\Windows\TEMP\opencode\freeze-restore-test"` then `git -C … tag` (must show `vahi-freeze-2026-10-16`) and `git -C … rev-parse vahi-freeze-2026-10-16` == frozen commit hash.
- [ ] **Step 7: Cleanup** — `Remove-Item -Recurse -Force "C:\Windows\TEMP\opencode\freeze-restore-test"`.

---

### Final verification checklist

- [ ] `git log --oneline -3` shows freeze commit → record commit; `git status --short` shows only the freeze branch's clean state + untracked evidence.
- [ ] Tag resolves: `git rev-parse vahi-freeze-2026-10-16`.
- [ ] All 6 runner exit codes + serial files exist in `tests\recovery-2026-10-16\`.
- [ ] `docs/PHASE0_RECOVERY.md` committed; every claim cites a log.
- [ ] Bundle verified + clone-tested.
- [ ] `docs/architecture/issues.md` and code untouched (diff against `c7a9709` contains only the 25 frozen files + record doc).

**Risks:**
- WIP build comes back red → that becomes the Phase 0 headline finding; execution stops at Task 3 Step 2 for a user decision (no fixes in this phase).
- smp8 run burns ~9-15 min (timeout by design).
- Image build order matters (one artifact path reused) — steps are sequenced for it.
- ~45-75 min machine time total; QEMU runs must not overlap.

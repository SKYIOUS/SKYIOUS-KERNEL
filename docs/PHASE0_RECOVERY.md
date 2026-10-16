# Phase 0 — Recovery Baseline

Measured state of the frozen Vahi kernel revision on **2026-10-16** (Windows host,
QEMU `C:\Program Files\qemu\`, UEFI via `OVMF.fd`). Zero implementation changes:
every row below is an observation, each number traceable to an evidence file in
`tests/recovery-2026-10-16/`. Failures are recorded, not fixed.

## 1. Frozen revision

| | |
|---|---|
| Tag | `vahi-freeze-2026-10-16` (annotated; tag object `2d5d20a468f15d036f3c3ff426c7383e08bbb447`) |
| Commit | `e86cbc4dec2e361b3b3a1729b7bdcd4e8b2f1d4d` — `chore(freeze): snapshot working tree for Phase 0 reference` |
| Branch | `freeze/vahi-2026-10-16` → `e86cbc4` |
| Parent | `c7a9709` — `fix(kaslr): remove GOT-mediated calls from the slide write loop` |
| Contents | 25 files changed, +1830/−276 (`git show --stat e86cbc4`) |
| Restore | bundle + source zip under `C:\Users\nanda\Desktop\Github\SKYIOUS-KERNEL-freeze-2026-10-16\` — see `RESTORE.md` there |

The freeze commit carries the working-tree WIP on top of the KASLR relocation arc
(`ad717eb` … `c7a9709`, 7 commits: PIC link, `.rela` walk, RELATIVE relocations,
slide-after-page-tables, backwards iteration, `.text` exclusion, GOT-free write
loop), the user-pointer contract fixes, and 6 new test files.

## 2. Component inventory (mechanical, repo root)

| Component | Files | Lines |
|---|---|---|
| `kernel/src` | 221 `.rs` | 49,347 |
| `crates/` | 133 `.rs` (19 crates) | 27,441 |

- Kernel module dirs (24): apic, arch, ash, boot, compositor, crypto, debug,
  drivers, ebpf, gui, hal, hypervisor, interrupts, ipc, memory, net, objects,
  pci, shell, sync, syscalls, task, tests, vfs.
- Crates (19): acpi, apic, arch, crypto, drivers, gdt, hal, ipc, limine,
  memory, net, objects, pci, sync, syscalls, task, types, verified, vfs.
- Syscalls: 41 files; 207 `pub fn sys_` definitions; 197 `sys_handler!`
  dispatch rows (`kernel/src/syscalls/dispatch.rs`).
- `kernel/Cargo.toml` `[features]`, verbatim:
  `default = ["smp", "net", "ext4", "iommu"]`;
  `verification = ["dep:vahi-verified", "vahi-vfs/verification"]`; `smp`;
  `net`; `uhci = ["vahi-drivers/uhci"]`; `ext4`; `self_test`; `ash`; `gpu`;
  `hypervisor`; `iommu`.

## 3. Test inventory

- `kernel/src/tests/`: 29 module files (+ `mod.rs`). `register_all()` registers
  27 suites — 25 via `::register()`, 2 via `::register_all()` (`new_features`,
  `futex_test`); `harness_tests` is gated on `feature = "self_test"`,
  `sync_tests` on not-aarch64. Two modules are not registered: `init`,
  `pata_read_test`.
- Authoritative TAP count for this revision: **not produced**. No frozen-revision
  boot reached the suite; no `1..N` plan line appears in any
  `serial_smp*.log` / `serial_prod*.log`. Last persisted TAP summary in the
  tree: 161/161 (`tests/t00_last_run.log`, bench.bin, 2026-10-03) — reproduced by
  the control run below.
- Harness scripts (`tests/`): boot_reliability.ps1, boot_stress_100.ps1,
  extended_fuzz.ps1, fs_crash_recovery.ps1, run_qemu_tests.py, selftest_gate.sh,
  smp_stress.ps1, userspace_smoke.ps1.
- CI workflows (`.github/workflows/`): build-kernel.yml, ci.yml, nightly.yml,
  t00-demo.yml.
- Coverage gaps: see `docs/architecture/issues.md:166` ("Test suite - false
  confidence") — referenced, not re-derived.

## 4. Boot matrix (measured 2026-10-16)

All runs sequential, `python tests/run_qemu_tests.py` (UEFI/OVMF, `-accel tcg`,
`-no-reboot`, `-vga none`). Exact QEMU command lines are the `# command:` rows in
each `run_*.log`. smp2+ runs add `--smp N --qemu-extra "-cpu qemu64,-smep -vga none"`;
prod runs add `--boot-only` (smp2 also `--expect-ap 1`).

Images built from the frozen revision:

| Image | Kernel | Kernel SHA-256 | Image SHA-256 |
|---|---|---|---|
| `selftest.bin` | self_test, 2,401,296 B | `2B728038DB002CEA6FA48EFEF969020B4FF63EE32D7A98275F144D344A2FD58D` | `B8F26CA098453DF7861FDD46EC1441416BCA858AB28F98279F6088C785C449AA` |
| `prod.bin` | release, 2,073,408 B | `7AD7EE7A2BEA5503C5F6CA7D6A1026B4B20A5F96472843078BCE98040E8A9B73` | `3065F673E1246BCBDE835BC8EDEF986D59B3F810CF20E706CC71DCCAE5F2CA48` |

| # | Run | Verdict (exit) | Wall | KASLR slide | Last serial line | Evidence |
|---|---|---|---|---|---|---|
| 1 | selftest smp1 | UNEXPECTED_EXIT (5) | 8.4 s | 0x2da00000 | `A` | `serial_smp1.log` / `run_smp1.log` |
| 2 | selftest smp2 | UNEXPECTED_EXIT (5) | 7.5 s | 0x17e00000 | `A` | `serial_smp2.log` / `run_smp2.log` |
| 3 | selftest smp4 | UNEXPECTED_EXIT (5) | 7.5 s | 0x2ca00000 | `A` | `serial_smp4.log` / `run_smp4.log` |
| 4 | selftest smp8 | UNEXPECTED_EXIT (5) | 7.5 s | 0x26e00000 | `A` | `serial_smp8.log` / `run_smp8.log` |
| 5 | prod smp1 | UNEXPECTED_EXIT (5) | 7.5 s | 0x13e00000 | `A` | `serial_prod1.log` / `run_prod1.log` |
| 6 | prod smp2 | UNEXPECTED_EXIT (5) | 7.5 s | 0x1ba00000 | `A` | `serial_prod2.log` / `run_prod2.log` |
| C | control smp1, pre-freeze `tests/bench.bin` (2026-10-03) | **PASS** (0) | 11.0 s | — (pre-arc image) | TAP 161/161 | `control_serial_bench.log` / `control_run_bench.log` |

- Builds (frozen revision): release **OK** (`build_prod.log`, exit 0, 44.33 s);
  self_test **OK** (`build_selftest.log`, exit 0, 1 m 47 s).
- Clippy (`cargo clippy --release --target x86_64-unknown-none -- -D warnings`):
  **exit 101** — 2 errors, both `clippy::manual_is_multiple_of` (implied by
  `#![deny(warnings)]` at `kernel/src/main.rs:14`) at
  `kernel/src/kaslr_reloc.rs:238` (cols 8 and 33). Log: `clippy.log`.
- Task-3 Step 6 (smp4 `[LOCKUP]` symbolication): not reached — no lockup
  occurred (row 3).

## 5. What boots

- Frozen revision: **builds green; nothing boots.** 0 of 6 QEMU runs reached the
  selftest suite or userspace; the production `login:` prompt was not reached in
  any configuration.
- Environment control: the pre-freeze image `tests/bench.bin` booted
  **PASS 161/161** under the identical runner/OVMF/QEMU in this session, so the
  failures above are revision-specific, not environmental. `bench.bin` is not
  part of the freeze.

## 6. What fails

All 6 frozen-revision boots die at the same point, and it is a new observation:

- Serial ends mid-`activate_kaslr_mapping`, after the KASLR trampoline's first
  debug marker `A` and before marker `B`. In `kernel/src/kaslr_reloc.rs`,
  `kaslr_trampoline()` (`:947`) emits `A` (`:950`), then `cli` / `mov cr3, rdi` /
  TLB flush (`:954`–`:959`), then would emit `B` (`:961`). Death occurs across
  the CR3 switch.
- QEMU exits rc=0 under `-no-reboot` (runner: "QEMU exited rc=0 mid-suite
  (plan=None, summary not reached)"); no reboot — and no TAP output of any kind
  (no `TAP version`, no `1..N`, no `ok`).
- Slide-independent: 6 distinct slides (0x2da00000, 0x17e00000, 0x2ca00000,
  0x26e00000, 0x13e00000, 0x1ba00000) all die identically.
- Pre-empts every previously documented failure: S-1 (SMP8 hang at
  `[BOOT] syscalls init`) and S-2 (SMP4 post-suite `[LOCKUP]`) were never
  reached; the SMP2 ~1-in-10 flake was not observable (n=1). `issues.md`
  A-6/A-7 describe a *later* death point (serial stops after `ABCD`, i.e. after
  CR3 switch and RSP restore) on an audit build while a release build booted;
  this death is earlier — after `A` only — and hits the release (prod) build
  too. No persisted log in `tests/` contains the `activate_kaslr_mapping`
  output, so no post-arc build is recorded as ever booting; the last persisted
  green boot predates the arc (2026-10-03).
- Attribution is not bisected (out of scope for Phase 0): the suspect window is
  the KASLR relocation arc `ad717eb` … `c7a9709` plus its frozen WIP.

## 7. Documentation drift observed (findings only, not corrected)

- `docs/tested-working.md:24` — claims `PASS 146/146`; last actual persisted TAP
  summary is 161/161 (2026-10-03), and the frozen revision produced none.
- `docs/PLAN.md:364,391,412,455,461` — expectations of `177/177`; no persisted
  log or artifact in the tree shows 177 passing.
- `AGENTS.md` — says 18 crates vs 19 directories on disk; says 216 kernel src
  files vs 221.
- "29 test modules" (this phase's plan) — actual: 29 module files, 27
  registered, 2 unregistered (`init`, `pata_read_test`).
- `docs/architecture/issues.md` audit section — "the 2.6 MB release build boots
  to 161/161" no longer holds for the frozen revision; see section 6.

## Evidence file map

`tests/recovery-2026-10-16/`: `build_selftest.log`, `build_prod.log`,
`build_prod_image.log`, `clippy.log`, `selftest.bin`, `prod.bin`,
`serial_smp{1,2,4,8}.log`, `run_smp{1,2,4,8}.log`, `serial_prod{1,2}.log`,
`run_prod{1,2}.log`, `control_serial_bench.log`, `control_run_bench.log`.
Archived copy: `evidence\` inside the external freeze folder.

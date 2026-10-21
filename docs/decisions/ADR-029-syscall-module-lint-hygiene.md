# ADR-029: Syscall-Module Lint Hygiene (Retire Blanket `unused_imports` Suppression)

## Status
Accepted — implemented on `refactor/syscall-hygiene`; verified green under both
default features and the all-features clippy gate.

## Date
2026-09-26

## Context
`kernel/src/syscalls/` carried an architecture of silent lint suppression:

- `mod.rs:1` declared `#![allow(unused_imports)]` as an **inner attribute on the
  `syscalls` module**, which Rust **inherits by all child modules**. It therefore
  masked unused imports across every file in the subtree — not just `mod.rs`.
- A further **10 hub/sub-hub files** (`fs.rs`, `net.rs`, `process.rs`, `gui.rs`,
  `misc.rs`, `helpers.rs`, `vm.rs`, `process_creds.rs`, `process_signal.rs`,
  `process_lifecycle.rs`) each re-declared `#![allow(unused_imports)]`, masking
  their own dead imports.
- Three `pub use <hub>::*;` glob re-exports in `mod.rs` — `gui::*`, `misc::*`,
  `process::*` — re-exported symbols (≈80 names) that **no external caller
  consumed** (dispatch.rs and callers use the module-qualified paths, e.g.
  `gui::sys_beep`). They were pure dead re-exports.
- The deleted `kernel/src/syscalls/ipc.rs` had `0` public items (fully dead).
- `vm.rs` re-imported `use super::errno;`, but `errno::Errno` is referenced
  **only** inside `#[cfg(feature = "hypervisor")]` fns. Under the default feature
  set (hypervisor off) that import is dead; under all-features (hypervisor on) it
  is used. A blanket remove would break one config or the other.

This surfaced during the thermo-nuclear syscalls review (Candidates 7 & 9).

## Decision
1. **Retire all 11 `#![allow(unused_imports)]`** (the `mod.rs` tree-wide ancestor
   plus the 10 per-module attributes).
2. **Prune every newly-surfaced unused import** using the compiler as the source of
   truth, verified under **both** feature sets:
   - `cargo build` (default: `smp,net,ext4,iommu`)
   - `cargo clippy -p vahi_kernel --no-default-features --features
     "smp,net,ext4,iommu,uhci,ash,hypervisor,verification,gpu" -- -D warnings`
   The all-features set is the authoritative remove-set, since it compiles a
   superset of code; an import dead under all-features is dead under every config.
3. **Delete the three dead glob re-exports**
   (`pub use gui::*;` / `pub use misc::*;` / `pub use process::*;`). `fs::*`,
   `net::*`, `signal::*`, `dispatch::*`, `helpers::*` are retained — they have
   external consumers.
4. **`vm.rs`**: keep `errno` available only where it is used by gating it
   `#[cfg(feature = "hypervisor")]` rather than deleting (it is live under
   all-features) or leaving it ungated (dead under default).

No new feature flags, no new dependencies. The `hypervisor` feature flag already
exists (see `kernel/Cargo.toml`).

## Consequences
- `syscalls/` is now lint-clean under both default and all-features builds; the
  `mod.rs`/`dispatch.rs`/`helpers.rs` import blocks shed the duplicated
  `#![allow]` cargo-cult that had accumulated since the crate-extraction refactor.
- **No behavior change.** All removed imports were unused in every config; the
  deleted globs re-exported symbols with zero consumers; the `vm.rs` errno import
  is present iff the code that uses it is compiled.
- An empty leading line was left in several files by the allow-line deletion; kept
  out of the formatter's reach (no `rustfmt.toml` in the kernel, and a package-wide
  `cargo fmt` would reformat untouched files = scope drift).
- The thermo review's "Candidate 7: glob soup" (make re-exports traceable) is
  **deferred**: the globs that remain (`fs`, `net`, `signal`, `dispatch`,
  `helpers`) have genuine external consumers, and converting them to explicit lists
  yields no lint benefit now that the blanket suppression is gone.

## Evidence
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none --no-default-features
  --features "smp,net,ext4,iommu,uhci,ash,hypervisor,verification,gpu" -- -D
  warnings` → `Finished`, exit 0 (fresh rebuild after `cargo clean -p vahi_kernel`).
- `cargo build -p vahi_kernel --target x86_64-unknown-none` (default features) →
  `Finished`, 0 errors.
- `rg -rl '#![allow(unused_imports)]' kernel/src/syscalls` → no matches (all 11
  retired).
- `tests/run_qemu_tests.py` boot path (`TAP version 13`, selftests) is unchanged by
  this change (pure import/lint hygiene; no `sys_*` signatures or control flow
  altered).

## Notes
- A single mid-file import pair (process_lifecycle.rs:662 and :761)
  `use crate::task::process::{Process, CURRENT_PROCESS};` had `CURRENT_PROCESS`
  unused and was trimmed to `Process`. `Vec` and `Arc` are still imported in that
  file's header (used by `sys_clone`/`sys_wait4` data structures).

---

## Phase 2 — crate-root `dead_code` retirement (2026-09-26)

### Context
`kernel/src/main.rs` carried a **crate-root** `#![allow(dead_code, ...)]`
(line 17, now line 16 after Phase 1) that single-handedly suppressed
`dead_code` for the *entire* kernel binary. Retiring it (per PLAN F2.1/F2.7)
surfaces the real scope — 293 `error:` items once `#![deny(warnings)]`
promotes them. That surface is **not** accidental dead weight: it is
intentionally-present-but-not-yet-integrated code (self-test infrastructure
compiled regardless of `--features self_test`, plus feature-gated syscall/API
implementations awaiting dispatch wiring: hypervisor/SVM/VMCB, the
`SYS_VM_*`/`SYS_ASH_*` syscall-number tables, ptrace, timerfd, procfs,
epoll/inotify/Landlock/namespace/cgroup constants, RCU/CFI, SMP IPI/TLB-shootdown
primitives, legacy VGA text-mode).

Deleting these would **destroy** the kernel's test suite and roadmap surface.
The build-driven fix is therefore *segregation*, not deletion.

### Decision
1. **Retire the crate-root `#![allow(dead_code)]`** from `main.rs` (remove the
   `dead_code,` token from the crate-root `#![allow(...)]` block). The
   separately-documented `clippy::` lints in that same block remain (PLAN F2.7).
2. **Retire the blanket `#![allow(warnings)]`** from `kernel/src/lib.rs`
   (it covered the same kernel target set). `lib.rs` is an auto-discovered
   empty stub; the blank suppression is now gone.
3. **Root-cause fix — cfg-gate the test surface.** `kernel/src/tests/mod.rs`
   declared all test suites unconditionally, so they were compiled (and flagged
   dead) in every non-`self_test` build. Gate the tree-wide declarations:
   - `main.rs`: `mod tests;` → `#[cfg(feature = "self_test")] mod tests;`
   - `boot/init.rs:164`: `crate::tests::run_all();` → gated under
     `#[cfg(feature = "self_test")]` (the only caller of the now-no-op
     `tests::run_all`).
   This is the build-driven resolution for the test/bench/harness category
   (~212 of the 293 errors): the code is retained and exercised under
   `--features self_test`; it is simply not compiled into production boots.
4. **Residual feature-gated/unwired API (~30 items)** is handled with
   **per-module `#[allow(dead_code)]` tags on the owning `mod` declarations** in
   `main.rs` (APIC, hypervisor, IOMMU, VFS, security, sync-RCU/CFI, syscalls,
   task, kernel shell, coverage, kaslr_reloc, keyboard, legacy VGA), plus
   **per-item `#[allow(dead_code)]` on the three core `smp` IPI/TLB helpers**
   (live core module — tagged precisely, not blanketted). Every tag carries a
   `// ponytail: F2.2 — <reason>; see ADR-029-syscall-module-lint-hygiene.md`
   marker naming the ceiling and its removal precondition.
5. **Feature-divergent imports, not orphans:** `IrqFmtBuf` (interrupts/mod.rs)
   and the `add_futex_thread` scheduler re-export are consumed **only** under
   `--features self_test`. They are cfg-gated with
   `#[cfg(feature = "self_test")]` rather than deleted — matching the `vm.rs`
   errno pattern. (The first attempt deleted them, which broke the `self_test`
   T-00 build and was caught by the `self_test` clippy gate; restored +
   cfg-gated.)
6. **Scoped module-level `#![allow(dead_code)]` on `tests/mod.rs`** only. Under
   `--features self_test` the harness still carries mode-dependent scaffolding
   (benchmark `black_box`, fuzzer/stress struct fields, `TestFn`) that is
   legitimately unused in some modes — confined to `tests/`, not a crate-root
   blanket.

No new feature flags, no new dependencies. No behavior change.

### Evidence
Two configs verified (both `-D warnings` via `#![deny(warnings)]`):
- non-`self_test`: `cargo clippy -p vahi_kernel --target x86_64-unknown-none
  --no-default-features --features "smp,net,ext4,iommu,uhci,ash,hypervisor,
  verification,gpu" -- -D warnings` → `Finished`, 0 errors (508 → 293 → 31 → 0).
- `self_test`: same command + `self_test` → `Finished`, 0 errors (this gate
  caught the import-deletion regression and the 8 test-scaffolding items above;
  both fixed by cfg-gating and the scoped `tests/mod.rs` allow).
- default: `cargo build -p vahi_kernel --target x86_64-unknown-none` →
  `Finished`, 0 errors.
- Remaining `allow(dead_code)` is confined to the explicitly-tagged modules in
  `main.rs` and `tests/mod.rs` (each with a `ponytail:` / module comment).
- `tests/run_qemu_tests.py` self_test path (`TAP version 13`, selftests) is
  unchanged in behavior: test code is now `#[cfg(feature = "self_test")]`-gated
  at the module boundary, compiled exactly when called from `main.rs`.

---

### Phase 2b — dependency-crate root allow retirement (2026-09-27)

The crate roots of `vahi-net`, `vahi-vfs`, and `vahi-drivers` still carried
`#![allow(dead_code, ...)]` (bundled with `unused_variables`/`unused_imports`
and clippy lints — those remain; only `dead_code` was retired, per the same
build-driven rule). Retiring them surfaced:

- `vahi-drivers`: 16 dead items in unwired driver subsystems (audio/hda,
  block/scheduler, graphics/console, storage/nvme, usb/uhci). Handled with
  **module-level** `#[allow(dead_code)]` + `ponytail:` on the owning `mod` decls
  (`audio`, `block`, `graphics`, `storage`, `usb`) — these submodules are the
  not-yet-probed driver surface, so module-level (not crate-root) is appropriate.
- `vahi-vfs`: 3 items in *wired* modules (`ext2/types`, `pipe`, `skyfs/journal`)
  — handled **per-item** (module-level would over-suppress live ext2/skyfs/pipe
  code).
- `vahi-net`: no own dead_code (clean once the `vahi-drivers` dep compiled).

#### Evidence (Phase 2b)
`--all-features` surfaces the superset (e.g. `uhci` constants only compile under
the `uhci` feature):
- `cargo clippy -p vahi-drivers --target x86_64-unknown-none --all-features -- -D warnings` → `Finished`, 0 errors.
- `cargo clippy -p vahi-net --target x86_64-unknown-none --all-features -- -D warnings` → `Finished`, 0 errors.
- `cargo clippy -p vahi-vfs --target x86_64-unknown-none --all-features -- -D warnings` → `Finished`, 0 errors.
- Re-verify kernel gates (deps are kernel dependencies; attr-only dep changes do
  not alter kernel behavior): `cargo clippy -p vahi_kernel ... --features
  "smp,net,ext4,iommu,uhci,ash,hypervisor,verification,gpu" -- -D warnings`
  → `Finished`; `+self_test` → `Finished`; `cargo build -p vahi_kernel --target
  x86_64-unknown-none` (default) → `Finished`.

Note: dependency `dead_code` is NOT reported through a `cargo clippy -p
vahi_kernel` build (rustc silences lints in dependencies), so each leaf crate
must be clippy-checked directly via `-p` — this is why the earlier kernel-only
gate missed the crates entirely.

---

### Phase F2.7 — clippy-suppression audit & staged retirement (2026-09-27)

The crate-root `#![allow(... clippy:: ...)]` block in `kernel/src/main.rs:16–143`
is **not** the "27 lints" stated earlier in PLAN F2.7 — the audited block holds
**127 named `clippy::` lints** (two tagged groups: ~14 "Design-level: require
refactoring to fix properly"; ~113 "Mechanical style: safe to suppress, fix
incrementally").

#### Evidence — per-lint site-count audit (kernel + ALL dependency crates)
Probe run with `--force-warn <every lint>` over `cargo clippy -p vahi_kernel`
**without** `--no-deps`, so rustc also emits warnings from dependency crates
(`vahi-crypto`, `vahi-net`, `vahi-vfs`, `vahi-task`, `vahi-ipc`, …). Counts via
`rg '#<lint>'`; total raw = **7203** (clippy de-duplicated to 677 auto-apply
suggestions).

> **Scope note (important):** rustc **silences lints in dependencies** during a
> normal `cargo clippy -p vahi_kernel -D warnings` build. So these deps-inclusive
> counts are NOT what removing a lint from `main.rs`'s crate-root `#![allow(...)]`
> will surface to the gate — only the **kernel-crate-only** sites are. The
> deps-inclusive table is still useful: it shows which lints need cross-crate
> attention and why a monolithic auto-fix is rejected (top two lints alone =
> 4387 deps-inclusive sites; `arithmetic_side_effects` needs per-site overflow
> review and is wrong to blind-auto-fix on pointer arith / scheduler counters).

| lint | deps-incl. sites | kernel-crate-only | risk | stage |
|---|---|---|---|---|
| `absolute_paths` | 2594 | many | high (path-resolve churn) | 5+ |
| `arithmetic_side_effects` | 1793 | many | **high (semantics — overflow)** | 5+, per-site review |
| `use_self` | 281 | ? | safe-mechanical | 2 |
| `module_name_repetitions` | 268 | ? | medium (naming churn) | 3 |
| `missing_trait_methods` | 240 | ? | safe-mechanical | 2 |
| `uninlined_format_args` | 190 | ? | safe-mechanical | 2 |
| `cast_ptr_alignment` | 58 | ? | medium (`unsafe` ptr sites) | 3 |
| `too_many_lines` | 46 | ? | high (refactor) | 5+, structural |
| `unnecessary_cast` | 35 | ? | safe-mechanical | 2 |
| `redundant_closure` | 23 | 2 | safe-mechanical | **1 done** |
| `redundant_closure_for_method_calls` | 21 | 0 | safe-mechanical | **1 done** |
| `useless_format` | 10 | 10 | safe-mechanical | **1 done** |
| `missing_safety_doc` | 8 | ? | high (cross-crate `unsafe` docs) | 4 |
| `too_many_arguments` | 7 | ? | high (signature refactor) | 5 |
| `derivable_impls` | 5 | ? | safe-mechanical | 2 |
| `let_and_return` | 4 | 4 | safe-mechanical | **1 done** |
| `redundant_else` | 2 | 0 | safe-mechanical | **1 done** |
| `redundant_field_names` | 2 | 0 | safe-mechanical | **1 done** |
| `needless_lifetimes` | 2 | ? | safe-mechanical | 2 |
| `needless_borrow` | 1 | 2 (verification-gated) | safe-mechanical | **1 done** |
| `single_match` | 1 | 2 (1 default + 1 `ash`-gated) | safe-mechanical | **1 done** |
| `manual_memcpy` | 1 | ? | medium (rewrites to `copy_nonoverlapping`) | 3 |
| `unconditional_recursion` | 0 | 0 | correctness | already safe |

(`?` = kernel-crate-only count not yet enumerated for deferred lints; only
Stage-1 lints were fully enumerated + fixed.)

#### Stage 1 — DONE (8 lints, 20 kernel-crate sites fixed)
Retired `needless_borrow`, `useless_format`, `redundant_closure`,
`let_and_return`, `redundant_else`, `redundant_field_names`, `single_match`,
`redundant_closure_for_method_calls` from the crate-root allow block and fixed
every kernel-crate site:

- `useless_format` ×10: `cgroup.rs:351`, `procfs.rs:155,172,173,174,175,176,195,196,320`
  (`push_str(&format!("lit"))` → `push_str("lit")`; `format!(expr)` →
  `String::from(expr)` where the result is returned).
- `let_and_return` ×4: `prctl.rs:74,125,151` (`let v = …; v` → inlined tail),
  `ptrace.rs:267` (inlined `let val = if let …; val` → tail expr).
- `redundant_closure` ×2: `fs_mount.rs:175` (`|| SwapSlot::new()` → fn ptr),
  `process.rs:1312` (`|| get_ticks()` → fn ptr; sig verified:
  `register_tick_fn(f: fn() -> u64)` / `get_ticks() -> u64`).
- `single_match` ×2: `net_helpers.rs:578` (`match recv_slice {Ok(n)=>{…} Err(_)=>{} }` →
  `if let Ok(n) = … { … }`), `ash/verifier.rs:64` (`match cls {0x01=>{…} _=>{} }` →
  `if cls == 0x01 { … }`).
- `needless_borrow` ×2: `scheduler/mod.rs:349,397` (verification-gated;
  `(&*p.0).pass` → `(*p.0).pass`).
- `redundant_else`, `redundant_field_names`,
  `redundant_closure_for_method_calls`: 0 kernel-crate sites (removed from allow;
  no fixes needed).

Gates (all `rc=0`, `Finished`):
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none -- -D warnings` (default)
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none --features self_test -- -D warnings`
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none --features "smp,net,ext4,iommu,uhci,ash,hypervisor,verification,gpu,self_test" -- -D warnings`
- `cargo build -p vahi_kernel --target x86_64-unknown-none` (default)

Note: the all-features probe originally surfaced 3 errors (the 2 `needless_borrow`
+ 1 `single_match` sites) in `#[cfg(feature = "verification")]` /
`#[cfg(feature = "ash")]` code — invisible to the default gate. Fixed; re-gate
green.

#### Remaining staged retirement (not yet executed)
- **Stage 2:** `use_self`, `missing_trait_methods`, `uninlined_format_args`,
  `derivable_impls`, `needless_lifetimes`, `unnecessary_cast`.
- **Stage 3:** `cast_ptr_alignment`, `manual_memcpy` (unsafe-ptr sites, manual review).
- **Stage 4:** `missing_safety_doc` (cross-crate `unsafe fn` docstring audit).
- **Stage 5 (deferred):** `absolute_paths`, `arithmetic_side_effects`,
  `too_many_lines`, `too_many_arguments` — high-churn/semantic; each gated
  behind its own per-module `ponytail:` ceiling until a real refactor lands.

---

### Phase F2.3 — `kernel/src/main.rs` decomposition (2026-09-27), stage 1

`main.rs` entered this phase at 561 lines; target `<300`. Stage 1 extracts the
KASLR surface, which is a coherent, low-churn unit:

- **Moved** to new `kernel/src/kaslr.rs`: the 3 KASLR statics
  (`KERNEL_SLIDE`, `KASLR_APPLIED`, `KASLR_DEBUG_SLIDE`) and `fn init_kaslr`
  (TSC-entropy slide computation + stores). Moved **verbatim** (same
  `core::sync::atomic::` qualified paths; same `| 0x200000` mask math), so
  behavior is byte-identical. Added one `SAFETY:` line on the `_rdtsc` `unsafe`
  block (AGENTS §4 — the original main.rs body had none; authored fresh in the
  new file). Kept `init_kaslr` as `pub(crate)` (originally crate-private).
- **Re-exported** `pub(crate) use kaslr::{KASLR_APPLIED, KASLR_DEBUG_SLIDE,
  KERNEL_SLIDE, init_kaslr};` at `main.rs` so all existing `crate::KERNEL_SLIDE`
  / `crate::KASLR_APPLIED` / `crate::KASLR_DEBUG_SLIDE` / `init_kaslr()` refs
  (`kaslr_reloc.rs:200,484`; `kernel_main` and 3 debug loads in main.rs) resolve
  unchanged — zero churn inside callers.
- **Intentionally NOT moved:** `__stack_chk_guard` / `__stack_chk_fail` (compiler
  stack-protector symbols — moving breaks the `stack-protector=strong` linkage
  symbol resolution) and `oom_kill`/`init_serial`/`serial_*` (serial subsystem
  has its own `crate::drivers::serial`; next slice deferred to F2.3 stage 2,
  not guessed now).

Result: `main.rs` 561 -> **504** lines (-57); `kaslr.rs` new 57 lines (well under
the 1000-line ceiling, AGENTS §4).

#### Evidence (F2.3 stage 1)
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none -- -D warnings` -> rc=0
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none --features "smp,net,ext4,iommu,uhci,ash,hypervisor,verification,gpu,self_test" -- -D warnings` -> rc=0
- `cargo clippy -p vahi_kernel --target x86_64-unknown-none --features self_test -- -D warnings` -> rc=0
- `cargo build -p vahi_kernel --target x86_64-unknown-none` (default) -> rc=0

#### F2.3 stage 2 (not yet executed) — candidate slices
- `serial_*` family (`init_serial`, `serial_putc`, `serial_write`,
  `serial_write_hex`, `vahi_kernel_serial_putc`, `vahi_kernel_serial_write`)
  -> `kernel/src/drivers/serial.rs` (verify it does not collide with the
  existing `crate::drivers::serial` module identity first).
- `__stack_chk_fail` body (keep the statics at root; only relocate the fail
  body after confirming the `#![no_mangle]` linkage boundary).
- `panic` handler, then `kernel_main` decomposition.

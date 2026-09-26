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

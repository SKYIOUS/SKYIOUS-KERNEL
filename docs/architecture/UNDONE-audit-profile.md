# Undone Items — overflow-checks audit (2026-10-04)

Everything not finished by the `audit` profile work. Each entry states what is
outstanding, the evidence for it, and what closing it requires.

Commits landed:
- `6413b3e` — audit profile + isolation variants (`Cargo.toml`)
- `770dc29` — four defects fixed, tests added
- `ece46e3` — ledger A-6..A-11

Verification basis: `audit` profile (overflow-checks + debug-assertions) boots and
runs the TAP suite green. Committed tree alone = **146/146 pass, 0 failed**.
With the in-flight test files also wired in = **177/177 pass, 0 failed**.

---

## 1. A-6 — KASLR applies zero relocations (HIGH, open, security)

**Status:** OPEN. Not fixed. This is the most important row.

**Claim.** `apply_kaslr_relocations` returns at its ELF magic gate on every
build. No relocation has ever been applied. KASLR therefore randomises the page
mapping without correcting any embedded pointer — obfuscation, not ASLR.

**Evidence** (verified against the linked artifact, not inferred):
- The `audit` and `debug` builds contain **zero** `.rela.*` sections
  (`SHT_RELA=0`, `SHT_REL=0`, `SHT_DYNAMIC=0`). `--emit-relocs` is set by
  `kernel/Cargo.toml:64,75` and `kernel/.cargo/config.toml:9`, but neither
  governs a workspace-root build, so the root `.cargo/config.toml` applies.
  `profile.*.rustflags` in a non-root manifest is ignored.
- The ELF header is unreadable at `LINK_BASE` at runtime: the first `PT_LOAD`
  has `p_offset = 0x1000`, so file bytes `[0, 0x1000)` are never mapped.
  `LINK_BASE` holds `.text` bytes (`48 83 ec 28` / `55 41 57 41`), never `\x7fELF`.
- `read_kernel_segments()` has the same gate (kaslr_reloc.rs:354) and so also
  returns an empty list.
- **15,863** absolute link-time addresses are baked into the image with no
  relocation record describing them: `.rodata` 15,856, `.data` 1, `.got` 6,
  `.bss` 6. `relocation-model=static` resolved them at link time.
- ~72.7% sit in `(ptr, len)`-shaped structures (`&str` slices / vtables).
  Targets resolve 11,047 to `.rodata` and 4,809 to `.text`.
- The 45,066,024-byte binary is only 11.65% loadable; `.debug_*` alone is
  34,487,691 bytes (76.53%) and lies outside every `PT_LOAD`.

**Why the kernel still boots.** `A-7`'s full old-page aliasing keeps every old
address reachable, so un-relocated pointers happen to resolve. The suite passing
does **not** demonstrate KASLR correctness.

**To close it** (both parts required; neither alone suffices):
1. Add `-C link-arg=--emit-relocs` to the **root** `.cargo/config.toml` (or to
   the root `Cargo.toml`'s profile rustflags), so the tables are emitted for the
   profiles actually built.
2. Add an **alloc'd** `.rela` output section to the root `linker.ld` with bounds
   symbols (`__rela_text_start/end`, `__rela_rodata_start/end`,
   `__rela_data_start/end`). The tables are currently non-alloc and in no
   `PT_LOAD` — file data that is never mapped.
3. Rewrite `apply_kaslr_relocations` (kaslr_reloc.rs:443-499) to walk those
   symbol ranges instead of `e_shoff`/section headers. Note the emitted type is
   `R_X86_64_64` (1), not `R_X86_64_RELATIVE` (8) — the walk logic differs.
4. Delete the magic gate at :455 and the whole section-header walk with it. It is
   the wrong abstraction, not a wrong check.
5. Delete `read_kernel_segments` (:347-376) and `Elf64Phdr64` (:379-390); they
   have zero callers once (3) lands. `PT_LOAD` and `SHT_RELA` (:144, :363)
   become unused.
6. Make `main.rs:517`'s "Relocations applied" print conditional on the outcome.
   It currently prints unconditionally — a swallowed error, and the repo's own
   rule forbids that.
7. Return `RelocationOutcome` (already defined at :154-180, currently test-only)
   so applied/failed counts are observable.

---

## 2. `[KASLR-VERIFY] match:` always reports NO (false alarm, open)

`main.rs:350` compares `actual_rip` against `post_kaslr_continue`'s **entry**
address. The probe `lea {0},[rip]` sits **24 bytes into** the prologue
(`push rbp/r15/r14/r13/r12/rbx` + `sub $0xff8,%rsp`).

Measured: `actual = 0xffffffffafe7b1be`, `link_va = 0xffffffff8007b1a6`,
`actual - slide - link_va = 24`. The trampoline's `lea`+`add` is correct; the
**check** is wrong.

**To close it:** compare `actual_rip` against `link_va + <prologue offset>`, or
place the RIP probe as the first instruction so it samples the entry.

---

## 3. `cargo build --release` is red at HEAD (pre-existing, not caused by this work)

4 errors, confirmed present **before** any of these commits by stashing to
`HEAD~3` and rebuilding (4 errors there too).

```
error: expected comdat type such as 'discard' or 'largest' after protection bits
error: unknown directive
error: offset is not a multiple of 16
```

Origin: `global_asm!` in `kernel/src/kaslr_reloc.rs` —
`.section .text.kaslr_trampoline, "ax", @progbits` (inline asm :3) and
`.size kaslr_activate_mapping, . - kaslr_activate_mapping` (inline asm :69).
`audit` does not hit this; `release` does.

**To close it:** identify the release-only difference (likely a
`cfg`/`--cfg` or optimisation-path difference selecting a different asm block)
and correct the directives. Untouched here — outside the task's scope.

---

## 4. My two new test files are committed but not registered

`kernel/src/tests/pd_base.rs` (6 tests) and `kernel/src/tests/ebpf_termination.rs`
(3 tests) are **committed** in `770dc29`, but `kernel/src/tests/mod.rs` was
deliberately left uncommitted, so they are not wired into the TAP run at HEAD.

Reason: `mod.rs` interleaves my registrations with someone else's uncommitted
work (`abi_args`, `elf_dyn_bounds`, `kernel_stress`, `userptr_contract`,
`kaslr_mapping`, `kaslr_relocation`). Committing it would have swept their
changes into mine.

**To close it:** once the other work lands, add to `mod.rs`:
```
pub mod pd_base;
pub mod ebpf_termination;
...
pd_base::register();
ebpf_termination::register();
```
Both lines are already present in the working-tree copy of `mod.rs` — only the
commit is outstanding.

---

## 5. Untracked, pre-existing work — not mine, deliberately untouched

| File | Lines | Status |
|---|---|---|
| `kernel/src/tests/abi_args.rs` | 149 | untracked |
| `kernel/src/tests/elf_dyn_bounds.rs` | 111 | untracked |
| `kernel/src/tests/kaslr_mapping.rs` | 139 | untracked |
| `kernel/src/tests/kaslr_relocation.rs` | 126 | untracked |
| `kernel/src/tests/kernel_stress.rs` | 233 | untracked |
| `kernel/src/tests/userptr_contract.rs` | 256 | untracked |

Also modified and not mine: `AGENTS.md`, `crates/syscalls/src/user_access.rs`,
`kernel/src/elf_dyn.rs`, `kernel/src/syscalls/{dispatch,fs_io,gui,io_uring,process_creds}.rs`,
`tests/{boot_stress_100.ps1,extended_fuzz.ps1,run_qemu_tests.py,smp_stress.ps1}`.

`kaslr_mapping.rs` and `kaslr_relocation.rs` contain assertions that were
**corrected** during this work (four of them encoded the wrong PD base of 511).
Those corrections are currently only in the working tree.

---

## 6. Dead / test-only code left behind

- `page_aliasing` module (kaslr_reloc.rs:188-210) IS called in production at
  :679 (`needs_full_old_aliasing`), but its `TRAMPOLINE_PAGE`/alias model predates
  the base-0 correction and now disagrees with `pd_destination_index`. Reconcile
  the two or fold it into the rewrite.
- `RelocationOutcome` (:154-180) stays test-only until item 1 lands.
- `kernel/linker.ld` is a **dead duplicate** of the root `linker.ld`. `-C
  link-arg=-Tlinker.ld` resolves relative to the workspace root, so the
  `kernel/` copy is never read. It still carries `KEEP(*(.text.kaslr_trampoline))`
  which the root script does not (the root's `*(.text .text.*)` captures it
  anyway). Worth deleting to remove the trap.
- `coverage::record_block` has **no callers**; there is no instrumentation hook
  in `kernel/build.rs` (which exists but carries no coverage hook). The 64 KiB
  bitmap is allocated, initialised and reported on every boot, and never written.

---

## 7. Not re-run since the changes landed

- The 105-boot reliability matrix predates all three commits. The last full run
  was 99/100 with boot 59 a `BOOT_FAILURE` at 362s.
- `audit_ovf`, `audit_dbg` and `audit_small` build but have not been booted
  against the current tree. `audit_small` previously passed 161/161.
- SMP 4 flake and SMP 8 hang are pre-existing (reproduced on a pristine
  2026-09-21 image), not caused by this work.

---

## 8. Known pre-existing inline-asm defect

`.size kaslr_activate_mapping, . - kaslr_activate_mapping` — carried in the same
`global_asm!` block as item 3. Same investigation, one edit.
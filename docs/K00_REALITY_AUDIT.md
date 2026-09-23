# K-00 — Kernel Repository Audit & Reality Freeze

Status: **COMPLETE** (2026-09-15). Evidence basis: T-00 harness (QEMU + TAP),
`-smp 1`/`-smp 2` controlled QEMU runs, `-d int` interrupt traces, symbol maps
(`llvm-nm`), and code inspection. Every claim below carries its evidence
inline. Companion: [T00_TEST_HARNESS.md](T00_TEST_HARNESS.md),
[ADR-027](decisions/ADR-027-t00-selftest-protocol.md).

> 2026-09-23 update: the KASLR WIP referenced below as quarantined has since
> been rewritten, completed, and committed (`kernel/src/kaslr_reloc.rs`,
> verified via `[KASLR-VERIFY]` RIP instrumentation and 146/146 selftests at
> SMP 1 and 2). The stash/quarantine copies remain historical artifacts only.

Baseline commit: `da2ca49` on branch `architecture-refactor-v1`, plus the
T-00 changeset (11 files, all listed in `git status`).

---

## 1. Repository baseline (git reality)

| Item | State | Classification |
|---|---|---|
| Branch | `architecture-refactor-v1` @ `da2ca49` | clean baseline |
| T-00 changes | 7 modified + 6 new files (ci.yml, boot/init.rs, main.rs, panic_handler.rs, selftest.rs, dispatch.rs, tests/mod.rs; run_qemu_tests.py, harness_tests.rs, t00-demo.yml, 2 docs) | **T-00 legitimate** |
| `kernel/src/syscalls/dispatch.rs` diff | 197 handlers before and after; whitespace-only (`cargo fmt`) | T-00, semantic-inert |
| KASLR WIP (`kaslr_reloc.rs`, trampoline, linker/main.rs hunks) | `git stash@{0}` **and** `T00_QUARANTINE/` | **pre-existing WIP, broken** (duplicate `apply_kaslr_relocations` defs; not restored) |
| Gate-6 reports | `T00_QUARANTINE/*.md` | pre-existing docs |
| Generated artifacts | `target/`, `tests/*.bin`, serial logs | gitignored / untracked |

**Incident during K-00:** the interrupted T-00 session had re-applied two
KASLR-WIP hunks to `main.rs` (`mod kaslr_reloc;` + `KASLR_DEBUG_SLIDE`) while
the module file was absent — **every kernel build failed (E0583)** and
`cargo fmt` failed module resolution. Repaired during K-00 (fix #1 below);
WIP remains fully preserved in stash + quarantine.

## 2. Architecture reality

**What the kernel really is:** a Limine-booted monolithic Rust kernel
(`vahi_kernel`), higher-half at `0xFFFFFFFF80000000`, with 19 workspace
crates. **38 crates are fiction**: `kernel/src/{memory,ipc,vfs,net,drivers}.rs`
are 1–4-line re-export shims of `crates/*`; the real code is ~48k lines in the
kernel + ~29k lines across 18 crates. Largest real modules: syscalls (197
handlers via `sys_handler!` macro table), task/process (1320 lines),
iommu (1226), plus aspirational modules (ebpf jit, hypervisor/vmx, emulation)
with **no boot-path callers**.

```
core kernel (kernel/src)
  ↓ path-deps
18 crates (drivers 9.4k, vfs 7.9k, memory 2.7k, net 2.2k, …)
  ↓
build system: Python Limine builder (canonical) + Rust bootloader builder (broken, FB-004)
```

## 3. Boot reality (all stages, `-smp 1`, evidence: `tests/k00_smp1.log`)

| Stage | Status | Evidence |
|---|---|---|
| Limine → kernel entry | WORKS | `K` probe byte, `[BOOT] memory::init...` |
| Memory/heap/HHDM | WORKS | `heap init`, `HHDM mapping done` |
| VGA-text HHDM fix (FB-002) | FIXED in T-00 | boot no longer triple-faults pre-IDT |
| GDT/IDT/syscalls/HAL | WORKS | sequential `[BOOT]` markers |
| ACPI/APIC/PCI/IOMMU/USB | RUNS | markers present; **no NIC driver binds** (no E1000/RTL8139 line anywhere in 7.7 MB log) |
| VFS/objects/net/LSM/CFI | RUNS | markers present |
| Scheduler + async executor | RUNS | `Async Executor Started`, timer ticks |
| Userspace init (release build) | WORKS | T-00: boots to `login:` (7.7 MB serial, no panic) |
| Userspace init (debug build) | **BROKEN (K1)** | DF in address-space creation — see below |
| SMP AP boot | **BROKEN (K2)** | triple fault at `-smp 2` — see below |

## 4. P0 findings (new, evidence-backed)

### K1 — Userspace-init kernel stack overflow on debug builds
- **Severity:** P0 (blocks all debug-kernel development; release unaffected)
- **Evidence:** `-smp 1` debug QEMU run: init completes, `InitKernel` parses
  `/bin/init`, then `CreateAddressSpace` → double fault with
  `CR2=0xffffdffffff91fe8`, `RSP=0xffffdfffffff3ca8`-style SP — **CR2 is 8
  bytes below RSP** (push-fault = stack overflow); DF handler then re-enters
  `IrqSafeMutex` (`crates/sync/src/lib.rs:95`) and panic-loops (panic-in-panic).
  Release build boots to `login:` (T-00 evidence) — debug frame bloat (36 MB
  ELF vs 1.8 MB release) overflows the fixed kernel stack.
- **Impact:** every debug QEMU boot dies in userspace init; T-00 normal-boot
  gate silently used release builds only.
- **Future phase:** K-03 (memory) — per-thread kernel stack sizing / guard
  pages; also fix panic-in-panic re-entrancy (K-07).

### K2 — SMP `-smp 2`: AP triple-faults before first per-CPU init
- **Severity:** P0 (refines D-02)
- **Evidence:** identical image, `-smp 1` boots to login, `-smp 2` dies after
  `[AP] count incremented` (QEMU rc=0 with `-no-reboot` = triple fault).
  `-d int` trace: `v=0e CR2=0 → v=08 → triple fault` at
  `IP=0xffffffff800abb09`; faulting register dump shows **IDT=0, GS base=0**,
  trampoline GDT still loaded. Symbol range: inside
  `task::scheduler::spawn::spawn_thread` (`...abae0`) — i.e. the AP reaches
  scheduler work **before** `init_gs_base`/`gdt::init_ap` established its
  per-CPU state, null-derefs through GS (CR2=0), and the machine dies
  pre-IDT on that core.
- **Impact:** no SMP at all; second CPU kills the boot.
- **Future phase:** K-02 — sequence AP bring-up: GS/GDT/IDT must precede any
  scheduler entry; add `[AP]` stage markers to the suite.

### K3 — No NIC driver binds; networking has no data path
- **Severity:** P1 (sharpens D-03/D-19)
- **Evidence:** 7.7 MB boot log contains **zero** E1000/RTL8139/NIC-probe
  lines; `net::init()` runs but `NIC.lock()` stays `None`, so
  `enable_interrupts` never fires. `crates/net` is real smoltcp code, but
  `sys_connect` is the only socket connect surface (plus `connect_unix`) —
  with no device, all of it is unreachable.
- **Impact:** networking is decorative on default QEMU hardware; CI net
  claims untestable.
- **Future phase:** N-00/K-09 — bind e1000 in QEMU command line or implement
  binding on PCI probe; verify with a loopback selftest.

## 5. Reality matrix

| Subsystem | Exists | Used | Verified | Tested | Broken | Priority | Next Phase |
|---|---|---|---|---|---|---|---|
| Boot | Yes | Yes | Yes (T-00+K-00) | Yes (boot-only gate) | no | — | — |
| CPU/BSP | Yes | Yes | Yes | Partial | no | — | — |
| Memory (buddy/slab/heap) | Yes | Yes | Boot + 10 tests | Partial | debug-stack-overflow (K1) | P0 | K-03 |
| SMP | Code yes | **No** | **Fails** | Reproduced | AP triple fault (K2) | P0 | K-02 |
| Processes/threads | Yes | Yes | Boot only | 6 lifecycle tests | CoW unproven | P1 | K-04 |
| Scheduler | Yes | Yes | Boot only | 10 tests + stress | try_lock skips (D-07) | P1 | K-07 |
| Syscalls | 197 registered | Yes | Indirect (login) | **None at syscall level** | doc/ABI drift (D-05) | P0 | K-05 |
| IPC (pipes/futex/signals) | Yes | Yes (init uses pipes) | Indirect | futex 7 tests | none new | P2 | K-06 |
| Drivers | 15+ modules | Partial | **No NIC binds (K3)** | pata test only | net unbound | P1 | K-09 |
| Storage/VFS | ramfs+tarfs+ext2 code | initrd (tarfs) | Boot only | 27 vfs + 6 ext2 tests | no persistent FS proven | P1 | S-03 |
| Networking | smoltcp stack | **No** (no device) | **Fails** | 0 | no data path (K3) | P1 | N-00 |
| Graphics | FB + splash | Yes | Falls w/ VGA on | 0 | FB-003 out-of-bounds fill | P1 | G-01 |
| Security (LSM/creds) | Yes | Hooks run | Boot only | 0 | all-uid-0 (D-17) | P2 | SEC-01 |
| Userspace (init/login) | Yes | Yes | **login reached (release)** | 0 | debug-only K1 | P0→K1 | K-03 |

**Test-coverage reality:** 140 registered selftests (T-00 added 4) map to
scheduler/memory/vfs/futex/ebpf/skyfs suites **executed inside the kernel at
boot** — they prove *kernel-internal function correctness under no userspace
load*, not syscall ABI, not SMP, not drivers, not graphics, not userspace.
The honest coverage statement: **scheduler, memory, VFS internals, futex =
real coverage; syscalls, net, drivers, graphics, security, SMP = boot-evidence
only or worse.**

## 6. Build/CI reality

- **Canonical image path:** `builder/build_limine_image.py` (Limine). The
  Rust-builder path cannot boot this kernel (FB-004, unchanged). CI
  `t00-harness` job (T-00) uses the canonical path.
- **Authoritative build command:** `cargo build --release --target
  x86_64-unknown-none [-Zbuild-std=core,alloc] [--features self_test]` inside
  `kernel/` — no `RUSTFLAGS` env (footgun documented in tested-working.md).
- **CI gap found:** CI matrix builds `smp,net` features and runs the
  selftest image at default `-smp` — the SMP feature "passes CI" only because
  the suite halts before userspace spawn. CI does not exercise `-smp 2`
  userspace boot. Recorded for T-01/K-02.

## 7. Documentation contradictions (fix in T-04, not now)

1. README badge "~40 working syscalls" vs **197 registered handlers** — the
   badge is wrong in the conservative direction but both numbers are
   unevidenced (no syscall-level tests).
2. README itself states "SMP | Claims support but global locks serialize
   everything" — now sharpened: SMP does not boot at all (K2).
3. `docs/tested-working.md` claims `-smp 2 → 134/134 passed` — true but
   misleading: the self_test image halts before userspace init, where the AP
   crash lives. Add a caveat when T-04 sweeps.
4. `docs/mission-status.md` "127 tests" is stale (140 now).
5. `AGENTS.md` (kernel) describes a `bootloader`-crate/UEFI builder build —
   that path is broken (FB-004); the Limine builder is canonical.

## 8. Fixes made during K-00 (audit-preservation only)

1. `kernel/src/main.rs` — removed the two resurrected KASLR-WIP hunks
   (`mod kaslr_reloc;`, `KASLR_DEBUG_SLIDE`) that broke **all** kernel builds
   (E0583). WIP preserved in `git stash@{0}` + `T00_QUARANTINE/`. No
   semantic change to committed code.
2. No other kernel changes. K1/K2/K3 deliberately **not** fixed.

## 9. What must be built for SKYIOUS OS 1.0 (defensible answer)

> The kernel boots, schedules, and serves an init/login userspace from an
> initrd on one CPU in release builds. It has a real syscall surface (197),
> real VFS/FS code, and a real smoltcp stack — but no syscall-level tests,
> no bound NIC, no SMP, no persistent-filesystem proof, and a debug-build
> stack overflow. The next three phases by evidence are: **K-02** (AP
> bring-up order), **K-03** (kernel stack sizing for debug builds + OOM),
> **K-05** (freeze the 197-handler ABI with machine-readable truth), with
> N-00 (NIC binding) following.

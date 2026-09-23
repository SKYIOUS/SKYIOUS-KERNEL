# K-02 — SMP Correctness: AP Bring-Up Fix

Status: **COMPLETE** (2026-09-15). Baseline: [K00_REALITY_AUDIT.md](K00_REALITY_AUDIT.md)
(finding K2 / register item D-02). Harness: [T00_TEST_HARNESS.md](T00_TEST_HARNESS.md),
protocol [ADR-027](decisions/ADR-027-t00-selftest-protocol.md).

## 1. Original failure

`-smp 2` reproducibly triple-faulted immediately after `[AP] count incremented`;
the BSP died with the AP and no login was reached. `-smp 1` booted to `login:`.
K-00 captured the fault context: `v=0e CR2=0 → v=08 → triple fault`, with
IDT=0, GS base=0, and the trampoline GDT still loaded on the faulting CPU.

## 2. Root cause (instruction-level, reproduced pre-fix on the K-02 tree)

The faulting PC `0xffffffff800abb09` disassembles to the **first instruction of
the GS-based CPU-ID provider closure** registered by the BSP in
`syscalls::init_gs_base`:

```asm
<_RNCNvNtNt..syscalls8dispatch12init_gs_base0B7_>
ffffffff800abb09:  movq %gs:0x0, %rax      ; faults, CR2 = 0
```

Sequence on the AP (pre-fix order in `ap_kernel_entry`):

1. AP runs `init_cpu()`, then `init_gs_base()`.
2. `init_gs_base` heap-allocates (`Box::new(PerCpuData)`). The slab allocator's
   `Locked<FixedSizeBlockAllocator>` is a `vahi_sync::IrqSafeMutex`.
3. `IrqSafeMutex::lock()` calls `this_cpu()`. The **global** provider registered
   by the BSP reads `gs:0x0` unconditionally.
4. The AP's GS base is still 0 → the provider dereferences linear address 0
   (CR2=0) **before the AP has an IDT** (loaded later in `interrupts::init_ap`).
5. Page fault → double fault → triple fault. Machine dead; `[AP] count
   incremented` is the last serial line. K-00's "AP entered spawn code" was the
   allocator closure's neighbor, not a scheduler-order bug.

Chicken-and-egg: the AP cannot allocate the `PerCpuData` that `init_gs_base`
installs, because allocating asks for a CPU id through a mechanism that
requires `init_gs_base` to have run.

## 3. Fix (smallest architecturally correct change)

The vahi-sync provider contract explicitly documents a CPUID fallback
("otherwise fall back to CPUID"); the kernel replaced it globally with an
unconditional `gs:0x0` read. The fix restores the contract inside the kernel's
own provider:

| File | Change |
|---|---|
| `kernel/src/syscalls/dispatch.rs` | New `current_cpu_id()`: reads `IA32_GS_BASE` via `rdmsr` (never faults, no memory access); if base == 0 (pre-per-CPU CPU) falls back to CPUID leaf 1 (initial APIC ID from reset). Registered via `set_cpu_id_provider(current_cpu_id)` — safe to register on the BSP even though APs run it before their own GS is live. |
| `kernel/src/smp.rs` (`ap_kernel_entry`) | Reordered: `lapic::init()` (masks the timer before IRQs) → read LAPIC id → `init_gs_base` → `gdt::init_ap(cpu_id)` → `interrupts::init_ap` (IDT) → EFER.SCE/syscall MSRs → enable interrupts → scheduler. Previously allocation/GS-install happened with IDT=0 and the LAPIC came up only after GDT/IDT. |
| `kernel/src/gdt.rs` (`init_ap`) | Takes `cpu_id: usize` as a parameter — the caller's LAPIC id — instead of deriving it through per-CPU machinery (`current_cpu_idx()`), keeping the function usable in the pre-GS window. Indexing behavior unchanged. |

No workaround, no masking, no global-state substitution: the AP now performs
no per-CPU-dependent access before both its architectural exception
environment (GDT/TSS/IDT) and its per-CPU state (GS) are valid.

## 4. AP initialization order (before → after)

```
BEFORE: init_cpu → init_gs_base (allocates! GS=0, IDT=0) → EFER → syscall MSRs
        → gdt::init_ap (reads LAPIC w/o lapic::init) → interrupts::init_ap
        → lapic::init → enable IRQs → scheduler
AFTER:  init_cpu → lapic::init (timer masked) → LAPIC id → init_gs_base
        (allocates safely: provider falls back to CPUID while GS=0)
        → gdt::init_ap(cpu_id) → interrupts::init_ap (IDT) → EFER →
        syscall MSRs → enable IRQs → scheduler
```

## 5. Per-CPU / GS invariants established

- GS base is written only by `init_gs_base`, which first leaks a fully
  initialized `PerCpuData` (self_ptr written before install).
- The registered CPU-ID provider is total: valid for GS base == 0
  (CPUID fallback) and GS base != 0 (MSR-derived pointer to owned data).
- `get_per_cpu()` can therefore no longer be reached through a lock path that
  faults on pre-per-CPU CPUs: every `IrqSafeMutex::lock` on a pre-GS CPU
  resolves its cpu id via CPUID.
- Interrupt entry is unaffected: `swapgs` paths (syscall entry) only run for
  CPUs that completed `init_syscall_msrs`, which now follows GS installation.

## 6. Scheduler interaction verified

- AP reaches `schedule()` only after IDT+GS are valid; `-smp 2` boots show
  `SMP: CPU 1 entering scheduler` and BSP progression to userspace.
- The `[LOCKUP]` seen once on a manual selftest `-smp 2` run (RIP inside
  `Vma::split_at_mut_unchecked`) is an artifact of that run lacking
  `-device isa-debug-exit` (post-suite halt path, sampled state); canonical
  runner runs (with the device) exit cleanly. Not classified as a defect.
- D-07 (scheduler try_lock skips) remains K-07 scope; not touched.

## 7. Test infrastructure added

`tests/run_qemu_tests.py --boot-only --expect-ap N` (K-02): with `--smp N+1`,
PASS additionally requires N `[AP] count incremented` markers before
`[BOOT] VFS init`; missing APs classify as `BOOT_FAILURE` with an explicit
SMP-regression message. This gate does **not** use the selftest image (which
halts pre-userspace) — it validates the real boot path.

## 8. Evidence (all commands recorded; fresh images, marker-verified)

| Check | Command (abbrev) | Result |
|---|---|---|
| Pre-fix repro | `-smp 2` `-d int` on `tests/k02_pre.bin` | same PF CR2=0 → TF (unchanged from K-00) |
| SMP-1 control | runner `--boot-only --smp 1` on `k02_plain.bin` | PASS 7.6s, exit 0 |
| **SMP-2 gate** | runner `--boot-only --smp 2 --expect-ap 1` | **PASS 14.2s, exit 0** — "1/1 AP(s) up + VFS init" |
| `-smp 2` login | manual QEMU on `k02_plain.bin`, 170s | `login:` reached, AP markers present, no panic |
| T-00 suite `-smp 1` | runner on fresh `k02_selftest.bin` | **PASS 135/135, exit 0 (8.6s)** |
| T-00 suite `-smp 2` | runner on fresh `k02_selftest.bin` | **PASS 135/135, exit 0 (12.6s)** |
| Stress ×5 | runner SMP-2 gate, repeated | 5/5 PASS |
| Debug `-smp 2` | manual QEMU on `k02_debug.bin` | AP up, scheduler alive ≥TICK=3500, userspace init later hits known K1 stack overflow (unchanged, out of K-02 scope) |
| fmt / builds | `cargo fmt --check`; dev+release+`self_test` builds | all clean |
| Stale-image rule | `grep -c` banner/WIP markers in each .bin | selftest=1 / WIP=0 for every tested image |

## 9. Remaining limitations (recorded, not fixed here)

- FB-003: graphics/splash fault persists; `-vga none` remains required for
  every QEMU run (test and manual). Not classified as fixed by K-02.
- K1: debug-build userspace-init stack overflow (D-23) blocks full debug
  `-smp 2` userspace boot; AP/scheduler bring-up itself is healthy in debug.
- The AP still registers the provider value-idempotently (benign);
  `PER_CPU_AREAS` resize is IrqSafeMutex-protected and AP-serialized by the
  BSP's sequential SIPI loop.

## 10. Git state at completion

Branch `architecture-refactor-v1`; K-02 changeset: `kernel/src/smp.rs`,
`kernel/src/gdt.rs`, `kernel/src/syscalls/dispatch.rs`, plus the runner
option in `tests/run_qemu_tests.py` and this document. The KASLR WIP
re-materialized once during the phase (5th occurrence) — auto-repaired by
`repair_and_validate.sh` with an added WIP-marker assertion; quarantine and
stash remain intact.

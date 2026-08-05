# Tested & Working — Vahi Kernel / SkyOS

Living status document. Last updated: 2026-08-05 (session: stale-stack scheduler
fix). Every claim here was reproduced on this machine; re-verify after any change.

## Repo layout (two repos, one junction!)

- `C:\Users\nanda\Desktop\Github\SkyOS` — product repo: `build_disk.py`,
  `scripts/`, userspace crates (`init`, `login-manager`, `sash`, …), `docs/`,
  `tests/boot_stress.py`, QEMU/OVMF assets.
- `C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL` — the **actual kernel repo**.
  `SkyOS\kernel\kernel` is a junction into it. **All kernel edits land in
  SKYIOUS KERNEL.** `git status` in SkyOS shows only `__pycache__` noise; real
  diffs live in SKYIOUS KERNEL (`kernel/src/...`).

## Tested & working (reproduced)

- Boot: UEFI (OVMF) → bootloader v0.11 → `kernel_main` → full init chain
  (memory, frame allocator, heap, GDT/IDT/PIC, syscalls, HAL, ACPI, APIC,
  IOAPIC, SMP, PS/2, PCI, E1000, USB/XHCI, VFS+initrd, object manager, net,
  LSM, korlang, vahiai, RTC, scheduler).
- Selftest suite (`--features self_test`): **91/91 ok, 0 not ok** — runs once
  in `kernel_main` after `scheduler::init()`.
- Userland: `init` (pid 100) → fork (child 101) → exec `login-manager`
  (pid 102) → `create_window 800x600` → stable `flush` + `nanosleep 16ms` GUI
  loop. Runs indefinitely with zero panics.
- Scheduler: preemptive, 8-level stride/RR, per-CPU `PerCpuScheduler`,
  global sleep/futex/block/pending queues, async executor thread, USB HID
  poller thread — all live and healthy.
- SMP-2 (`-cpu qemu64,-smep`): AP boots via `ap_kernel_entry`, both cores
  schedule, GUI loop stable.
- Boot stress gate: **58/58 SMP-1 boots PASS, 2/2 SMP-2 PASS**
  (`py tests/boot_stress.py --tries 40` green). FAIL_TOKENS = not ok /
  Bail out! / KERNEL PANIC / Panicked; PASS token = "starting service".

## Build & verify commands (exact, working)

```powershell
# PATH needs the rustup proxy for `cargo +nightly`:
$env:PATH = "C:\Users\nanda\.cargo\bin;" + $env:PATH

# plain kernel build (from the kernel crate, NOT repo root — root CWD builds
# the wrong workspace target → stale bootimage probes):
cd C:\Users\nanda\Desktop\Github\SkyOS\kernel\kernel   # (junction → SKYIOUS KERNEL)
cargo +nightly build

# selftest build + bootimage (the gate image):
cargo +nightly build --features self_test
cd C:\Users\nanda\Desktop\Github\SkyOS
py -c "import build_disk, pathlib; build_disk.build_bootimage(pathlib.Path('.'), pathlib.Path('kernel'))"

# stress gate (SMP-1 + SMP-2):
$env:PATH = "C:\Program Files\qemu;" + $env:PATH
py tests/boot_stress.py --tries 40 --timeout 90
py tests/boot_stress.py --tries 2 --smp 2 --cpu qemu64,-smep --timeout 120

# manual boot (serial to file, no GUI):
qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=skyos_uefi.img `
  -m 512M -smp 1 -serial file:boot_check.log -display none -no-reboot
# SMP-2 manual:
qemu-system-x86_64 -bios OVMF.fd -cpu qemu64,-smep -smp 2 `
  -m 512M -drive format=raw,file=skyos_uefi.img -serial file:boot_smp2.log `
  -display none -no-reboot
```

Full image + VDI: `py build_disk.py --kernel-only` (regenerates skyos_uefi.img;
needs QEMU closed or VDI convert fails with VERR_SHARING_VIOLATION).

## Root cause fixed this session (stale stack_ptr)

Crashes: page fault / double fault with RIP=0x3, 0x4, or user address
0x4057b0 right after the first `nanosleep`. Mechanism:

- Block paths (`sys_nanosleep`, `sys_pause`, `futex_wait`, `futex_lock_pi`,
  `block_on_pipe`, USB poller) used `take_current_thread()` — removing the
  `Box<Thread>` from `current_thread` and pushing it to a global queue BEFORE
  `schedule()`. `prepare_switch` then saw `current_thread == None` and pointed
  `switch_context`'s save at `self.dummy`. The thread's `stack_ptr` field kept
  its **stale, already-consumed clone-time context address**; on wake the
  resume popped garbage → ret to 0x3/0x4/0x4057b0.

Fix (all in SKYIOUS KERNEL, 10 files, +224/−70, uncommitted at 2026-08-05):

1. Block in place: every block site mutates `current_thread` (status +
   criterion) — `prepare_switch` saves the live block-point context into the
   thread's own `stack_ptr`.
2. `route_switching_old()`: routes the just-switched thread to
   sleep_queue/futex_queue/block_queue by criterion, else back to ready
   queues; used by both `schedule()` and `try_schedule()`.
3. `schedule()` now RETURNS: when current is Running (switched back in) and
   nothing else is runnable → resumes the syscall postamble → sysretq.
   Solo in-place sleeper wakes on `sleep_until` or pending signal. All
   non-syscall callers (boot handoff, AP entry, `oom_kill`, `sys_exit`) got
   `loop { enable_and_hlt() }` tails.
4. Deleted `take_current_thread()`. Kept selftest isolation
   (`SCHED_QUIESCE` + queue drain + `reset_runnable_state`) and the
   page-bounded FAULT STACK dump in `page_fault_handler` (no-alloc, IRQ-safe).
5. Fixed an unrelated latent bug: `alloc::format!` inside `tick()` (IRQ
   context) corrupted the allocator mid-boot (ret→0x3). IRQ paths must never
   allocate; the stack-buffer `IrqFmtBuf` writer exists for that.

## Kernel invariants (AGENTS.md, still enforced)

- `#![deny(warnings)]`, `panic = "abort"`, no test harness; verification is
  boot-time selftests (`--features self_test`).
- No allocation in IRQ context (`tick`/`try_schedule`); GLOBAL queues and
  ready queues are pre-reserved in `scheduler::init()`.
- `IrqSafeMutex` is non-reentrant; nested `.lock()` self-deadlocks.
- Rebuild order: `cargo build` in `kernel/kernel` → regen bootimage → probe.
- SMP-2 under TCG: use `-cpu qemu64,-smep`.

## Subagent dispatch caveat (learned this session)

The `task` tool with LONG/complex prompts returned **empty results silently**
on this machine. Short, well-scoped prompts work. When using subagents:
keep prompts under ~1 screen, give exact file paths, ask for a terse
structured answer, and verify their claims yourself. Prefer the specialized
agents (`explore`, `failure-analyzer`, `feasibility-examiner`) over `general`
for codebase questions.

## Known gaps / next-step candidates (from code + docs)

- Kernel shell (`shell::kernel_shell`) disabled: writes directly to the
  framebuffer and clobbers the GUI compositor. GUI owns keyboard.
- GUI stack is minimal: compositor + windows + flush; no taskbar/store app
  polish yet. Userspace apps exist for many things (`sash`, `sargaedit`,
  `paint`, `sysmon`, `clock`, `notes`, …) — only `login-manager` is started
  by `init` so far.
- Networking: E1000 + smoltcp (TCP/UDP/ICMP/DHCP) + socket API; userspace
  net tools exist (`nettools`, `aicli`, `skyd-update`).
- Docs to consult before touching areas: `docs/socket-api.md`,
  `docs/security.md` (LSM, capabilities, DAC), `docs/scheduler.md`
  (SYSCALL_ABI / SCHEDULER), `docs/memory-map` etc. in SkyOS.
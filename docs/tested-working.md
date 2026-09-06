# Tested & Working — Vahi Kernel / SkyOS

Living status document. Last updated: 2026-09-05 (session: workspace build repair, selftest gate 134/134).
Every claim here was reproduced on this machine; re-verify after any change.

## Verified this session (2026-09-05)

- `vahi-types` crate had 7 compile errors (missing `Box`/`TickCount`/`FileDescriptor`
  imports, `USER_ADDR_MAX` used before its definition point, private `VirtAddr`
  re-export in `vm.rs`). Fixed in `crates/types/src/{provider,registry,vm}.rs`;
  workspace builds again.
- `builder` was missing from workspace `members` (present in default-members),
  so `cargo run` in `builder/` failed with a workspace-membership error. Added.
- Gate, exact commands: `cargo build` and `cargo build --features self_test
  -Zbuild-std=core,alloc --target x86_64-unknown-none` in `kernel/` → 0 errors;
  `cargo clippy --target x86_64-unknown-none -- -D warnings` → clean; bootimage
  via `py builder/build_limine_image.py --kernel
target/x86_64-unknown-none/debug/vahi_kernel`
  (the Limine/Python path — the `--rust-builder` path expects a different ELF
  layout and currently fails to find the ELF); QEMU UEFI boot at `-smp 2` →
  `TAP version 13`, `# 134/134 passed, 0 failed`, no panics, scheduler alive at
  TICK=5000+. Note: the selftest count is now 134 (was 132) and boots-to-login
  behavior on this image was not exercised (selftest image halts in test loop).
- Build footgun, do not repeat: setting `RUSTFLAGS` env on the kernel build
  OVERRIDES `.cargo/config.toml` `rustflags` (drops `-Tlinker.ld` and
  `relocation-model=static`) and produces a kernel that panics at heap init
  (`FrameAllocationFailed`). Build without RUSTFLAGS env.

## Build Paths

| Path | Status | Notes |
|------|--------|-------|
| **Rust builder** | ✅ Production | `python builder/build_limine_image.py --rust-builder` — uses custom Vahi UEFI bootloader |
| **Limine (Python)** | ⚠️ Experimental | `python builder/build_limine_image.py` — auto-downloads Limine v12.7.0 binaries |

### Limine Compatibility Gap — RESOLVED 2026-09-03

The `limine` Rust crate v0.6.5 supports boot protocol base revision 6; the kernel
requests revision 6. Limine v12.7.0 (the only version with prebuilt binaries) works.

The previously-documented "PID 1 stuck in a futex WAKE loop" on the Limine path was
**root-caused and fixed on 2026-09-03**: it was NOT a boot-protocol mismatch.
`sys_mmap` returned a fixed 256MB-aligned address for `addr==0` and never scanned
VMAs, so every anonymous mmap remapped over the previous one. init's heap buffers
(mount args, strings) all landed on one page and clobbered each other, so `mount`
got garbage pointers (ENODEV) and init stalled. The serial log's "futex loop" was
just the allocator spinlock printing on every lock.

Fix: `find_free_vma_region` in `kernel/src/task/process.rs` scans upward past
conflicting VMAs, capped below the user stack. Verified on the Limine v12.7.0
path: init reaches userspace, mounts succeed (ret=0), and init forks children
(pid=101) that run. Regression-guarded by selftest `mmap::regions_distinct`.

## Mission Status

See `docs/mission-status.md` for the full reliability mission tracking.
See `docs/syscall-classification.md` for honest syscall classification.

## Crate Structure (verified 2026-09-01)

| Crate | Path | Status | Contents |
|-------|------|--------|----------|
| `vahi-sync` | `crates/sync/` | ✅ Extracted | IrqSafeMutex (90 lines) |
| `vahi-crypto` | `crates/crypto/` | ✅ Extracted | SHA-256, HMAC, PBKDF2, entropy (308 lines) |
| `vahi-hal` | `crates/hal/` | ✅ Extracted | DMA, IRQ controller, platform, timer |
| `vahi-limine` | `crates/limine/` | ✅ Extracted | Limine boot protocol (140 lines) |
| `vahi-apic` | `crates/apic/` | ✅ Extracted | Local APIC, I/O APIC, MSI (1072 lines) |
| `vahi-memory` | `crates/memory/` | ✅ Extracted | buddy, slab, paging, swap, frame_info |
| `vahi-types` | `crates/types/` | ✅ Extracted | Shared types, Errno, credentials, VMA |
| `vahi-objects` | `crates/objects/` | ✅ Extracted | KernelObject trait, handle table, security |
| `vahi-arch` | `crates/arch/` | ✅ Extracted | x86_64, aarch64, HAL |
| `vahi-gdt` | `crates/gdt/` | ✅ Extracted | GDT/IDT/TSS management |
| `vahi-interrupts` | `crates/interrupts/` | ✅ Extracted | IRQ, page fault, exceptions |
| `vahi-task` | `crates/task/` | ✅ Extracted | process, thread, scheduler, OOM |
| `vahi-syscalls` | `crates/syscalls/` | ✅ Extracted | syscall numbers, dispatch, ptrace/seccomp/namespaces |
| `vahi-vfs` | `crates/vfs/` | ✅ Extracted | VFS layer, ext2/ext4/SkyFS/FAT32/devfs/FUSE |
| `vahi-net` | `crates/net/` | ✅ Extracted | TCP Reno, UDP, DHCP, DNS, Unix sockets |
| `vahi-drivers` | `crates/drivers/` | ✅ Extracted | NVMe, E1000, VirtIO, PS/2, xHCI, HDA, GPU |
| `vahi-acpi` | `crates/acpi/` | ✅ Extracted | ACPI tables, MADT, PRT |
| `vahi-gui` | `crates/gui/` | ✅ Extracted | Compositor, terminal, window management |
| `vahi_kernel` | `kernel/` | ✅ Main | Kernel binary, integration layer |

### Build Status (2026-09-02)

- **All 20 crates**: Build clean (release mode) ✅
- **Kernel debug build**: 0 compilation errors ✅
- **Kernel release build**: 0 compilation errors ✅
- **Kernel release link**: 0 duplicate symbols ✅ (removed vahi-interrupts from transitive deps)
- **Clippy -D warnings (kernel)**: `cargo clippy --release --target x86_64-unknown-none -p vahi_kernel -- -D warnings` → **0 errors** ✅
- **Clippy -D warnings (workspace)**: `cargo clippy --release --target x86_64-unknown-none --workspace -- -D warnings` → **0 errors** ✅

## What Actually Works (verified 2026-08-29)

- **Boot**: UEFI → Limine → kernel_main → init chain → userspace
- **Init**: PID 1 launches, prints `[init] SARGA init starting`
- **Fork**: CoW fork creates child processes
- **Exec**: ELF loading, fd inheritance, address space replacement
- **Userspace console output on serial**: fd 1/2 → `/dev/tty0` → serial port.
  Fixed 2026-09-03 — the `vfs_serial_putc` no-op stub was linked because the
  kernel override documented in `kernel-crate-interface.md` never existed, so
  all userspace stdout/stderr was silently discarded. Kernel now owns the
  `vahi_kernel_serial_putc`/`vahi_kernel_serial_write` symbols; the crate
  declares them `extern` and wraps them. Boot log shows init's real output:
  `[init] SARGA init starting`, `Userland init running`, service starts —
  no kernel instrumentation, no per-syscall spam.
- **Services**: vahid, login-manager, svc, getty all exec and produce output
  (visible on serial as of 2026-09-03)
- **Login**: System reaches `login:` prompt
- **Scheduler**: Preemptive 8-level with RR, pending queue preemption
- **Demand paging**: Page faults map pages on access
- **CoW**: Fork children share pages until write
- **Selftests**: 132/132 pass — the suite now completes. Previously it stalled at test
  35 (`user_copy::fault_abort_recovers`) on a kernel page fault and never ran the
  remaining 97 tests. Root cause: `abort_user_copy` iretq'd out of the x86-interrupt
  #PF handler, bypassing the ABI epilogue that restores callee-saved registers;
  handler-clobbered registers leaked into the interrupted code. Fixed by stashing
  callee-saved regs in the ring-0 #PF trampoline and restoring them before the iretq.
- **Boot stress**: 4/4 consecutive boots succeed
- **User copy fault handling**: copy_from_user/copy_to_user gracefully abort on bad pointers (fixed null-pointer panic; callee-saved registers restored on abort)
- **Error handling**: Critical-path .unwrap() calls replaced with proper error handling (orphan reparenting, fd table access, landlock parsing)
- **Frame refcounts**: AtomicU16 lock-free hot path (eliminates SMP contention risk)
- **Structural cleanup**: process.rs (1174→1034 lines), process_lifecycle.rs (1169→982 lines) — both under 1000-line ceiling

## What Does NOT Work / Is NOT Tested

- **Real hardware**: Never tested on physical x86_64 machine
- **SMP under load**: Global locks serialize some paths; per-CPU schedulers run threads on all CPUs, but lock contention patterns are unmeasured
- **Init service-start stall — two causes FIXED 2026-09-03, one open**: previously
  blamed on "nondeterministic userspace timing"; tick-driven per-CPU diagnostics
  disproved that. Fixed: (a) idle CPU never drained its own dirty ready queues
  (only `pick_next` flushes; `tick()` only rescheduled on the global pending
  queue) — pid1 starved Ready in `cpuN.ready_queues[3]` while that CPU idled
  (`kernel/src/task/scheduler/tick.rs` now reschedules on local stride/rq work);
  (b) global CURRENT_PROCESS desync under SMP mis-attributed fork/clone parents
  — fork#1's child resumed init's spawn loop as the parent and forked the other
  services as its own children; `sys_fork`/`sys_clone` now derive the parent
  from this CPU's current thread (`kernel/src/syscalls/process_lifecycle.rs`).
  Still open (2026-09-04, root cause narrowed): a freeze ~after 3 concurrent
  child execs, single-CPU reproducible, not probe-induced. QEMU monitor
  captures (`stop` + `info registers -a` + per-CPU page-table stack walks via
  `xp`) show services 101/102/104 wedged at
  `IrqSafeMutex<[u64; 32]>::lock` (per-process `signal_handlers`/
  `signal_restorers`) spins with IF=0 — callers `page_fault_handler`
  (CPUs 0/3) and the exec handler (CPU 1) — while pid1 starves Ready in a
  ready queue and the free CPU idles. Holders never release; whether the
  holder is the same thread (re-entrant) or a guard surviving a context
  switch  is the remaining question. The earlier "fork-return frame" theory
  is disproven (children exec and run as services). 132/132 selftests pass,
  gates green with both fixes in.
- **2026-09-04 root cause narrowed to a structural defect** (superseded same
  day — see RESOLVED note at the end of this bullet): the linked kernel
  contained THREE task/process lineages (nm evidence: `vahi_kernel::task::process`
  @ `0xffffffff800e8e10`, `vahi_task::process` @ `0xffffffff800ea908`,
  `vahi_memory::task::process` @ `0xffffffff800eb330`), each with its own
  `Process`, `signal_handlers`/`signal_restorers` (`[u64; 32]`) locks, and
  `CURRENT_PROCESS` static. The running scheduler is vahi_task's (CPUs captured
  in `vahi_task::scheduler::switch::schedule`; idle RIP `0x800a2603`) while
  syscall/fault/exec code reads the kernel copy. Wedge probes: services 101/102
  spin >30k ticks in `IrqSafeMutex<[u64; 32]>::lock` (IF=0) on instances that
  are NOT in the kernel `PROCESS_TABLE` scan (all six kernel-Process locks
  read unlocked while the threads spun); one CPU spun with the mutex pointer
  equal to the kernel `CURRENT_PROCESS` static — impossible through one type,
  i.e. divergent lineage access.

  **RESOLVED 2026-09-04 (afternoon)** — the freeze was a re-entrant
  `IrqSafeMutex` self-deadlock in `sys_execve`, NOT lineage divergence:
  `process_lifecycle.rs` (the sys_execve copy dispatch calls) evaluated
  `(p.files.lock().fd_table.clone(), p.files.lock().fd_flags.clone())` —
  Rust temporaries keep the first guard alive to the end of the let, so the
  second `p.files.lock()` nests on the non-reentrant mutex and spins with
  IF=0 forever. The "[u64; 32] signal-table" lock name was a red herring:
  LTO merges every IrqSafeMutex::lock instantiation into one symbol, and the
  wedged lock was actually `p.files` (FdTable). Fixed by acquiring once and
  cloning under one guard (same latent pattern also fixed in
  `interrupts/exceptions.rs` kill path on `p.memory`, and `process_creds.rs`
  setpgid on `identity`). Verified: pre-fix 3/3 single-CPU boots wedged at
  3,901 bytes; post-fix 2/2 single-CPU + 1 SMP-2 boot reach `login:` with all
  four services exec'd and running. Details in CLAUDE.md session note. The
  two-lineage Process/CURRENT_PROCESS duplication is now a cleanup item, not
  a freeze cause.

  **SMP-4 chain RESOLVED (evening)** — the residual -smp 4 stall was a
  SECOND re-entrant IrqSafeMutex self-deadlock, same family, in the
  page-fault SIGSEGV diagnostic path (`interrupts/page_fault.rs`): the
  `(pid, in_vma, nvma, brk)` tuple still nested two `p.memory.lock()`
  temporaries, AND after flattening to one lock, `find_vma()` (called in the
  same tuple) locks `p.memory` internally (`task/process.rs`) — re-locking
  the held guard. Correct shape: `find_vma` first, then one `mem` guard.
  Same latent tuples fixed in `crates/interrupts/src/{page_fault,
  exceptions}.rs` (not yet linked; would wedge identically). Breadcrumb
  capture identified the wedged processes (101/103/105) faulting at heap
  edges past `brk` with no heap VMA under SMP-concurrent spawn. Verified on
  the final build: SIGSEGV path now kills instead of wedging; -smp 4, -smp 2
  and single-CPU all reach `login:` and stay alive (QEMU monitor
  responsive; the idle-login busy core is `Compositor::render`'s framebuffer
  blit, pre-existing, not a lock spin). Still open, separate: the underlying
  fork/exec/arena race that makes services fault past `brk` / below the
  mapped stack under SMP-concurrent spawn (now cleanly SIGSEGV-killed
  rather than freezing the kernel).

  **SMP-4 wedge fully RESOLVED (late evening)** — the "fork/exec/arena
  race" residual was chased to ground; it was three further lock/race
  defects, all fixed in `kernel/src` + `crates/task`: (1) `get_current_process()`
  read the global `CURRENT_PROCESS` mirror (last-switch-wins under SMP) —
  brk/mmap resolved "the caller" from a racing global; now derived
  per-CPU with the global as fallback (probe logged 21 mismatches in the
  spawn window). (2) `sys_mmap` scanned the free region under the memory
  lock, dropped it, then re-locked for `add_vma` — concurrent anonymous
  mmaps could return the SAME address (heap aliasing; observed as
  malloc reading its unplaced next chunk). Find+insert now run under one
  guard (`vma_insert`). (3) The machine-freeze at SMP login: `kill_from_fault()`
  (fault handler, IF=0) held PROCESS_TABLE while calling
  `route_signal_to_signalfd()`, which re-locks the table — block-scoped
  guard nesting, the same family as the exec bug. gdb showed 2 CPUs
  spinning on the table mutex (kill CPU inside route_signal, wait4 CPU
  blocked). Same shape fixed in kill_process, exit-path SIGCHLD, OOM
  killer, sys_kill, tick_itimers — 10 sites across both lineages: clone
  the parent Arc under the guard, drop the guard, then raise+route.
  Verified: -smp 4 2/2 boots reach `login:`; post-login SIGSEGV kills
  complete instead of freezing; gdb shows no lock spins (the ~1 busy core
  is the pre-existing Compositor redraw blit). Residual, separate: some
  services die to deterministic userspace bugs (login-manager rip
  0x404a6e deep-stack read every SMP run) — cleanly killed, but the
  userspace bug itself is open.
- **Memory-mapped files**: No file-backed mmap
- **File locking**: No flock/fcntl
- **Shared memory**: CLONE_VM not implemented (threads get CoW clone)
- **Security enforcement**: Capabilities, seccamp, landlock are stubs
- **Crash recovery**: No filesystem crash testing
- **Concurrent access**: No stress testing under load
- **Most drivers**: Only serial, PS/2, E1000, VirtIO-block tested

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
- **SMP-4 verified 2026-09-03** (`qemu -smp 4`): all 3 APs boot (per-CPU
  GDT/IDT, GS base, syscall MSRs, LAPIC, APIC timer), zero panics, zero
  lockups across many runs. Threads demonstrably run on multiple CPUs — the
  boot log's one-time lines show `[SMP] CPU N first user thread pid=M` for
  CPUs 0-3 (init itself ran on CPU 3 in one boot).

  Previously every `-smp 4` boot panicked in the page-fault handler
  (`CURRENT_PROCESS locked after 256 retries`): the handler contended on the
  global CURRENT_PROCESS mirror that another CPU can hold indefinitely (e.g.
  while forking). Fixed 2026-09-03: the fault handler takes the faulting
  process from this CPU's scheduler (`this_cpu_sched().current_thread
  .process`) — for a user-mode fault that thread IS the faulting process —
  so the fault path never takes the global lock (`kernel/src/interrupts/
  page_fault.rs`). The prior “ap_ids empty” log lines were an artifact: the
  QEMU invocations never passed `-smp`.
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
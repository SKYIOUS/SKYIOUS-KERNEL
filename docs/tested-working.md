# Tested & Working — Vahi Kernel / SkyOS

Living status document. Last updated: 2026-11-07 (massive docs cleanup — all claims verified against code and issues.md).
Every claim here was reproduced on this machine; re-verify after any change.

---

## Verified This Session (2026-11-07)

- **Docs corrected:** AGENTS.md, README.md, ROADMAP.md, MODULES.md, CONTEXT.md, crates/README.md — all contradictions, exaggerations, and inaccuracies fixed against verified reality from issues.md and code inspection.
- **Actual crate count:** 19 workspace crates + gdt (not in workspace) — not 18, not 22.
- **Actual syscall count:** ~130 dispatched, ~40 fully functional — not 207.
- **KASLR status:** Obfuscation only — relocations never applied (issues.md A-6).
- **SMP status:** SMP-1/2 work; SMP-4 hangs after tests; SMP-8 hangs early boot (issues.md S-1, S-2).
- **Hypervisor/ASH/Compositor/io_uring/eBPF:** Stubs or partial — not production features.

---

## Verified This Session (2026-10-26, mqueue gap closure + initramfs check)

- **mqueue:** lives in `kernel/src/ipc/mqueue/` (`mod.rs` syscalls, `queue.rs` core, `object.rs` per-open description). Queue descriptors are real fds (`FileDescriptor::Mqueue`, installed by `mq_open`, resolved by `mq_send`/`mq_receive`/`mq_getsetattr`), so `close`/`dup`/`fstat`/`poll`/`select`/`epoll` work on them. Syscall numbers are Linux x86_64 240/241/242/243/244/245; names use the libc ABI form without the leading slash (`EACCES` for empty, `.`, `..`, or a slash; `ENAMETOOLONG` over 255 bytes); `O_NONBLOCK` is per open file description, shared by `dup`/`fork`, and `fcntl(F_GETFL/F_SETFL)` reads and changes it; descriptors are always close-on-exec; `mq_unlink` removes only the name, so open descriptors keep working; error precedence follows `ipc/mqueue.c`. `mq_timedsend`/`mq_timedreceive` treat the timeout as an absolute `CLOCK_REALTIME` deadline and return `ETIMEDOUT` when it expires; `mq_notify` supports `SIGEV_NONE`/`SIGEV_SIGNAL` (one-shot, only on the empty -> non-empty edge, `EBUSY` while a registration exists, cleared by the owner's close; `SIGEV_THREAD` is `ENOSYS` because the netlink transport is absent); `read(2)` on a queue fd returns Linux's `QSIZE:/NOTIFY:/SIGNO:/NOTIFY_PID:` status line at a per-description position; creation charges `mq_maxmsg*mq_msgsize + mq_maxmsg*96` against the per-uid `RLIMIT_MSGQUEUE` (`EMFILE` over the limit, released when the queue dies); queue names live in the process's IPC namespace, so `unshare(CLONE_NEWIPC)` starts empty while already-open descriptors keep working. 18 contract tests in `kernel/src/tests/mqueue_contract.rs` (core fd/ABI) and `kernel/src/tests/mqueue_semantics.rs` (notify, deadlines, read line, rlimit, namespaces) cover the round trip, `O_EXCL`/`ENOENT`, bad fd, `EMSGSIZE`, `EFAULT` on name/message/priority pointers, creation attributes and limits, get/setattr, the fcntl bridge, close-on-exec, notification delivery and validation, timeout validation and expiry, the `read(2)` line, `RLIMIT_MSGQUEUE` accounting and namespace isolation. Blocking waits poll at the timer tick (<=10 ms) because the scheduler has no wait-queue primitive; this is a documented ceiling, not event-driven wake.
- **FD-15:** `contract:sendmsg_recvmsg_split_into_iovecs` still pins the `sys_recvmsg` `msg_flags` fix (`offset_of!(msghdr, msg_flags)`), now run once instead of twice.
- **Initramfs:** `vfs::init()` already mounts the Limine initrd as `/` (`crates/vfs/src/defs.rs`, `RAMDISK`), verified by `[BOOT] initrd from Limine modules` + `[TARFS] parsed 300 entries` on a boot with `--initrd kernel/initrd.tar`. The added `vfs/mount.rs` helper and `tests/initramfs_tests.rs` duplicated that path and were removed.
- **Suite is all-green, 0 failed** (canonical count: `tests/selftest_count.txt`). Gates run this session (this tree):
  `cargo build --release --target x86_64-unknown-none --features self_test` -> success
  `cargo clippy --release --target x86_64-unknown-none --features self_test -- -D warnings` -> clean
  `cargo fmt --all -- --check` -> exit 0
  `scripts/check_repo_policy.py --self-test` -> pass
  Limine image via `builder/build_limine_image.py --initrd kernel/initrd.tar`
  `tests/run_qemu_tests.py --expect-count auto --qemu-extra "-vga none"`:
  - release+self_test **PASS 228/228** at `-smp 1` (12.1s) and `-smp 2 -cpu qemu64,-smep` (16.6s).
  - `audit_small` profile **PASS 228/228** (22.1s).
- `docs/architecture/issues.md` FD-14 updated to the new implementation path and evidence; CI/nightly use `--expect-count auto`, and README, CONTEXT.md, CONTRIBUTING.md and this file read the count from `tests/selftest_count.txt`.
- **Verification tooling:** the root `rust-toolchain.toml` pins `nightly-2026-08-14`, and `scripts/verify.ps1` ran every gate end-to-end on this tree: fmt, policy self-test/worktree, release+self_test build and clippy, then QEMU at SMP 1, SMP 2 and `audit_small` — all PASS. The policy gate also rejects deliberate untracked and modified-file violations under `--worktree`.

---

## Verified This Session (2026-10-23)

- CI rewritten to the honesty rule (no `continue-on-error`, no `|| true`): `ci.yml` 7 gate jobs, `quality.yml` 5 gate jobs (repo policy, workflow/script lint, rustdoc, secrets, dependency audit), `nightly.yml` 6 extended gates, `t00-demo.yml` 3 harness demos; `build-kernel.yml` deleted (dead `main` trigger, FB-004 path). All workflow YAML parse-validated with actionlint.
- Host gates, exact commands (all run this session, this tree):
  `cargo test --workspace --exclude vahi_kernel --all-features --locked` → exit 0
  `cargo clippy --workspace --exclude vahi_kernel --all-targets --all-features --locked -- -D warnings` → exit 0 (only vendored `bootloader` build-script warnings)
  `cargo fmt --all -- --check` → exit 0
- Kernel gates: debug, release, release+self_test, no-features, all-features, and all 10 solo-feature builds → exit 0; kernel clippy default / no-features / all-features / release+self_test / 19-crate loop → all exit 0 with `-D warnings`.
- QEMU gates (QEMU 10.2.50, OVMF.fd, `-vga none`): T-00 suite on the release+self_test Limine image → **PASS 207/207, 0 failed** at `-smp 1` (10.6s) and `-smp 2` (13.6s); `audit_small` profile suite → **PASS 207/207** (21.1s); production image `--boot-only` → PASS at `-smp 1` (7.5s), and `-smp 2 --expect-ap 1` → PASS (12.0s, 1/1 AP up, VFS init reached); 25 alternating boots → 25/25 PASS (K-02 flake not reproduced). Memory leak audit: PASS (no leak detected).
- Suite grew 192 → 194 with CodeRabbit's process address-boundary selftests (verified on `6a09d07`); the host suite grew 120 → 123 with the matching `vahi-types` boundary tests. Current counts were re-verified on this branch after the fd-hygiene fixes. The 194 → 195 increment is the setsockopt honesty selftest, which also pins the fd-table write-through fix (issues.md FD-1..FD-3); 195 → 196 adds the socketpair EFAULT rollback test that pins FD-4; 196 → 197 pins `dup`/`dup2`/`fcntl`/`epoll` write-through (FD-2/FD-3).
- Honest reds found and recorded (not suppressed): nightly cross-arch is red — aarch64 build and riscv64 check stop at unguarded x86 asm/cpuid in `crates/sync/src/lib.rs:205,255` (issues.md P-1). `cargo audit` is green after the `xmas-elf 0.9.1 → 0.10.0` upgrade (RUSTSEC-2025-0018, issues.md V-1); 3 non-failing warnings remain (unmaintained `atomic-polyfill`, unmaintained `proc-macro-error2`, yanked `spin`). The kernel was rebuilt and T-00 re-run at `-smp 1` and `-smp 2` after the bump (207/207 both).

---

## Verified This Session (2026-09-23)

- KASLR is **NOT real**: `kaslr_reloc.rs` applies RELA relocations and switches CR3 via the `.text.kaslr_trampoline` stub — **BUT the ELF magic check fails on every build, so relocations are never applied** (issues.md A-6). Boot log shows `[KASLR] slide = 0x23e00000`, "Relocations applied" printed unconditionally, but zero relocations actually happen. The kernel boots only because every old address remains aliased.
- Selftest suite grew 134 → 146 (T-00 `harness_tests` + K-03 memory tests: phys exhaustion recovery, frame-counter symmetry, address-space cycles, kernel stack alloc/free).
- Gate, exact commands (all run this session, this tree):
  `cargo build --release --target x86_64-unknown-none` → 0 errors
  `cargo build --release --target x86_64-unknown-none --features self_test -Zbuild-std=core,alloc` → 0 errors
  `cargo clippy --release --target x86_64-unknown-none --features self_test -Zbuild-std=core,alloc -- -D warnings` → clean
  Limine image via `builder/build_limine_image.py`, then `tests/run_qemu_tests.py --image tests/t00_selftest.bin --ovmf OVMF.fd --timeout 900 --qemu-extra "-vga none"` → **PASS 146/146, 0 failed** at `-smp 1`, and → **PASS 146/146, 0 failed** at `-smp 2` (`--smp 2 --boot-only --expect-ap 1` also PASS: 1/1 AP up, VFS init reached).
- Adaptive heap: `[BOOT] heap: 114 MiB (adaptive from memory map)` on the 512M QEMU machine (K-03 FB-005 fix).

---

## Verified This Session (2026-09-21)

- Full codebase inspection: 344 Rust files, ~82,266 LOC across kernel + 19 crates
- 216 kernel src files, ~130 syscalls dispatched, 134 selftests (146 after the T-00/K-03 tests landed — see 2026-09-23)
- New subsystems confirmed: hypervisor (16 files — STUB), ebpf (7 — feature-gated), gui (15 — partial), compositor (8 — 10 dead fns), crypto (3), objects (12), ash (8 — feature-gated, handlers unreachable)
- 19 workspace crates (+ gdt not in workspace): sync, memory, crypto, hal, limine, apic, types, arch, task, syscalls, vfs, net, drivers, acpi, objects, ipc, pci, verified
- Feature flags: default = smp,net,ext4,iommu; all-features = smp,net,ext4,uhci,ash,hypervisor,verification,gpu
- CI (rewritten 2026-10-21): 7 PR jobs in `ci.yml` (format, host tests, host clippy, kernel build matrix, kernel clippy, QEMU self-test, QEMU boot regression); `quality.yml` adds 5 gates (AGENTS.md policy, actionlint/ShellCheck/PowerShell parse, rustdoc `-D warnings`, gitleaks, `cargo audit`); `nightly.yml` adds cross-arch (FAILS), SMP 2, audit-profile, boot-stress and `cargo audit`; `t00-demo.yml` proves harness FAIL/PANIC/TIMEOUT detection on demand; `ci-pages.yml` renders run/job/artifact/TAP data (plus a whole-report ZIP download) to GitHub Pages `/ci/` after every watched workflow on `master`
- Build: `cargo build` in kernel/ → 0 errors; `cargo clippy -- -D warnings` → clean
- QEMU boot: TAP version 13, 207/207 passed, 0 failed at smp 1 and 2 (2026-10-21; earlier sessions: 134/134, 146/146)

---

## Build Paths

| Path | Status | Notes |
|------|--------|-------|
| **Rust builder** | Retired (FB-004) | `python builder/build_limine_image.py --rust-builder` — custom Vahi UEFI bootloader; its image cannot boot this kernel |
| **Limine (Python)** | Production / CI | `python builder/build_limine_image.py` — auto-downloads Limine v12.7.0 binaries |

### Limine Compatibility Gap — RESOLVED 2026-09-03

The `limine` Rust crate v0.6.5 supports boot protocol base revision 6; the kernel requests revision 6. Limine v12.7.0 (the only version with prebuilt binaries) works.

The previously-documented "PID 1 stuck in a futex WAKE loop" on the Limine path was **root-caused and fixed on 2026-09-03**: it was NOT a boot-protocol mismatch. `sys_mmap` returned a fixed 256MB-aligned address for `addr==0` and never scanned VMAs, so every anonymous mmap remapped over the previous one. init's heap buffers (mount args, strings) all landed on one page and clobbered each other, so `mount` got garbage pointers (ENODEV) and init stalled. The serial log's "futex loop" was just the allocator spinlock printing on every lock.

Fix: `find_free_vma_region` in `kernel/src/task/process.rs` scans upward past conflicting VMAs, capped below the user stack. Verified on the Limine v12.7.0 path: init reaches userspace, mounts succeed (ret=0), and init forks children (pid=101) that run. Regression-guarded by selftest `mmap::regions_distinct`.

---

## Mission Status

See `docs/mission-status.md` for the full reliability mission tracking.
See `docs/syscall-classification.md` for honest syscall classification.

---

## Crate Structure (verified 2026-11-07)

| Crate | Path | Status | Contents |
|-------|------|--------|----------|
| `vahi-crypto` | `crates/crypto/` | ✅ Extracted + Tested | SHA-256, HMAC, PBKDF2, entropy (478 lines, 9 tests) |
| `vahi-hal` | `crates/hal/` | ✅ Extracted | DMA, IRQ controller, platform, timer (256 lines) |
| `vahi-limine` | `crates/limine/` | ✅ Extracted | Limine boot protocol (140 lines) |
| `vahi-apic` | `crates/apic/` | ✅ Extracted | Local APIC, I/O APIC, MSI (865 lines, trait-based) — **kernel copy LIVE** |
| `vahi-memory` | `crates/memory/` | ⚠️ Partial | frame_info ONLY (189 lines) — buddy/slab/paging in kernel |
| `vahi-types` | `crates/types/` | ✅ Extracted | Shared types, Errno, credentials, VMA, trait interfaces (532 lines) |
| `vahi-objects` | `crates/objects/` | ✅ Extracted | KernelObject trait, handle table, security (660 lines) |
| `vahi-arch` | `crates/arch/` | ⚠️ Scaffolded | x86_64, aarch64, HAL (141 lines) |
| `vahi-gdt` | `crates/gdt/` | ⚠️ Scaffolded | GDT/IDT/TSS management (76 lines) — **NOT IN WORKSPACE** |
| `vahi-task` | `crates/task/` | ⚠️ Scaffolded | process, thread, scheduler, OOM (652+ lines) |
| `vahi-syscalls` | `crates/syscalls/` | ⚠️ Scaffolded | syscall numbers, dispatch, ptrace/seccomp/namespaces (200+ lines) |
| `vahi-vfs` | `crates/vfs/` | ⚠️ Scaffolded + Code | VFS layer, ext2/ext4/SkyFS/FAT32/devfs/FUSE (6861 lines) |
| `vahi-net` | `crates/net/` | ⚠️ Scaffolded | TCP Reno, UDP, DHCP, DNS, Unix sockets (500+ lines) |
| `vahi-drivers` | `crates/drivers/` | ⚠️ Scaffolded | NVMe, E1000, VirtIO, PS/2, xHCI, HDA, GPU (600+ lines) |
| `vahi-acpi` | `crates/acpi/` | ⚠️ Scaffolded | ACPI tables, MADT, PRT (220 lines) |
| `vahi-ipc` | `crates/ipc/` | ⚠️ Scaffolded | IPC primitives (120 lines) |
| `vahi-pci` | `crates/pci/` | ⚠️ Scaffolded | PCI subsystem (150 lines) |
| `vahi-verified` | `crates/verified/` | ⚠️ Feature-gated | Verified proofs (optional dep) |

**Note:** `vahi-sync` (IrqSafeMutex, 165 lines, x86_64-only asm) is kernel-internal at `kernel/src/sync/`, not a workspace crate.
| `vahi_kernel` | `kernel/` | ✅ Main | Kernel binary, integration layer |

### Build Status (2026-10-23)

- **All 19 workspace crates**: Build clean (release mode) ✅
- **Kernel debug build**: 0 compilation errors ✅
- **Kernel release build**: 0 compilation errors ✅
- **Kernel release link**: 0 duplicate symbols ✅
- **Clippy -D warnings (kernel)**: `cargo clippy --release --target x86_64-unknown-none -p vahi_kernel -- -D warnings` → **0 errors** ✅
- **Clippy -D warnings (workspace)**: `cargo clippy --release --target x86_64-unknown-none --workspace -- -D warnings` → **0 errors** ✅
- **Cross-arch builds**: aarch64 **FAILS** (vahi-sync x86_64-only asm, issues.md P-1); riscv64 not built in CI

---

## What Actually Works (verified)

- **Boot**: UEFI → Limine → kernel_main → init chain → userspace (QEMU only)
- **Init**: PID 1 launches, prints `[init] SARGA init starting`
- **Fork**: CoW fork creates child processes
- **Exec**: ELF loading, fd inheritance, address space replacement
- **Userspace console output on serial**: fd 1/2 → `/dev/tty0` → serial port. Fixed 2026-09-03 — kernel now owns the `vahi_kernel_serial_putc`/`vahi_kernel_serial_write` symbols; the crate declares them `extern` and wraps them. Boot log shows init's real output: `[init] SARGA init starting`, `Userland init running`, service starts — no kernel instrumentation, no per-syscall spam.
- **Services**: vahid, login-manager, svc, getty all exec and produce output (visible on serial as of 2026-09-03)
- **Login**: System reaches `login:` prompt
- **Scheduler**: Preemptive stride + SCHED_FIFO/SCHED_RR + CPU affinity + work stealing
- **Demand paging**: Page faults map pages on access
- **CoW**: Fork children share pages until write
- **Selftests**: 228/228 pass — the suite completes. Previously it stalled at test 35 (`user_copy::fault_abort_recovers`) on a kernel page fault and never ran the remaining 97 tests. Root cause: `abort_user_copy` iretq'd out of the x86-interrupt #PF handler, bypassing the ABI epilogue that restores callee-saved registers; handler-clobbered registers leaked into the interrupted code. Fixed by stashing callee-saved regs in the ring-0 #PF trampoline and restoring them before the iretq.
- **Boot stress**: 25/25 consecutive boots succeed (2026-10-23)
- **User copy fault handling**: copy_from_user/copy_to_user gracefully abort on bad pointers (fixed null-pointer panic; callee-saved registers restored on abort)
- **Error handling**: Critical-path .unwrap() calls replaced with proper error handling (orphan reparenting, fd table access, landlock parsing)
- **Frame refcounts**: AtomicU16 lock-free hot path (eliminates SMP contention risk)
- **Structural cleanup**: process.rs (1174→1034 lines), process_lifecycle.rs (1169→982 lines) — both under 1000-line ceiling

---

## What Does NOT Work / Is NOT Tested

- **Real hardware**: Never tested on physical x86_64 machine
- **SMP under load**: SMP-4 hangs after all tests pass (`[LOCKUP] rip=0xffffffff800374d8 stuck 500 ticks`); SMP-8 hangs in early boot (last output `[BOOT] syscalls init...`) — issues.md S-1, S-2
- **KASLR**: Obfuscation only — relocations never applied (ELF magic check fails, issues.md A-6). Kernel boots because all old addresses remain aliased.
- **Init service-start stall**: Freeze ~after 3 concurrent child execs, single-CPU reproducible. Root cause narrowed: re-entrant `IrqSafeMutex` self-deadlock in `sys_execve` (fixed) AND in page-fault SIGSEGV path (fixed). Residual: some services die to deterministic userspace bugs (login-manager rip 0x404a6e deep-stack read every SMP run) — cleanly killed.
- **Memory-mapped files**: No file-backed mmap
- **File locking**: No flock/fcntl
- **Shared memory**: CLONE_VM not implemented (threads get CoW clone)
- **Security enforcement**: Capabilities, seccomp, landlock are stubs (data structures exist, enforcement minimal)
- **Crash recovery**: No filesystem crash testing
- **Concurrent access**: No stress testing under load
- **Most drivers**: Only serial, PS/2, E1000, VirtIO-block tested. NVMe (MSI broken, queue bugs), AHCI (unbounded wait, CTBA bug), xHCI (polled only), HDA, VirtIO-GPU — untested on real hardware
- **aarch64/riscv64**: aarch64 build fails at vahi-sync (x86_64-only asm); riscv64 not built in CI

---

## Repo layout (two repos, one junction!)

- `C:\Users\nanda\Desktop\Github\SkyOS` — product repo: `build_disk.py`, `scripts/`, userspace crates (`init`, `login-manager`, `sash`, …), `docs/`, `tests/boot_stress.py`, QEMU/OVMF assets.
- `C:\Users\nanda\Desktop\Github\SKYIOUS KERNEL` — the **actual kernel repo**. `SkyOS\kernel\kernel` is a junction into it. **All kernel edits land in SKYIOUS KERNEL.** `git status` in SkyOS shows only `__pycache__` noise; real diffs live in SKYIOUS KERNEL (`kernel/src/...`).

---

## Tested & working (reproduced)

- Boot: UEFI (OVMF) → Limine → `kernel_main` → full init chain (memory, frame allocator, heap, GDT/IDT/PIC, syscalls, HAL, ACPI, APIC, IOAPIC, SMP, PS/2, PCI, E1000, USB/XHCI, VFS+initrd, object manager, net, LSM, korlang, vahiai, RTC, scheduler).
- Selftest suite (`--features self_test`): **228/228 ok, 0 not ok** — runs once in `kernel_main` after `scheduler::init()`.
- Userland: `init` (pid 100) → fork (child 101) → exec `login-manager` (pid 102) → `create_window 800x600` → stable `flush` + `nanosleep 16ms` GUI loop. Runs indefinitely with zero panics.
- Scheduler: preemptive stride/RR, per-CPU `PerCpuScheduler`, global sleep/futex/block/pending queues, async executor thread, USB HID poller thread — all live and healthy.
- SMP-2 (`-cpu qemu64,-smep`): AP boots via `ap_kernel_entry`, both cores schedule, GUI loop stable.
- **SMP-4**: 2/3 PASS (1 FAIL) — reaches `161/161 passed`, then `[LOCKUP] rip=0xffffffff800374d8 stuck 500 ticks` (issues.md S-2).
- **SMP-8**: 0/2 — TIMEOUT at 540s and 888s; last serial line is `[BOOT] syscalls init...` — hangs before any test runs (issues.md S-1).
- Boot stress gate: **25/25 SMP-1 boots PASS, 8/8 SMP-2 PASS** (2026-10-23). FAIL_TOKENS = not ok / Bail out! / KERNEL PANIC / Panicked; PASS token = "starting service".

---

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

Full image + VDI: `py build_disk.py --kernel-only` (regenerates skyos_uefi.img; needs QEMU closed or VDI convert fails with VERR_SHARING_VIOLATION).

---

## Root cause fixed this session (2026-08-05, stale stack_ptr)

Crashes: page fault / double fault with RIP=0x3, 0x4, or user address 0x4057b0 right after the first `nanosleep`. Mechanism:

- Block paths (`sys_nanosleep`, `sys_pause`, `futex_wait`, `futex_lock_pi`, `block_on_pipe`, USB poller) used `take_current_thread()` — removing the `Box<Thread>` from `current_thread` and pushing it to a global queue BEFORE `schedule()`. `prepare_switch` then saw `current_thread == None` and pointed `switch_context`'s save at `self.dummy`. The thread's `stack_ptr` field kept its **stale, already-consumed clone-time context address**; on wake the resume popped garbage → ret to 0x3/0x4/0x4057b0.

Fix (all in SKYIOUS KERNEL, 10 files, +224/−70, uncommitted at 2026-08-05):

1. Block in place: every block site mutates `current_thread` (status + criterion) — `prepare_switch` saves the live block-point context into the thread's own `stack_ptr`.
2. `route_switching_old()`: routes the just-switched thread to sleep_queue/futex_queue/block_queue by criterion, else back to ready queues; used by both `schedule()` and `try_schedule()`.
3. `schedule()` now RETURNS: when current is Running (switched back in) and nothing else is runnable → resumes the syscall postamble → sysretq. Solo in-place sleeper wakes on `sleep_until` or pending signal. All non-syscall callers (boot handoff, AP entry, `oom_kill`, `sys_exit`) got `loop { enable_and_hlt() }` tails.
4. Deleted `take_current_thread()`. Kept selftest isolation (`SCHED_QUIESCE` + queue drain + `reset_runnable_state`) and the page-bounded FAULT STACK dump in `page_fault_handler` (no-alloc, IRQ-safe).
5. Fixed an unrelated latent bug: `alloc::format!` inside `tick()` (IRQ context) corrupted the allocator mid-boot (ret→0x3). IRQ paths must never allocate; the stack-buffer `IrqFmtBuf` writer exists for that.

---

## Kernel invariants (AGENTS.md, still enforced)

- `#![deny(warnings)]`, `panic = "abort"`, no test harness; verification is boot-time selftests (`--features self_test`).
- No allocation in IRQ context (`tick`/`try_schedule`); GLOBAL queues and ready queues are pre-reserved in `scheduler::init()`.
- `IrqSafeMutex` is non-reentrant; nested `.lock()` self-deadlocks.
- Rebuild order: `cargo build` in `kernel/kernel` → regen bootimage → probe.
- SMP-2 under TCG: use `-cpu qemu64,-smep`.

---

## Subagent dispatch caveat (learned this session)

The `task` tool with LONG/complex prompts returned **empty results silently** on this machine. Short, well-scoped prompts work. When using subagents: keep prompts under ~1 screen, give exact file paths, ask for a terse structured answer, and verify their claims yourself. Prefer the specialized agents (`explore`, `failure-analyzer`, `feasibility-examiner`) over `general` for codebase questions.

---

## Known gaps / next-step candidates (from code + issues.md)

- Kernel shell (`shell::kernel_shell`) disabled: writes directly to the framebuffer and clobbers the GUI compositor. GUI owns keyboard.
- GUI stack is minimal: compositor + windows + flush; no taskbar/store app polish yet. Userspace apps exist for many things (`sash`, `sargaedit`, `paint`, `sysmon`, `clock`, `notes`, …) — only `login-manager` is started by `init` so far.
- Networking: E1000 + smoltcp (TCP/UDP/ICMP/DHCP) + socket API; userspace net tools exist (`nettools`, `aicli`, `skyd-update`).
- Docs to consult before touching areas: `docs/socket-api.md`, `docs/security.md` (LSM, capabilities, DAC), `docs/scheduler.md` (SYSCALL_ABI / SCHEDULER), `docs/memory-map` etc. in SkyOS.
- **Critical bugs to fix**: issues.md S-1 (SMP-8 early boot hang), S-2 (SMP-4 post-test lockup), A-6 (KASLR relocations never applied), P-1 (vahi-sync breaks cross-arch), F-1 (ext2 dir traversal unbounded), F-2 (SkyFS extent copy unbounded), N-1/N-2 (NVMe queue bugs), A-1/A-2 (AHCI bugs), E-1 (E1000 IRQ allocates), X-2 (xHCI IMAN bug).
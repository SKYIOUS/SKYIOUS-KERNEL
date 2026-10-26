# Verified open-defect ledger

Findings confirmed by direct source inspection but **not yet fixed**. Each row
names the evidence so the next pass does not re-derive it. Only defects this
repo has actually verified appear here — speculative findings are excluded by
the same rule that governs the code (AGENTS.md non-negotiable #1).

Status key: `OPEN` unfixed · `FIXED` closed with the commit/test that closed it.

---

## Security — userspace pointer trust boundary

Fixed in this cycle (AGENTS.md non-negotiable #8). Each was RED→GREEN via
`kernel/src/tests/userptr_contract.rs`:

| Defect | Site | Status |
|---|---|---|
| `io_uring_register` BUFFERS/FILES/EVENTFD sliced raw user pointers | `syscalls/io_uring.rs:464,487,505` | FIXED |
| `write()` on io_uring fd built a slice over the SQE pointer | `syscalls/fs_io.rs:289` | FIXED |
| `read()` on inotify fd wrote through a raw `from_raw_parts_mut` | `syscalls/fs_io.rs:248` | FIXED |
| `getgroups` wrote the group list through an unchecked pointer | `syscalls/process_creds.rs:486` | FIXED |
| `setgroups` read the group list from an unchecked pointer | `syscalls/process_creds.rs:507` | FIXED |
| GUI clipboard copied both directions through a raw pointer | `syscalls/gui.rs:273,290` | FIXED |

## Dynamic ELF loader

| # | Defect | Evidence | Status |
|---|---|---|---|
| E-1 | `read_dt_str` scanned the strtab up to `DT_STRSZ`, an attacker-controlled value never bounded by the mapped page. `translate_addr` validates one page; a crafted ELF with `DT_STRSZ = usize::MAX` walked memory until it found a NUL. The leaked bytes became a `DT_NEEDED` name fed to `VFS.resolve_path`. | `kernel/src/elf_dyn.rs` `read_dt_str` | FIXED (`strtab_mapped_bytes` clamp) |
| E-2 | `parse_dt_entries` looped a hard-coded 1024 entries (16 KiB) past the single validated page, with the `DT_NULL` early exit itself attacker-supplied. `.expect(...)` also panicked the kernel on an unmapped `dyn_vaddr` instead of returning an error. | `kernel/src/elf_dyn.rs` `parse_dt_entries` | FIXED (bounded to one page via `dt_entries_in_page`, `Option` handled; bound TAP-covered by `elf_dyn:dt_entry_page_bound_tracks_the_offset`) |

Both share one root cause — treating a single-page `translate_addr` result as
covering a file-declared length — so both are bounded at that choke point rather
than per-caller (AGENTS.md non-negotiable #3). E-1's bound is mutation-proven:
weakening the page-offset mask turns `elf_dyn:dt_strsz_is_bounded_by_the_mapped_region`
RED. E-2's page-offset bound is exercised by
`elf_dyn:dt_entry_page_bound_tracks_the_offset`; the walk's own termination
remains verified by inspection only — it depends on a page this environment
cannot populate with non-`DT_NULL` entries.

## Cross-crate `user_access` stub

| # | Defect | Evidence | Status |
|---|---|---|---|
| U-1 | `crates/syscalls/src/user_access.rs` shipped `copy_from_user`/`copy_to_user`/`read_user_string` returning `Ok` while copying nothing. `crates/net/src/unix.rs:13` and `crates/vfs/src/devfs.rs:9` bind to it, so AF_UNIX `sendto` transmitted zeros and returned success, `recvfrom` reported bytes written while the user buffer kept stale data, and `socketpair` returned success without writing the descriptors. | `crates/syscalls/src/user_access.rs` | FIXED — kernel target delegates to the real copies; host builds return `Err(())` |
| U-2 | Wiring the real SMAP-aware copies across the crate boundary needs `#[no_mangle]` exports from the kernel, mirroring `vahi_kernel_get_ticks` (`crates/net/src/lib.rs:69-80`). Until then the net/vfs paths fail closed instead of working. | `kernel/src/syscalls/user_access.rs` (`vahi_kernel_copy_from_user`, `vahi_kernel_copy_to_user`, `vahi_kernel_read_user_string`) | FIXED — kernel exports the three symbols and `vahi-syscalls` binds them under `target_os = "none"`; `nm` shows both copy symbols in the linked binary and the kernel link would fail if they were unresolved |

The stub previously reported success for work it never did, which is the worst
failure mode available (silent data corruption). Host builds still return
`Err(())` so no host test silently "succeeds" without moving bytes. **Consequence
to be aware of:** TAP selftests run on kernel pointers, so every user-copy path
still returns `EFAULT` there; AF_UNIX send/recv and devfs argument copies are now
wired but only a real userspace program exercises the success path.
`socketpair` creation installs real fds (FD-4).

## Syscall ABI — argument order

| Defect | Site | Status |
|---|---|---|
| `symlinkat` passed `(a2,a1,a3)` into `(target,dirfd,linkpath)` — inverted, syscall inoperable | `syscalls/dispatch.rs:852` | FIXED (verified by inspection) |
| `inotify_rm_watch` bound `wd` from `a3` instead of `a2` | `syscalls/dispatch.rs:1059` | FIXED (verified by inspection) |

Both are fixed but **not TAP-covered**: the selftest environment maps no
userspace pages and `AddressSpace` exposes no mapping entry point, so a
handler that reads a string via `read_user_string` cannot be driven to the
point where a swapped register is observable. Asserting a discriminator there
would pass regardless of the dispatch row. See the module doc in
`kernel/src/tests/abi_args.rs`.

## Process fd table — mutations applied to a clone (2026-10-21)

Every handler below took `process.files.lock().fd_table.clone()`, mutated the
copy and dropped it, so the real table never changed. All fd-table mutations now
go through `Process::with_fd_table` (`kernel/src/task/process.rs`), which mutates
the live table and its flags under one lock.

| # | Defect | Evidence | Status |
|---|---|---|---|
| FD-1 | `socket()` (AF_UNIX and AF_INET) and `accept()` installed the new fd into a clone; every returned socket fd was phantom — `setsockopt`, `bind`, `close` resolved it as `EBADF`. | `kernel/src/syscalls/net_socket.rs` (`sys_socket`, `sys_accept`) | FIXED — AF_UNIX accept installs live via `with_fd_table`, pinned by `contract:unix_accept_installs_live_fd` (accept + accept4 flags on the live slot) and `userptr:setsockopt_rejects_unimplemented_options` |
| FD-2 | `close()` cleared, and `dup()`/`dup2()`/`fcntl(F_DUPFD)` wrote fds into a clone: `close` left the fd live, `dup*` returned fd numbers absent from the real table. `fcntl(F_SETFD/F_SETFL)` lost flag writes the same way; `F_DUPFD` also skipped a free fd equal to `arg`. | `kernel/src/syscalls/fs_open.rs` (`sys_close`, `sys_dup`, `sys_dup2`, `sys_fcntl`) | FIXED — pinned by `contract:dup_fcntl_epoll_write_through` (dup → close → `dup` `EBADF`, `F_DUPFD`, `dup2`, `FD_CLOEXEC`/`O_NONBLOCK` read-back) |
| FD-3 | `epoll_create1()` and `memfd_create()` registered fds in a clone; `MFD_CLOEXEC` was dropped too. | `kernel/src/syscalls/epoll.rs`, `kernel/src/syscalls/shm.rs` (`sys_memfd_create`) | FIXED — epoll install pinned by `contract:dup_fcntl_epoll_write_through`; `memfd_create` now installs via `with_fd_table` (shm.rs:617-644) and its pointer validation is pinned by `contract:memfd_create_rejects_non_user_name`. The install/CLOEXEC success path is now reachable through `contract_tests::with_user_page` but has no TAP test yet |
| FD-14 | `mqueue` fd install untested | `kernel/src/ipc/mqueue/mod.rs` | FIXED - 13 contract tests in `kernel/src/tests/mqueue_contract.rs` map one userspace page and pin `mq_open`/`mq_unlink`/`mq_timedsend`/`mq_timedreceive`/`mq_getsetattr` plus `close`/`dup`/`fcntl`. A queue descriptor is a real fd (`FileDescriptor::Mqueue`), names use the libc ABI form (no leading slash; empty/`.`/`..`/slash → `EACCES`, >255 → `ENAMETOOLONG`), `O_NONBLOCK` is per open file description, `mq_unlink` only drops the name, and `mq_open` descriptors are always close-on-exec as on Linux. Known gaps: `mq_notify` (244) is `ENOSYS`; a non-NULL timeout in `mq_timedsend`/`mq_timedreceive` is `ENOSYS`; `read(2)` on a queue fd is `EINVAL` (Linux returns the `QSIZE:` line); no per-uid `RLIMIT_MSGQUEUE` accounting; the queue namespace is global rather than per IPC namespace |
| FD-15 | `recvmsg`/`sendmsg` offset handling untested | `kernel/src/syscalls/net_options.rs` | FIXED — `contract:sendmsg_recvmsg_split_into_iovecs` drives a real `AF_UNIX` pair through mapped user memory and caught `sys_recvmsg` writing `msg_flags` at `msg+24` (the `msg_iovlen` slot); the offset is now `offset_of!(msghdr, msg_flags)` |
| FD-16 | Unbounded allocations in fd-table growth paths | `kernel/src/task/process.rs` | OPEN — stress test needed |
| FD-4 | `sys_socketpair` delegated to `crates/net/src/unix.rs`, which returned two raw `UNIX_SOCKETS` handles as fd numbers; no `FileDescriptor::UnixSocket` was installed, so both fds were unusable and the handles leaked. | `kernel/src/syscalls/net_socket.rs` (`sys_socketpair`), `crates/net/src/unix.rs` (`create_unix_socketpair`) | FIXED — kernel installs both fds via `with_fd_table` and rolls back fds plus handles on `EFAULT`; pinned by `userptr:socketpair_rolls_back_on_bad_user_buffer`; the positive path (fd install + send/recv through both ends) is pinned by `contract:sendmsg_recvmsg_split_into_iovecs`. |
| FD-5 | `clone` arg order was wrong; `child_tid` via parent AS; no cleanup on failure | `kernel/src/syscalls/process_lifecycle.rs` (`sys_clone`), `kernel/src/syscalls/dispatch.rs` | FIXED — arg order fixed; child_tid via child AS; cleanup on failure (contract_tests.rs) |
| FD-6 | `sendto`/`recvfrom` 6th arg from regs; flags handling wrong | `kernel/src/syscalls/net_socket.rs` (`sys_sendto`, `sys_recvfrom`), `kernel/src/syscalls/dispatch.rs`, `kernel/src/syscalls/net_helpers.rs`, `crates/net/src/unix.rs` | FIXED — 6th arg from regs; flags handling corrected (contract_tests.rs) |
| FD-7 | `accept4` missing native dispatch; flag validation absent | `kernel/src/syscalls/net_socket.rs` (`sys_accept4`), `kernel/src/syscalls/dispatch.rs` | FIXED — native dispatch with flag validation (contract_tests.rs) |
| FD-8 | seccomp: sock_fprog offset 8; check_syscall Option<u64>; TRAP/TRACE/KILL/ERRNO/LOG; original Linux nr filter | `kernel/src/syscalls/emulation.rs`, `kernel/src/syscalls/seccomp.rs`, `kernel/src/syscalls/dispatch.rs` | FIXED — sock_fprog offset 8; check_syscall Option<u64>; action codes; original nr filter in dispatch_linux_syscall (selftests) |
| FD-9 | futex: atomic stac/clac CAS + user-range validation | `kernel/src/syscalls/futex.rs` | FIXED — atomic stac/clac CAS with user-range validation (selftests) |
| FD-10 | ptrace: target-AS translation; tracer_pid auth on ALL requests; stopped-tracee ceiling | `kernel/src/syscalls/ptrace.rs` | FIXED — target-AS translation; tracer_pid auth on all requests; stopped-tracee ceiling documented (selftests) |
| FD-11 | wait4: reap by child_pid; ECHILD; copy_to_user | `kernel/src/syscalls/process_lifecycle.rs` (`sys_wait4`) | FIXED — reap by child_pid; ECHILD; copy_to_user (selftests) |
| FD-12 | sigprocmask: validate how+set before copy; no partial state | `kernel/src/syscalls/process_signal.rs` (`sys_sigprocmask`) | FIXED — validate how+set before copy; no partial state (selftests) |
| FD-13 | boot marker: true end-of-init in main.rs (lines 466, 583); removed early marker | `kernel/src/main.rs` | FIXED — true end-of-init marker at lines 466, 583; early marker removed |

---

## NVMe driver

Verified by reading `crates/drivers/src/storage/nvme.rs`. **No NVMe hardware path
is exercised by the QEMU gate, so neither has a test.**

| # | Defect | Evidence | Severity |
|---|---|---|---|
| N-1 | Admin and I/O queue pairs share one `sq_tail`, one `cq_head`, one `phase`. `admin_cmd` (`:399`) and `io_cmd` (`:429`) pass the same three counters into two independent rings. After N admin commands the I/O submit lands at index N, so the controller executes unwritten entries and the completion is read from the wrong slot. | `:155-157` (single set of fields), `:399-403`, `:429-433`, `:469-473` | High |
| N-2 | `reap` (`:345`) reads only the CQE phase bit and returns `true`, discarding `cqe.status` (SCT/SC) and never matching the CID. A failed command is reported as success; `read_sector` then copies the buffer as valid sector data, disclosing stale heap when `DmaBuf` is uninitialised. | `:350-358`, `read_sector` copy site | High |
| N-3 | `DmaBuf::new` uses `alloc()`, not `alloc_zeroed()`, so unwritten SQ entries are uninitialised heap rather than zeros — this is what turns N-1 into executing garbage opcodes. | `crates/hal/src/dma.rs:24` | Medium |
| N-4 | `db_offset = 0x1000 + (2*qid)*db_stride` with `db_stride = 4 << CAP.DSTRD` is never bounds-checked against the BAR length. A controller reporting `DSTRD = 15` yields a doorbell write far past a typically 16 KiB BAR. | `:177`, `:313-329`; no BAR length validation in `pci/mod.rs` | Medium |

**Suggested fix order:** N-1 at the `reap` choke point (give each queue pair its
own head/phase state), then N-2 in the same function (it already reads the CQE
and discards everything but the phase). N-4 wants a BAR-length check at
`map_bar_mmio`.

## DMA / IOMMU

| # | Defect | Evidence | Severity |
|---|---|---|---|
| D-1 | `PooledDma::alloc(min_size, _bdf)` ignores `bdf` and never calls `iommu_map`. The drivers crate imports this type, so NVMe ring/identify/data buffers and xHCI control/interrupt buffers are unmapped while `iommu` is on by default in `default` features — on IOMMU-capable hardware every such DMA faults. The kernel's own `kernel/src/hal/dma.rs:61,79` does map, so this is a divergent second copy. | `crates/hal/src/dma.rs:82-94`; `grep -c iommu nvme.rs` = 0 | High |
| D-2 | xHCI frees the transfer DMA buffer on every timeout while the controller may still be writing into it (`pending_dma.pop()` on the `None` branch). `usb_hid_poller` submits an interrupt-IN per tick, and such a transfer only completes when the device has data, so an idle tick hits this path routinely. | `crates/drivers/src/usb/xhci/mod.rs:817-836` | High |
| D-3 | `crates/drivers/src/lib.rs:146` holds an `iommu_map` stub that is a silent no-op with a different signature (`flags: u32` vs the real `u64`). Currently uncalled, but it shadows the real choke point and has the same shape as D-1. | `crates/drivers/src/lib.rs:146` | Low |

## Storage (AHCI)

| # | Defect | Evidence | Severity |
|---|---|---|---|
| A-1 | The completion wait loop has no iteration bound, so a device that never completes hangs the CPU with interrupts disabled inside a block-device `IrqSafeMutex` — no tick, no watchdog. The preceding `tfd` wait is bounded, so the omission is local. | `ahci.rs:432-444`, `:677-688` | High |
| A-2 | Only command-table header 0 gets its `CTBA` programmed. Free-slot selection can pick header 1-31, which are zero-filled, so the HBA takes its command table from physical address 0. Reachable whenever slot 0 is left busy (e.g. after the `spin > 1000000` bail-out). | `ahci.rs:271-277` vs `:321-329`, `:349` | High |
| A-3 | Bounce buffer leaked on every post-allocation error return; only two paths deallocate. | `ahci.rs:383-389` vs `:421-443` | Medium |
| A-4 | `buf.len()` is never checked against `total_len = sectors * 512`; the non-crossing path hands the HBA the caller's pointer with `dbc = total_len - 1`. All in-tree callers happen to pass 512 bytes. | `ahci.rs:381-396` | Medium |

## Network (E1000)

| # | Defect | Evidence | Severity |
|---|---|---|---|
| E-1 | The IRQ path calls `net::poll()` and then takes four blocking `IrqSafeMutex` locks plus heap allocation (`vec![0u8; pkt_len]`) with interrupts disabled — a direct violation of kernel rule #1. | `interrupts/irq.rs:141-161` → `crates/net/src/lib.rs:282-300` → `e1000.rs:508,724` | High |
| E-2 | The only ICR acknowledge code has zero callers; the live handler reads ICR but never writes it (ICR is write-1-to-clear). Latent while MSI is used, live on the `pci_route_legacy_irq` fallback. | `e1000.rs:613-641` (no callers), `irq.rs:147` | Medium |
| E-3 | `reset_nic` rewrites the RX ring base and re-enables RCTL but never re-arms RDT, so after a chip reset no descriptors are available and one TX timeout permanently kills RX. | `e1000.rs:435-468` vs `:327` | Medium |
| E-4 | Descriptor rings are allocated with alignment 1 (`Box::new([..])` of a packed type) while RDBAL/TDBAL require 16-byte alignment. | `e1000.rs:288,342` | Medium |

## xHCI

| # | Defect | Evidence | Severity |
|---|---|---|---|
| X-1 | `rt_regs()` casts a 32 KiB struct at `rt_offset` taken from the capability register with no bounds check, and `op_regs()`/`rt_regs()` both hand out `&mut` derived from `&self` (`clippy::mut_from_ref`), so multiple live `&mut` can alias one MMIO region. | `xhci/mod.rs:105-114`, `regs.rs:40` | High |
| X-2 | IMAN bit 1 (IP) is written where bit 0 (IE) is meant, so the interrupter is never enabled and the ack is a no-op. Latent only because no xHCI IRQ handler is registered — the controller runs fully polled. | `xhci/mod.rs:260,705-706` | Medium |
| X-3 | `control_transfer` returns success when `PooledDma::alloc` fails: the TD is submitted with no data stage and `cc == 1` is reported as success with the caller's buffer untouched. This is what feeds an uninitialised `dev_desc` into the printed VID/PID. | `xhci/mod.rs:786-802,838-846` | Medium |

## Filesystems

Verified by reading; not exercised by the gate beyond tmpfs/ext2 unit tests.

| # | Defect | Evidence | Severity |
|---|---|---|---|
| F-1 | ext2 directory traversal advances `off += e.rec_len` with no bounds check, so a corrupt or hostile `rec_len` walks the offset past the block and subsequent entries are read out of bounds. | `crates/vfs/src/ext2/directory.rs:45,94` | High |
| F-2 | SkyFS `store_data_blocks` multiplies a disk-supplied `extent_count` by `size_of::<Extent>()` unchecked before comparing against `MAX_INLINE_DATA`, then copies that many bytes into a fixed-size array. | `crates/vfs/src/skyfs/mod.rs:531-535` | High |

## SMP scaling — found by stress testing (2026-10-03)

PLAN.md F1.4 listed "4 and 8 CPUs" as never tested. Exercising them for the
first time found two distinct failures. **Both reproduce on the pristine
pre-change image (`bootimage-vahi_kernel.bin`, dated 2026-09-21), so neither is
a regression from the 2026-10-03 work.**

| Topology | Result | Where it stops |
|---|---|---|
| SMP 1 | 3/3 PASS | — |
| SMP 2 | 3/3 PASS, but 1 FAIL observed in a later 4-run batch (160/161); 8/8 PASS on immediate re-run | test name not captured before the batch ended |
| SMP 4 | 2/3 PASS (1 FAIL) | reaches `161/161 passed`, then `[LOCKUP] rip=0xffffffff800374d8 stuck 500 ticks` |
| SMP 8 | 0/2 — TIMEOUT at 540s and 888s | last serial line is `[BOOT] syscalls init...` — hangs before any test runs |

| # | Defect | Evidence | Severity |
|---|---|---|---|
| S-1 | **SMP 8 hangs in early boot.** The last output is `[BOOT] syscalls init...`; nothing after it, on two consecutive runs. Reproduces identically on the pristine pre-change image, which stops slightly earlier (KASLR activation). | serial capture, SMP 8 | High |
| S-2 | **SMP 4 passes every test then locks up.** `# 161/161 passed, 0 failed` is printed, then `[LOCKUP] rip=0xffffffff800374d8 stuck 500 ticks`. The kernel completes its work and then wedges — a shutdown/idle-path hang, not a test failure. | serial capture, SMP 4 | High |

SMP 2 additionally showed one intermittent `160/161` failure that did not
reproduce across 8 subsequent runs, so its frequency is roughly 1-in-10 and the
failing test name was not captured before that batch ended. It is recorded as
unreproduced rather than dismissed: SMP 2 is the one topology CI and the docs
treat as the gate, so a 10% flake rate there is worth pinning.

Neither S-1 nor S-2 has a TAP test, because both occur outside the selftest
harness: S-1 before it starts, S-2 after it finishes. They need boot/shutdown-path
instrumentation to pin, and the `[LOCKUP]` watchdog output already identifies a
RIP (`0xffffffff800374d8`) to symbolise.

## Test harness

| # | Defect | Evidence | Status |
|---|---|---|---|
| H-1 | `run_qemu_tests.py` defaulted every run to ONE serial capture path and deleted it on start, so concurrent runs destroyed each other's capture. This produced a spurious `BOOT_FAILURE` (boot 59 of 100) during a parallel reliability run. | `tests/run_qemu_tests.py` serial-path default | FIXED — unique path per PID; `--serial` still overrides |
| H-2 | The stale-capture removal swallowed `OSError`, so a leftover file could be parsed as the current run's verdict. | same | FIXED — absent is the normal case, a genuine removal failure is now `INFRASTRUCTURE_ERROR` |

## Test suite — false confidence

No `issues.md` row is a substitute for these; they are structural.

- `kernel/src/tests/stress.rs`: 8 of 13 tests never call the subsystem they name
  — they exercise a local `Vec`/`VecDeque`, and `inode_churn` defines a
  `FakeInode` inside the test. `alloc_dealloc_churn` and `fragmented_alloc`
  have no assertion at all.
- `kernel/src/tests/fuzzer.rs`: the fuzzer never invokes a syscall (stated in
  its own comment at `:361-366`); all five `fuzzer::*` tests validate the
  generator's own PRNG and counter bookkeeping.
- `kernel/src/tests/benchmarks.rs`: `BenchStats::report()` only prints; no bound
  is asserted, so a large regression stays green. `memory_leak_audit` is the one
  benchmark-derived test that returns `Err`.
- `tests/boot_stress_100.ps1`: declares `$PassToken` and never reads it, so a
  boot that dies silently without one of its four fail tokens counts as a pass.
  It also boots the production image, which has no `self_test` feature, so the
  154 tests never run in it.
- `tests/extended_fuzz.ps1`: same problem — boots the production image, so the
  fuzzer is not compiled in. Zero crash-detection capability as written.
- Network packet parsing has **no** registered test in any direction, which
  AGENTS.md test-first #4 explicitly requires.

## Unnecessary complexity vs production kernels (audit 2026-10-03)

Measured, not speculative. Line counts and caller counts verified against the
tree. OpenCode (1.18.34) was run for a second opinion; its crate-boundary
reasoning held up, its "remove one buddy allocator" suggestion did NOT — both
are heavily used (see the refutation below).

### Dead code

**209 of 1,213 `pub fn`s in kernel/src + crates have zero callers (17%).**
Counted mechanically: a function is dead if its name appears nowhere outside its
own definition line. Distribution:

| Location | Dead `pub fn`s |
|---|---|
| `kernel/src` root modules | 93 |
| `kernel/src/objects` | 20 |
| `kernel/src/syscalls` | 18 |
| `kernel/src/hypervisor` | 16 |
| `kernel/src/task` | 13 |
| `kernel/src/compositor` | 10 |
| `kernel/src/ash` | 8 |
| `kernel/src/ebpf` | 7 |
| `kernel/src/apic` | 6 |
| elsewhere | 34 |

Notable clusters:
- `crates/net/src/congestion.rs` (81 lines) — **a complete duplicate of
  `tcp_congestion.rs` (221 lines) with zero callers.** The live module has 4
  call sites (`net_helpers.rs:203,233,532,546`). `congestion.rs`'s `on_ack`,
  `on_triple_dup_ack`, `on_timeout` are never called.
- `kernel/src/compositor/compositor.rs` — 10 dead fns including `composite_frame`
  and `composite_window`, the two functions the file exists to provide.
- `kernel/src/hypervisor/ept.rs` + `sched.rs` — 16 dead fns including
  `handle_ept_violation`, `schedule_vcpus`, `unmap_guest`.
- `crates/drivers/src/block/scheduler.rs` — `scheduler_init` and
  `scheduler_submit` both dead: the whole block-scheduler is unwired.
- `crates/vfs/src/skyfs/journal.rs` — `journal_begin` and `is_full` dead, so the
  journaling path ADR-018 describes is only partly live.

### Duplicated implementations (verified)

| # | Duplication | Status |
|---|---|---|
| D-1 | `crates/net/src/congestion.rs` vs `crates/net/src/tcp_congestion.rs` | **CONFIRMED** — first is 81 lines, zero callers |
| D-2 | `crates/syscalls/src/user_access.rs` (host-only fail-closed stubs; kernel target delegates to exports) vs `kernel/src/syscalls/user_access.rs` (real SMAP impl) | RESOLVED — see U-1/U-2; the crate side is now only an ABI bridge, not a second implementation |

**Refuted:** `crates/memory/src/buddy.rs` holds both `BuddyAllocator` and
`BuddyFrameAllocator`, which looks like duplication. It is not: the former is the
core allocator (52 external refs to `BUDDY_ALLOCATOR`), the latter a thin
`FrameAllocator` adapter over it (70 refs). Both are live. Do not merge.

### Subsystems compiled but barely reachable

| Subsystem | Lines | Built by default? | Runtime consumer |
|---|---|---|---|
| `hypervisor/` | 2843 | **No** — `feature="hypervisor"`, off in `default` | 16 dead fns |
| `ash/` | 825 | **No** — `feature="ash"`, off in `default` | 8 dead fns; 4 syscall handlers unreachable |
| `ebpf/` | 2228 | **Yes** | 7 dead fns; reachable via `SYS_BPF` (`dispatch.rs:979`) |
| `compositor/` | 743 | **Yes** | 10 dead fns incl. its two core entry points |

`ash` and `hypervisor` are correctly feature-gated off, so they cost nothing at
runtime — but `crates/verified` (1215 lines) exists only to serve the
`verification` feature, also off by default.

### Silently-accepted socket options (worse than dead code)

`kernel/src/syscalls/net_options.rs:36-120`: ~15 socket options return success
while doing nothing. `SO_LINGER`, `SO_SNDBUF`, `SO_RCVBUF`, `SO_ERROR`,
`SO_TYPE`, `SO_BINDTODEVICE` are literal `{ /* comment */ 0u64 }` bodies — no
code at all. A program that sets `SO_LINGER` before `close()` expects a blocking
close and gets none; `SO_RCVBUF`/`SO_SNDBUF` sizing is ignored entirely.
`docs/socket-api.md:53-57` documents 3 supported options while ~15 are accepted.
**FIXED 2026-10-21**: unimplemented options now return `ENOPROTOOPT`;
`SO_REUSEPORT` is the only settable option and `docs/socket-api.md` matches.
Pinned by `userptr:setsockopt_rejects_unimplemented_options`.

### Crate structure vs ADR-001 (monolith)

19 crates, 30,721 lines, and `kernel/Cargo.toml` depends on 17 of them. ADR-001
deliberately chose a monolith; the extraction reintroduced the indirection it
rejected. `types` (1046 lines) is genuinely justified as a shared-types crate.
The small single-consumer crates (`gdt` 77, `limine` 141, `acpi` 234, `pci` 234)
are the cheapest to fold. The large ones (`drivers` 9465, `vfs` 7918) are
high-cohesion and folding them is high-risk for little gain.

## APIC: crate/kernel duplication — migration blocked, real bug found

`vahi-apic` (1071 lines) and `kernel/src/apic` (1102 lines) define the **same 26
public functions** (`mode`, `is_x2apic`, `send_ipi`, `eoi`, `set_tpr`, `tpr`,
`route_pci_irq`, `wait_for_ipi`, the whole `msi` and `errata` surface, ...). The
bodies of `send_ipi`/`send_broadcast_ipi` are byte-identical.

**Current call graph:** the kernel copy is the live one — 41 call sites use
`crate::apic::`. The crate has 4, and all 4 are the MSI problem below.

### A-3: MSI is silently unavailable to `vahi-pci` and NVMe (REAL BUG)

`crates/pci/src/lib.rs:242` and `crates/drivers/src/storage/nvme.rs:705` both
call `vahi_apic::msi::alloc()`, which reads the **crate's** pool
(`crates/apic/src/msi.rs:120`, `static POOL`). Nothing ever calls
`vahi_apic::init()`, so that pool is permanently `None`, and `alloc()` is
`POOL.lock().as_mut().and_then(|p| p.alloc())` — it returns `None` forever.
`crates/pci` propagates the `None`; NVMe's `if let Some(...)` just skips MSI.

Net effect: both devices silently fall back to legacy INTx interrupts. No crash,
no log line — MSI simply never works for them. The kernel's own
`kernel/src/pci/mod.rs:54,72` uses the *other* pool and works fine, which is why
this went unnoticed.

Fixing it requires calling `vahi_apic::init(...)`, which is blocked by A-4.

### A-4: why the swap is NOT a safe deletion

The crate is not a drop-in replacement for the kernel module:

1. **Different state model.** `kernel/src/apic/mod.rs:56` owns
   `static MODE: spin::Once<ApicMode>` and reads IOAPIC addresses from
   `crate::acpi::IOAPIC_ADDRS`. The crate owns its own `MODE` and reads
   `acpi.ioapic_addrs()` from an `AcpiProvider` trait object
   (`crates/apic/src/lib.rs:245,299`). Making the crate primary means the kernel
   must implement `AcpiProvider`, `MemoryProvider` and `SerialWriter` over its
   existing globals — new adapter code, not deletion.
2. **Not a superset.** The crate is missing `ioapic_clear_stuck_irr`
   (the kernel's only unique APIC function).
3. **Risk profile.** This rewrites interrupt-controller init on a kernel that
   already has two unreproduced SMP hangs (S-1, S-2). A regression here would be
   very hard to attribute.

Recommended order: land A-3 first as a standalone fix (wire the crate's ACPI
provider and call `vahi_apic::init`, leaving the kernel module in place), verify
MSI works for `vahi-pci`/NVMe, and only then consider retiring the kernel copy.

## Benchmark baseline (2026-10-03, QEMU TCG, 1 CPU, 512 MiB)

Measured on the current tree; two independent boots agreed within ~25% on p50.
These are the reference numbers for any future regression threshold. The audit
found `benchmarks.rs` asserts nothing, so nothing currently fails on a
regression — these are recorded so a threshold can be set deliberately.

| Benchmark | min | p50 | p99 | max | iters |
|---|---|---|---|---|---|
| `fork_exec_clone` (CoW clone + process create) | 183–202 µs | 212–264 µs | 383–394 µs | 395–440 µs | 100 |
| `pipe_throughput_4k` | — | 826 µs / 4096 B | — | — | 1000 |
| `ctxswitch_yield_poll` | 116–158 ns | 158–183 ns | 241–250 ns | 27–49 µs | 10000 |
| `mmap_munmap_vma` | 94–100 µs | 132–144 µs | 215–242 µs | 270–360 µs | 500 |
| `syscall_getpid` | 200–258 ns | 283–350 ns | 391–416 ns | 35–56 µs | 10000 |
| `page_alloc_free` | 17–18 µs | 24–35 µs | 52–137 µs | 0.3–2.2 ms | 10000 |

`benchmarks::memory_leak_audit` is the one benchmark-shaped test that asserts:
slab active delta 0 bytes, PASS on both runs.

Note the `max` outliers (ctxswitch 27–49 µs against a 158 ns p50, page_alloc
2.2 ms against 24 µs p50) — consistent with scheduler preemption landing inside
the measured window, and worth knowing before anyone sets a `max` threshold.

## Arithmetic-safety profile — what it actually found: KASLR relocates nothing

Added `[profile.audit]` (plus `audit_ovf`, `audit_dbg`, `audit_small`) to the
workspace root, since `release` leaves `overflow-checks` and `debug-assertions`
off.

**Symptom:** an unoptimised 79.7 MB kernel dies at KASLR mapping activation;
the 2.6 MB release build boots to 161/161. Serial stops after `ABCD` (the four
trampoline markers at `kaslr_reloc.rs:26/41/51/65`), so CR3 switch and RSP
restore complete and the final `jmp rax` to the relocated entry faults.

**Controlled:** the slide is NOT the variable. audit failed 8/8 across shifts
85-423; audit_small passed 8/8 across shifts 33-485 — overlapping ranges.

**Ruled out with evidence:** the hard-coded 64 MiB constant (loadable PT_LOAD
spans are only 5.02 MB and 1.73 MB — nothing approaches it); ELF geometry;
section-header placement (outside the PT_LOAD span on *every* profile,
including working ones).

### A-5 (SUPERSEDED): "kaslr_reloc.rs:371 hardcodes 64 MiB" — WRONG

The constant is genuinely wrong and is now derived from the image, but it was
**not** the boot failure. Recorded here because the wrong attribution was
reported and acted on before being corrected.

### A-6: `apply_kaslr_relocations` silently relocates nothing (REAL, HIGH)

Boot diagnostic, reading the ELF header the function itself reads:

```
[DIAG] magic=........ phnum=4808 shoff=ccccccc328c48348
```

The magic at `LINK_BASE` is not `ELF`, so `kaslr_reloc.rs:338` returns
immediately — **on every build**: audit, audit_small, and release. Zero
relocations have ever been applied. `main.rs` prints "Relocations applied"
unconditionally, so the failure is invisible.

This is why nothing ever diagnosed this: it violates the repo's own
"errors are handled, never swallowed" rule.

### A-7: KASLR preserves only ONE page of the old mapping (REAL, HIGH)

The rebuild maps every kernel page at its new shifted index only
(`kaslr_reloc.rs:543-549`). The old index is preserved for the trampoline page
alone (`:589`). Diagnostic confirms `tramp_pd=0` — a single 2 MiB page.

Because A-6 means relocations never run, absolute pointers into the old image
survive the CR3 switch. So:

| | PT_LOAD span | pages in `new_pd` | old-index coverage | result |
|---|---|---|---|---|
| audit_small | 1.73 MB | 1 | page 0 = whole image | boots |
| audit | 5.02 MB | 3 | page 0 only | faults |

**Kernels under 2 MiB are safe by accident; anything larger is unbootable.**
No size check exists that would catch this.

Also noted: the comment at `:342` claiming the kernel sits at PD index 511 is
wrong — `(LINK_BASE >> 21) & 0x1FF` is **0**. The shift arithmetic only works by
accident of the modulo.

### Tests added (RED-first, 7 new, 161 -> 168/168, 0 failures)

`kernel/src/tests/kaslr_mapping.rs` (3): map pages from image size not a
constant; shift that wraps the PD is reported; span derived from segments.
`kernel/src/tests/kaslr_relocation.rs` (4): zero relocations reports failure;
non-zero reports success; old-address coverage spans the whole kernel; kernel
over one page is flagged.

Two of these failed on first run for **wrong arithmetic in the test**, not
production code (`pd_shift_aliases(100,32)` is 643 > 512, so it does alias;
`from_counts(4096,4096)` sets failed=4096). Corrected, then green.

## Cross-arch portability (aarch64 / riscv64) — CI now builds these (2026-10-21)

The nightly `cross-arch` job runs `cargo build --target aarch64-unknown-none
--features smp` and `cargo check --target riscv64gc-unknown-none-elf` in
`kernel/` (ADR-028: riscv64 was never built in CI, which is how this stayed
hidden). Both are RED at the time of writing, reproduced locally with the
exact CI commands:

| # | Defect | Evidence | Status |
|---|---|---|---|
| P-1 | `vahi-sync` compiles x86-only intrinsics and asm unconditionally: `core::arch::x86_64::__cpuid` and `asm!("pushfq; pop {rflags}; cli", options(att_syntax))` in the IRQ-save path | `crates/sync/src/lib.rs:205,255`; aarch64 build: `E0433: cannot find x86_64 in arch` + `error: the att_syntax option is only supported on x86`; riscv64 check: same two errors | OPEN |

The build stops at the first crate, so breakage beyond `vahi-sync` is unknown
until P-1 is fixed. The job is intentionally left failing: a red nightly is a
finding, not noise.

## Dependency vulnerabilities (cargo audit, nightly)

The nightly `dependency-audit` job fails on any vulnerability (no fallback).
Verified locally 2026-10-21 with `cargo audit` (0.22.2):

| # | Defect | Evidence | Status |
|---|---|---|---|
| V-1 | `xmas-elf 0.9.1` (the kernel's ELF loader, untrusted input path) — RUSTSEC-2025-0018: potential out-of-bounds read with a malformed ELF file and the HashTable API; fixed in >= 0.10 | Upgrade to `xmas-elf 0.10.0`: `cargo audit` exit 0 (0 vulnerabilities); kernel rebuilt; QEMU T-00 207/207 at `-smp 1` and `-smp 2` | FIXED 2026-10-21 — no code changes needed (API used here is unchanged) |

`cargo audit` additionally reports 3 non-failing warnings (unmaintained
`atomic-polyfill 1.0.3`, unmaintained `proc-macro-error2 2.0.1`, yanked
`spin 0.9.8`) — informational; warnings do not fail the gate.

## Documentation

- ADR-013 and ADR-014 are missing from `docs/decisions/` while
  `docs/architecture/strategic-roadmap-to-linux-parity.md` references them.
- `kernel/src/tests/mod.rs` gates `sync_tests::register()` on `target_arch` but
  `harness_tests::register()` on the `self_test` feature, so on aarch64 the sync
  suite silently disappears and the TAP plan shrinks while still reporting PASS.
  No aarch64 job runs the gate.

### A-8: PD-index arithmetic used the wrong base (FIXED)

`LINK_BASE = 0xFFFFFFFF80000000` decomposes to PML4[511] -> PDPT[510] -> PD[0],
so the kernel image occupies page-directory index **0**. The rebuild loop used
`(511 + i) % 512`, which at `i=0` read `PD[511]` -- an unrelated mapping -- and
never copied the kernel's own pages. Every absolute pointer into the old image
faulted after the CR3 switch.

Fixed via `kernel_base_pd_index()` / `pd_destination_index()`. `pd_shift_aliases`
now delegates to the same predicate so the two cannot disagree about the
512-entry boundary. Four assertions in `tests/kaslr_mapping.rs` encoded the old
511 base; they were corrected (not deleted) with the reason recorded, since the
wrong values had been pinned as correct.

### A-9: 64 KiB stack temporary in coverage init (FIXED)

`Box::new([0u8; COVERAGE_BITMAP_SIZE])` materialises the whole array as a stack
temporary before copying to the heap. The boot stack is far smaller, so the
kernel wedged before the first self-test with no fault and no diagnostic. Now
zeroes a heap slice instead.

### A-10: 1200-byte VState stack temporaries in the eBPF verifier (FIXED)

`VState` (11 `RegState` + 64 `Tnum`) was cloned onto the stack throughout
`tnum_verify` -- `pre`, `post` and `merged` are live simultaneously. A
two-instruction program hung the kernel. This was initially misdiagnosed as a
non-terminating worklist; `[TV] pop 0` with no second pop proved the hang was
*inside* instruction 0, below the loop. Payload is now boxed, so this function's
stack depth is independent of the boot stack limit.

A worklist iteration bound (`MAX_TNUM_STEPS_PER_INSN`) was added at the same
time and is retained as defence in depth, but it was **not** the cause.

### A-11: `.got` fell outside the derived kernel span (FIXED)

`__kernel_end` was derived from `.bss`, but `.got` is the last section in the
linker script and sat 2184 bytes beyond it. Now `ADDR(.got) + SIZEOF(.got)`.

Note: the live linker script is the **repository-root** `linker.ld`, not
`kernel/linker.ld`. `-C link-arg=-Tlinker.ld` resolves relative to the workspace
root, so the `kernel/` copy is dead. Its CRLF line endings also break `rust-lld`
("unknown directive"), because the file is a linker script.

### A-6 (STILL OPEN): relocations are never applied -- KASLR is obfuscation only

`apply_kaslr_relocations` returns at its magic gate on every build. Verified by
direct analysis of the linked artifact:

- The audit/debug builds contain **zero** `.rela.*` sections. `--emit-relocs` is
  passed by `kernel/Cargo.toml` and `kernel/.cargo/config.toml`, but neither is in
  effect for a workspace-root build, so the root `.cargo/config.toml` governs.
- The ELF header is not readable at `LINK_BASE` at runtime: the first `PT_LOAD`
  has `p_offset = 0x1000`, so file bytes `[0, 0x1000)` are never mapped and
  `LINK_BASE` holds `.text` bytes.
- Consequently there is no iteration source for a RELATIVE relocation walk, and
  `__rela_start` / `__rela_end` do not exist.
- 15,863 absolute link-time addresses are baked into `.rodata` (15,856), `.data`
  (1), `.got` (6) and `.bss` (6) with **no relocation record describing them**.
  `relocation-model=static` resolved them at link time.
- `.got` also sat outside `[__kernel_start, __kernel_end)` before A-11.

The kernel boots only because **every** old address remains aliased. KASLR
currently randomises the mapping without correcting a single embedded pointer,
which is obfuscation rather than the ASLR guarantee it appears to provide.

Fixing it properly needs both: `--emit-relocs` on the profiles actually used
(root `.cargo/config.toml`), and an **alloc'd** `.rela` output section with
bounds symbols in `linker.ld` -- today the tables are non-alloc and appear in no
`PT_LOAD`, so they are file data that is never mapped.

Also noted: `main.rs:350`'s `[KASLR-VERIFY] match:` check compares a RIP taken
24 bytes into the function prologue against the function's *entry* address, so it
reports `NO` even on a correct slide. The check is wrong, not the trampoline.

# Invariant Ledger

Invariants that must hold across the Vahi kernel, their enforcement, and
their verification evidence. Produced 2026-09-04 after the re-entrant
lock family (10+ instances) was root-caused and fixed; every invariant
below has at least one instance in the bug history.

## I1 — Mutex non-reentrancy (no same-CPU nested acquisition)

**Statement:** No code path acquires a non-reentrant `IrqSafeMutex` that
the same CPU already holds. A same-CPU `lock()` on a held mutex spins
forever with IF=0 — invisible to ticks, the watchdog, and gdb-free
debugging.

**Boundary:** every `IrqSafeMutex` in `kernel/src` and `crates/`.

**Mutation points:** all `.lock()` / `.try_lock()` call sites; nested
guards from temporaries (`(a.lock().x, a.lock().y)` — statement scope)
or block-scoped guards (`let t = M.lock(); call_that_locks_M();`).

**Enforcement:**
- Runtime: `vahi-sync` `IrqSafeMutex` tracks the holder CPU
  (CPUID leaf 1 APIC ID). `lock()` on a same-CPU hold **panics with the
  mutex address** instead of spinning. `try_lock()` returns `None`
  (probe patterns depend on it). Cleared before spin-release; the
  residual detection window (clear → release) is a few instructions and
  can only weaken detection, never allow a nested acquisition to succeed.
- Static: `scan_locknest2.py` (statement + block scope, one-level call
  graph). Statement pass catches `(a.lock().x, a.lock().y)`; block pass
  catches `let g = M.lock()` alive across a call to a function that
  locks M.

**Verification evidence:**
- Bug history: exec fd-save tuple, page_fault SIGSEGV tuple,
  find_vma-under-mem-guard, kill_from_fault/exit-SIGCHLD/OOM/sys_kill/
  tick_itimers PROCESS_TABLE-across-route_signal (10 sites, both
  lineages; kill_process was the same shape and is now removed as dead
  code — zero callers), plus 12 CURRENT_PROCESS/creds guard-across-helper
  sites found by the scanner (fixed via per-CPU helper resolution +
  site scoping).
- Unit tests: `lock_while_held_same_cpu_panics`,
  `try_lock_while_held_same_cpu_returns_none`, `relock_after_drop_succeeds`
  (10/10 vahi-sync host tests).
- Scanner: 0 hits across 249 files (validated against a re-created
  buggy/fixed pair first). Callee-side try_lock acquisitions are
  excluded — they bail on contention, so they cannot self-deadlock
  (this is what lets the contract tests' deliberate contention holds
  pass cleanly).
- Runtime: single-CPU, SMP-4, and selftest (134/134) boots with the
  detector armed reach `login:` with zero panics.
- Executable contract (kernel/src/tests/contract_tests.rs + panic_path_tests.rs,
  in the selftest suite): exit_code encoding table (i32::MIN sentinel, 139
  fault-kill, raw u64→i32 store), signalfd mask matrix, ssi_* info
  population + FIFO drain, the three-lock contention bail, and the
  allocation-free panic path (IrqFmtBuf formatting/truncation + exact
  register/backtrace line shapes through serial_fmt) — the campaign's
  core claims run as TAP cases (134/134 as of 2026-09-04) instead of
  being asserted only by boot-watch evidence. The 12 overlapping
  process_lifecycle cases were consolidated to 6 distinct-assertion
  tests (shared helper; fd round trip now seeded).

**Residual risk:** the clear→release window weakens detection (not
correctness) for re-entry in that instant; the scanner is a screening
tool (name-collision false positives possible; one-level call graph).

## I2 — Lock ordering (PROCESS_TABLE → per-process → SOCKETS)

**Statement:** Multi-lock acquisitions always acquire in
PROCESS_TABLE → per-process → SOCKETS order; never reversed.

**Boundary:** any code path taking two or more of these locks.

**Enforcement:** AGENTS.md rule; manual review; scanner screens for
re-entrant acquisition which is the common reversal vehicle.

**Evidence:** no reversal found in the 2026-09-04 audit; the
table-across-route_signal fixes preserve table → per-process ordering.

**Residual risk:** no mechanical check; ordering violations that are not
re-entrant (two different mutexes, two CPUs) are undetected by the
scanner and surfaced only by SMP deadlock analysis.

## I3 — Guard lifetime containment

**Statement:** A guard's lifetime never spans a call that acquires the
same mutex (re-entrancy, I1) or violates I2.

**Boundary:** block-scoped `let g = M.lock()` bindings.

**Enforcement:** `scan_locknest2.py` block pass (guard-scope scan against
callee lock sets). Choke-point fixes: helpers that used to re-lock the
global (`has_capability`, `audit_log`, `get_current_euid/egid`) now
resolve the process per-CPU (no global lock) so they are safe under any
guard; `compute_oom_score_from_proc` exists for table-held callers.

**Evidence:** 12 sites flagged by the scanner → fixed → 0 hits; boot
verification silent with the runtime detector armed.

**Residual risk:** scanner scope ends at function end; cross-function
guard passing (returning guards) is not modeled.

## I4 — IRQ-context safety

**Statement:** Code running with IF=0 (fault handlers, timer tick)
never blocks on a lock held by another CPU, never allocates, and never
calls a function that does.

**Boundary:** `interrupts/`, `task/scheduler/tick.rs`, fault kill paths.

**Enforcement:** `try_lock` discipline in tick/fault paths; stack-buffered
`IrqFmtBuf` serial formatting; `#[cfg]`-gated heap-free paths; the
re-entrancy detector converts violations into panics instead of wedges.

**Evidence:** the tick_itimers table-guard fix; breadcrumb-instrumented
fault path (no `format!` in IRQ); post-fix SMP-4 boots show no lock
spins in gdb captures.

**Residual risk (closed 2026-09-04 evening):** `route_signal_to_signalfd`
previously took blocking locks from IF=0 contexts. The single routing
function is now `route_signal_to_signalfd_for(proc, ...)`: it takes an
already-resolved process and uses try_lock only, bailing silently on
contention (the signal was already queued via the raise; signalfd is a
secondary notification). The old PID-keyed blocking wrapper was deleted
on 2026-09-04 — every caller (oom, exit path, sys_kill) already held the
process Arc, so the wrapper only re-resolved it from the table a second
time. IRQ/fault callers: tick_itimers, kill_from_fault (raise is
try_lock + skip; wait4 re-scans the table every tick so reaping does not
depend on it), and the keyboard Ctrl+C path
(now resolves the process via the new non-blocking
try_get_current_process helper instead of locking the global).
Verified with the detector armed: selftest 132/132, SMP-4 boot with a
post-login SIGSEGV kill and a Ctrl+C keystroke — no panics, no spins,
system stable. Mirrored in crates/task.

**Fault path closed (2026-09-04 late):** the SIGSEGV kill path now takes
ZERO blocking locks and allocates nothing:
- `exit_code` is `AtomicI32` (`i32::MIN` = none) — the mandatory
  fault-path write is lock-free; wait4/exit updated (both kernel/src
  and crates/task lineages). `kill_process` was removed (dead code,
  zero callers).
- `kill_from_fault`: table lookup and SIGCHLD raise are try_lock +
  skip-on-contention (wait4 reaps via the table scan every tick, so
  reaping does not depend on the raise).
- The SIGSEGV summary + SIGVM dumps (page_fault.rs and exceptions.rs,
  kernel + crates/interrupts): every lock is try_lock and VMAs are
  formatted under the guard — removing the previous `vmas.clone()`
  heap allocation in IF=0 context. Contended locks skip the dump
  (best-effort diagnostics). exceptions.rs resolves the process via
  try_get_current_process (no global lock).
- The re-entrancy detector remains armed across all of it.

Verified: selftest 132/132; SMP-4 detector-armed boot with a post-login
SIGSEGV kill (wild write) and a Ctrl+C keystroke — zero panics, zero
spins, stable at login; scanner main pass 0/248.

**Remaining landscape (documented, open):** the RESOLVABLE-fault path
(swap-in, cgroup brk-check, COW — lines ~124-238 of page_fault.rs) and
the tick wake path (wake_process_futex/blocked_threads, route_outgoing)
still take brief blocking locks (buddy allocator, SWAP_*, queues).
Same bounded-spin class: holders are running threads that release in
microseconds, no known inversion. `feed_scancode`'s TTY_KEYBOARD lock
is safe-by-ownership (only the keyboard IRQ itself ever holds it). The
scanner's I4 pass (BFS from IF=0 entries) lists these; it is a
name-based screening tool — verify chains before acting on hits.
Conversion of the resolvable-path locks (swap-in with try-lock
fallbacks) is a separate larger change.

## I5 — Process identity is per-CPU

**Statement:** A syscall's process context is the calling thread's
process on the calling CPU, never a global last-switch-wins mirror.

**Boundary:** syscall layer (`get_current_process()` and everything that
resolves "the caller").

**Mutation points:** context switches (writes to the `CURRENT_PROCESS`
mirror), syscall entry (reads), exec (rewrites the mirror).

**Enforcement:** `get_current_process()` derives per-CPU first
(this-CPU sched lock via try_lock), global as fallback; exec fd-table
and setuid reads use the same derivation; `page_fault.rs` and `sys_fork`
already used the per-CPU rule.

**Evidence:** a probe logged 21 global-vs-local mismatches in the SMP
spawn window pre-fix; post-fix SMP-4 spawn SIGSEGVs dropped from 2-3 to
0-1 per boot, and the remaining ones are userspace bugs (login-manager
rip 0x404a6e), not wrong-process VMA corruption.

**2026-09-04 (SMP-4 "stall" debug):** the exit family still read the
raw global. `sys_exit`, `sys_wait4`, `sys_getpid`, `sys_getppid`,
`sys_set_tid_address`, and the pid==0 sched_attr/affinity resolvers all
converted to `get_current_process()`. Symptom: a service thread calling
exit(1) was read as PID 1 -> spurious "PID 1 (init) exited" panic +
SYSTEM HALTED (the panic dump's per-CPU thread PID proved the
misattribution: pre-fix panics showed the panicking thread as PID
107/108 while the message claimed PID 1). Probe evidence: pre-fix 4/5
SMP-4 boots panicked (half misattribution), post-fix misattribution
panics are zero.

**Residual risk:** the global fallback remains for idle/boot and
sched-lock-held callers; if a caller holds the global AND reaches the
fallback, I1 re-entry is possible (none found by the scanner). Remaining
raw-global identity readers not yet converted: sys_lseek,
sys_getdents64 (fs_io.rs), eventfd, timerfd, io_uring, misc,
objects/syscalls — same I5 class, no known corruption trigger, swept
next. Separate open item: fresh SMP processes occasionally crash on
first heap ops (near-null writes at varied sites) and init can then
exit(145) — a code value absent from init's binary (only exit(0/1)
literals), suggesting execution corruption; pre-existing (present in
pre-fix kernels), not regressed by the conversions.

## I6 — VMA uniqueness (atomic mmap)

**Statement:** No two VMAs in a process's address space overlap; mmap's
free-region search and insert are atomic under the memory lock, so two
concurrent anonymous mmaps can never return the same address.

**Boundary:** `sys_mmap` / `sys_munmap` / `sys_mprotect` /
`find_free_vma_region` / `add_vma` / `vma_insert`.

**Enforcement:** `find_free_vma_region` + insert run under ONE memory-lock
guard (`vma_insert`); the old scan-then-re-lock TOCTOU is gone.

**Evidence:** pre-fix SMP boots showed arena reads 8B–16KB past mapped
chunk ends (malloc's next chunk never placed); post-fix boots show no
such faults; SIGVM dumps show per-process VMA lists distinct.

**Residual risk:** no debug assertion re-checks non-overlap on insert
(could add one behind `debug_assert!`).

---

## I7 — Panic diagnostics print without allocation; init death always halts

**Statement:** (a) a kernel panic always prints its message and context
regardless of allocator state — the panic path never allocates; (b)
when PID 1 dies, the machine halts by design ("no init = system
dead"), on every death path.

**Boundary:** `handle_panic` / `#[panic_handler]` and
`Process::kill_from_fault` (the exception-handler kill path).

**Enforcement (2026-09-04):**
- Panic handler is allocation-free: every diagnostic goes through
  `serial_fmt` (stack-buffer formatter, reuses `IrqFmtBuf`), including
  the message, location, CPU/thread, register dumps, boot trace, and
  the stack walker. Previously `alloc::format!` recurred when the panic
  *was* an allocation failure, printing only the banner in a 40k-line
  loop and hiding the message (observed). This also made panicking from
  IF=0 fault context safe.
- `kill_from_fault` halts on PID 1 with the same "PID 1 (init)
  exited... no init process = system dead" contract as the sys_exit
  path. A fault-killed init previously left the machine alive-but-dead
  at 0 cores, never reaching the halt guard (observed: boot where init
  executed an instruction fetch at rip=0 and the system neither
  logged in nor halted).

**Evidence:** post-fix panics print full context (registers, backtrace,
boot trace) — `cow_4.log`, `final2_*.log`. SMP-4 boots reach login
12/12 with zero panics; the only faults are the deterministic
login-manager userspace bugs (0x403920 write to freed 0x500000000000
arena; 0x403e65/0x403d7e reads just past the 0x20000000 arena;
0x404a6e stale-stack read) — handled as kills, no kernel corruption.

**Residual risk:** the COW check-act race (two CPUs breaking/forking
the same frame concurrently) was investigated and serialized under a
trial `COW_LOCK`, and a free-list frame-liveness detector was trialed;
neither produced any observable change across 16+ SMP-4 boots, so both
were **removed as unproven** (a global lock on every COW fault and a
panic in the buddy hot path are not defensible without evidence). The
race remains latent in the code. init exits(145) by design when a
service fails, tripping the PID-1 halt.

---

## Enforcement hygiene

- `scan_locknest2.py` is a screening tool: run it after any change that
  adds locks or guard-held calls; validate it first (it was once broken
  and silently reported 0 hits — always run the buggy/fixed scratch pair
  check).
- The runtime detector turns the I1 wedge (invisible, hours of
  forensics) into a named panic with the mutex address.
- Docs that contradict code are bugs: this ledger reflects the code as
  of 2026-09-04 and must be updated with any change that touches the
  enforcement points above.
# vDSO Design Rationale

## Executive Summary

The SKYIOUS vDSO is designed to be the **best vDSO in existence** — faster, more correct, more featureful, and more tailored to SKYIOUS KERNEL than any other kernel's vDSO (Linux, FreeBSD, OpenBSD, NetBSD, illumos, etc.). This document captures the design rationale, trade-offs, and alternatives considered.

---

## Design Principles

### 1. Control the Full Stack → No Compromises

SKYIOUS KERNEL controls the entire software stack:
- Kernel (this vDSO)
- Custom libc (`sash`/`init`)
- Custom loader (no `ld.so`)
- Custom userspace applications

**Implication**: We don't need Linux ABI compatibility as a ceiling — we need it as a floor. Every design decision starts from: *what would the ideal vDSO look like for this kernel?*

### 2. Per-CPU Pages (Not Shared)

**Decision**: Each CPU gets its own 4 KiB vDSO page at `VDSO_BASE + cpu_id * 4096`.

**Rationale**:
- **Zero cache-line false sharing**: Timer tick (100 Hz) updates time fields on each CPU's page independently. No bouncing of shared cache lines across sockets.
- **Simpler synchronization**: Each CPU owns its page. Timer tick runs on CPU N, updates page N. Context switch on CPU N updates PID/CPU on page N.
- **No remote NUMA access penalty**: Page is local to CPU by construction.

**Alternative considered**: Single shared page with per-CPU data segments.
- **Rejected**: Requires cache-line padding between CPU segments, still causes false sharing on seqlock counters, adds complexity for remote NUMA access.

### 3. Two Independent Seqlocks (Not One)

**Decision**: Separate seqlocks for time fields (`seq`) and PID/CPU fields (`pid_cpu_seq`).

**Rationale**:
- **Writer isolation**: Timer tick (100 Hz, IRQ context) writes time fields. Context switch (unbounded rate, thread context) writes PID/CPU. They never contend.
- **Reader fairness**: Time readers never blocked by context switch writers, and vice versa.
- **Priority inversion prevention**: Timer tick writer always succeeds within 1 tick (short critical section). Context switch writer can take longer but doesn't block time readers.

**Alternative considered**: Single seqlock for all fields.
- **Rejected**: Context switch under load could starve timer tick updates, causing time staleness.

### 4. TSC as Single Source of Truth

**Decision**: TSC is the only clock source. No NTP slew on monotonic clock.

**Rationale**:
- **Monotonic guarantee**: `CLOCK_MONOTONIC` never goes backward — ever. TSC is invariant on modern x86_64.
- **NTP only affects REALTIME**: Wall-clock adjustments via RTC base + TSC delta. Monotonic is pure TSC since boot.
- **Simplicity**: No leap second handling, no slew state machine, no adjtime/ntp_adjtime complexity in fast path.

**Alternative considered**: Linux-style NTP-adjusted monotonic with slew.
- **Rejected**: Violates "never goes backward" guarantee. Adds complexity to fast path.

### 5. vDSO as PRIMARY ABI (Syscall = Fallback)

**Decision**: Userspace libc uses vDSO fast paths as the primary ABI. Syscalls are fallback only.

**Rationale**:
- **Minimal syscall overhead**: `clock_gettime` and `getpid` become single memory loads (or TSC read + math) in userspace. `getcpu` still requires a syscall (or `rdpid` on x86) to determine the current CPU before reading the per-CPU page — this is faster than full `getpid`/`getcpu` syscalls but not zero overhead.
- **Custom libc**: `sash`/`init` are built with vDSO fast paths as the only path. No `vdso.so`/`ld.so` dance.
- **AT_SYSINFO_EHDR**: Points directly to vDSO base. No `dlopen`/`ld.so` resolution needed.

**Alternative considered**: Linux model (vdso as optimization, syscall as primary).
- **Rejected**: Adds syscall overhead to common case. Requires dynamic linker. Violates "best-in-class" goal.

---

### 6. Migration-Safe getpid/getcpu Mechanism

**Decision**: Per-CPU vDSO pages store PID/CPU for the currently running thread on that CPU. On thread migration, the new CPU's page is updated via context switch before the thread resumes.

**Mechanism**:
- Each CPU has its own vDSO page at `VDSO_BASE + cpu_id * 4096`
- Context switch on CPU N updates page N's PID/CPU fields via `pid_cpu_seq` seqlock
- Userspace reads from the page corresponding to its current CPU (determined via `getcpu` syscall or `rdpid` instruction on supported CPUs)
- The PID in the page always matches the process currently running on that CPU

**Why page-local seqlock is insufficient**: A simple page-local seqlock only protects against concurrent reads/writes on the same page. It doesn't handle migration where a thread moves from CPU A to CPU B - the thread must read from CPU B's page after migration, which requires the kernel to have updated CPU B's page before the thread resumes. The context switch on the target CPU ensures this ordering.

**Userspace read protocol**: To safely read per-CPU fields (PID, CPU ID) without kernel assistance:
1. Call `getcpu` syscall (or use `rdpid` on supported CPUs) to obtain current CPU ID.
2. Compute the vDSO page address: `VDSO_BASE + cpu_id * 4096`.
3. Read the `pid_cpu_seq` seqlock (Acquire), then read `pid` and `cpu_id` (Relaxed), then read `pid_cpu_seq` again (Acquire).
4. **Validate CPU identity**: Compare the `cpu_id` read from the vDSO page with the CPU ID from step 1. If they differ, the thread has migrated — retry from step 1 or fall back to the `getpid`/`getcpu` syscall.
5. If `pid_cpu_seq` is even and unchanged between the two reads, the data is consistent.

This validation ensures that a saved pointer to the original CPU's page is never used after migration, because the CPU ID check will fail and trigger a retry with the new CPU's page.

**Trade-offs**: 
- Requires context switch to update vDSO before thread resumes on new CPU
- Userspace must use `getcpu` syscall (or `rdpid` on x86) to find current CPU, then read from that page
- On x86 without `rdpid`, the `getcpu` syscall is a small overhead but still faster than full `getpid` syscall

---

## Architecture-Specific Decisions

### x86_64 (Primary)

| Feature | Implementation |
|---------|----------------|
| Clock source | TSC (`rdtsc` instruction) |
| Frequency calibration | CPUID.0x15 → PIT channel 2 measurement → LAPIC bus estimate |
| TSC conversion | Linux algorithm: `mult = (1e9 << shift) / freq`, shift chosen so mult fits in u32 |
| TSC sync | IPI broadcast every timer tick (CPU 0), drift check every 1s |
| Per-CPU pages | 4 KiB at `0x0000_7FFF_FFFC_0000 + cpu_id * 4096` |
| Seqlock atomics | `AtomicU64` with Acquire/Release ordering (x86 TSO makes this optimal) |

### aarch64 / riscv64 (Parity)

| Feature | aarch64 | riscv64 |
|---------|---------|---------|
| Clock source | `CNTVCT_EL0` (generic timer) | `time` CSR (CLINT) |
| Frequency | `CNTFRQ_EL0` | `riscv,timebase-frequency` DT property |
| Sync mechanism | IPI + `cntvct_el0` read | IPI + `time` CSR read |
| Timer frequency | Fixed per-platform | Fixed per-platform |

**Abstraction**: `TimerSource` trait with `get_frequency()`, `read_timer()`, `timer_delta_to_ns()`. Architecture-agnostic `vdso_read_timer_ns()`.

---

## Trade-offs and Alternatives Considered

### 1. Seqlock vs. RCU vs. Lock-Free

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| **Seqlock** (chosen) | Simple, wait-free readers, single writer | Writers can starve readers | ✅ Best for single-writer, many-reader (timer tick, context switch) |
| RCU | Wait-free readers/writers | Complex grace periods, memory overhead | ❌ Overkill for single-page updates |
| Lock-free CAS | No locks | ABA problem, complex multi-field updates | ❌ Hard to get right for multi-field (sec + nsec) |

### 2. Per-CPU Pages vs. Shared Page with CPU Segments

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| **Per-CPU pages** (chosen) | Zero false sharing, simple sync, NUMA-local | 4 KiB × MAX_CPUS memory | ✅ Memory is cheap; cache line ping-pong is expensive |
| Shared page + padding | Less memory | False sharing on seqlock counters, remote NUMA access | ❌ False sharing kills scalability |

### 3. TSC Sync Cadence

| Interval | Mechanism | Drift Bound | Overhead | Status |
|----------|-----------|-------------|----------|--------|
| **Every tick (10ms)** | IPI broadcast to all CPUs; each AP updates its `tsc_base` | ~100ns (target, not measured) | 1 IPI + up to 1,000,000 spin-loop iterations per tick for ack wait | ✅ Primary sync |
| **Every 100 ticks (1s)** | `vdso_check_tsc_drift()` reads all online CPUs' `tsc_base` and compares | Logs warning if > 100ns | Samples taken at different times; skew bound by sampling window | ✅ Drift detection |
| On-demand (drift > threshold) | Not implemented | Variable | Unpredictable latency | ❌ Non-deterministic |

**Note**: The "Every tick" bound of <100ns is a *design target*. The diagnostic compares samples taken at different times (sequential reads of each CPU's page), so the measured difference includes sampling skew. The ack wait loop allows up to 1,000,000 iterations before timeout.

### 4. Drift Threshold

| Threshold | False Positive Rate | Detection Speed | Decision |
|-----------|---------------------|-----------------|----------|
| 1ms (current warning threshold) | Low | 1s | ⚠️ Start conservative |
| **100ns (target for production)** | Medium | 10ms | ✅ Goal; labeled as target, not measured result |
| 1μs | High | 10ms | ❌ Too noisy |

---

## Memory Layout Evolution

### v1 (Initial)
```
0x000: seq (8)
0x008: monotonic_sec (8)
0x010: monotonic_nsec (4) + pad (4)
0x018: realtime_sec (8)
0x020: realtime_nsec (4) + pad (4)
0x028: tsc_freq_hz (8)
0x030: tsc_mult (4) + tsc_shift (4)
0x038: tsc_base (8)
0x040: pid_cpu_seq (8)
0x048: pid (8)
0x050: cpu_id (4) + pad (4)
0x058-0xFFF: reserved (~4032 bytes)
```

### v2 (SKYIOUS Clocks) — Current
Added at 0x040+:
```
0x040: tsc_base_origin (8)             // Original TSC base at boot (preserved across syncs)
0x048: pid_cpu_seq (8)                 // Seqlock for PID/CPU fields
0x050: pid (8)                         // Current process ID
0x058: cpu_id (4) + pad (4)            // Current CPU ID
0x060: thread_cputime_ns (8)           // CLOCK_SKYIOUS_THREAD_CPUTIME (100)
0x068: kernel_flags (8)                // CLOCK_SKYIOUS_KERNEL_FLAGS (103)
0x070: process_cgroup_id (8)           // CLOCK_SKYIOUS_PROCESS_INFO (104)
0x078: process_namespace_flags (8)     // CLOCK_SKYIOUS_PROCESS_INFO (104)
0x080: process_cap_effective (8)       // CLOCK_SKYIOUS_PROCESS_INFO (104)
0x088: tsc_sync_ack_count (4)          // TSC sync IPI acknowledgment count
0x08C: tsc_sync_seq (4)                // TSC sync sequence number
```

**Total used: ~144 bytes of 4096**. Plenty of room for future clocks.

### CLOCK_REALTIME: RTC-Cached TSC Interpolation

**Problem**: TSC synchronization (IPI broadcast every tick) updates `tsc_base` on each CPU. If `CLOCK_REALTIME` used `tsc_base` directly, every TSC sync would cause a step change in the reported wall-clock time.

**Solution**: The kernel caches a consistent (RTC sec, RTC nsec, TSC timestamp) triple under a seqlock (`CACHED_RTC_SEQ`). `CLOCK_REALTIME` interpolates from this cached TSC timestamp (`cached_rtc_tsc`), not from the live `tsc_base`. TSC synchronization updates `tsc_base` but leaves the cached RTC sample unchanged, so wall-clock time is continuous across syncs.

**RTC Seqlock Upgrade**: The RTC seqlock now uses `compare_exchange_weak` with `AcqRel` ordering to serialize writers across CPUs, preventing writer starvation. The cached sample includes `CACHED_RTC_TSC` (the TSC value at the moment of RTC read).

---

## Formal Verification

### Kani Proofs (4/4 passing)

| Proof | Property Verified |
|-------|-------------------|
| `seqlock_read_never_returns_torn_data` | Reader never returns inconsistent (torn) data |
| `seqlock_writer_ends_even` | Writer always leaves seq even (seq+2) |
| `seqlock_reader_detects_concurrent_write` | Reader detects concurrent writes |
| `vdso_time_seqlock_correctness` | Full VDSO time seqlock protocol correctness |

**Toolchain**: Kani 0.68.0 + CBMC 6.11.0 + CaDiCaL 3.0.0
**Environment**: Native Linux with matching nightly toolchain required.

---

## Performance Targets

| Metric | Target | Current (QEMU/TCG) | Status |
|--------|--------|--------------------|--------|
| `clock_gettime` latency | < 5 ns | ~1061 ns p50 | ❌ TCG overhead |
| `getpid` latency | < 3 ns | ~450 ns p50 | ❌ TCG overhead |
| `getcpu` latency | < 3 ns | ~1251 ns p50 | ❌ TCG overhead |
| `clock_getres` latency | < 1 ns | ~20 ns p50 | ❌ TCG overhead |
| TSC drift | < 100 ns | N/A (TSC=0 in QEMU) | ❓ Hardware needed |
| Seqlock retry rate | < 0.01% | 0 in 2000 reads | ✅ |
| Code size | < 2 KiB | ~2.5 KiB | ~ |

**Note**: QEMU/TCG has no invariant TSC and emulates `rdtsc` via host gettimeofday — figures are not hardware latency.

---

## Future Extensions

1. **Additional SKYIOUS clocks**:
   - `CLOCK_SKYIOUS_THREAD_START_NS` — thread creation time
   - `CLOCK_SKYIOUS_SCHED_RUNTIME_NS` — remaining runtime for deadline threads
   - `CLOCK_SKYIOUS_INTERRUPT_COUNT` — interrupt counter for profiling

2. **Hardware validation**:
   - Run benchmarks on physical hardware with invariant TSC
   - Validate <5ns/<3ns/<1ns targets
   - Measure actual TSC drift across CPUs under load

3. **Userspace integration**:
   - `vdso-userspace.md` — libc integration guide
   - `vdso-clocks.md` — complete clock ID registry

---

## References

- Linux vDSO implementation: `arch/x86/entry/vdso/`
- FreeBSD vDSO: `sys/x86/vdso/`
- "Seqlocks in the Linux Kernel" — LWN.net
- "TSC Synchronization in Linux" — Linux Kernel Documentation
- Kani Verification: `kernel/src/tests/kani_test/`
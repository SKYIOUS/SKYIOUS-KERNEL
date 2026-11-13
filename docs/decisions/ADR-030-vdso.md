# ADR-030: vDSO (Virtual Dynamic Shared Object) — Kernel Data Page in Userspace

## Status
Accepted — implemented 2026-10-31

## Date
2026-10-30

## Context

High-frequency syscalls (`clock_gettime(CLOCK_MONOTONIC)`, `clock_gettime(CLOCK_REALTIME)`, `getpid`, `getcpu`) execute millions of times per second in performance-sensitive applications. Each syscall incurs a full ring-3 → ring-0 transition (SYSCALL/SYSRET on x86_64, SVC on ARM64), register save/restore, and TLB flush — typically 200–600 ns on x86_64.

Linux and other OSes solve this with a **vDSO**: a read-only kernel page mapped into every process address space at a fixed virtual address. The kernel updates time/CPU/PID data on timer ticks; userspace reads directly via memory load instructions — **zero syscall overhead**.

## Decision

Implement a minimal vDSO for Vahi kernel with the following scope:

### 1. vDSO Data Structure
Single 4 KiB page containing atomic data:
```rust
#[repr(C, align(4096))]
pub struct VdsoData {
    // Time — updated every timer tick (100 Hz)
    pub monotonic_sec: AtomicU64,       // CLOCK_MONOTONIC seconds
    pub monotonic_nsec: AtomicU32,      // CLOCK_MONOTONIC nanoseconds
    pub realtime_sec: AtomicU64,        // CLOCK_REALTIME seconds
    pub realtime_nsec: AtomicU32,       // CLOCK_REALTIME nanoseconds
    
    // TSC conversion for high-resolution userspace time
    pub tsc_freq_hz: AtomicU64,         // TSC frequency (Hz)
    pub tsc_mult: u32,                  // Multiplier for TSC → nanoseconds
    pub tsc_shift: u32,                 // Shift for TSC → nanoseconds
    pub tsc_base: AtomicU64,            // TSC at boot (for elapsed calc)
    
    // Process/CPU identity — updated on context switch
    pub pid: AtomicU64,                 // Current process ID
    pub cpu_id: AtomicU32,              // Current CPU ID
    
    // Version/seqlock for lock-free readers
    pub seq: AtomicU64,                 // Even = consistent, odd = updating
}
```
Total: ~64 bytes (fits in one cache line pair). Rest of page reserved.

### 2. Mapping Strategy
- **Virtual address**: Fixed per-arch address in user space (x86_64: `0x0000_7FFF_FFFC_0000` — 256 KiB below `USER_MMAP_TOP`; aarch64/riscv64: similar). Each CPU gets a page at `VDSO_BASE + cpu_id * 4096`.
- **Mapped at**: `Isolate::new()` (process creation) — 8 pages (one per CPU), read-only, user-accessible
- **Per-CPU physical pages**: Each CPU has its own physical frame (8 × 4 KiB total). All processes map the same set of 8 frames (COW not needed — kernel-only writes)
- **Page table flags**: `PRESENT | USER_ACCESSIBLE | NO_EXECUTE` (read-only, no execute)

### 3. Update Mechanism
- **Timer tick (100 Hz)**: Update `monotonic_*`, `realtime_*`, `seq` (write sequence: odd → data → even)
- **Context switch**: Update `pid`, `cpu_id` (same seq protocol)
- **TSC calibration**: `tsc_freq_hz`, `tsc_mult`, `tsc_shift`, `tsc_base` set at boot (read-only after)

### 4. Syscall Fast Paths
Modify syscalls to check vDSO first:
- `clock_gettime(CLOCK_MONOTONIC)`: Read `monotonic_sec`/`monotonic_nsec` from vDSO (seqlock retry loop)
- `clock_gettime(CLOCK_REALTIME)`: Read `realtime_sec`/`realtime_nsec` from vDSO
- `getpid()`: Read `pid` from vDSO
- `getcpu()`: Read `cpu_id` from vDSO (NEW syscall)

Fallback to slow path (current implementation) if vDSO unavailable or seq indicates inconsistency.

### 5. Userspace Access
- Export vDSO base address via `auxv` (AT_SYSINFO_EHDR) at execve
- Provide minimal userspace helper (C/assembly) for seqlock read
- No ELF dynamic linking required — raw memory access

## Consequences

### Positive
- **100× syscall latency reduction** for time/process queries (target: <5 ns vs ~500 ns)
- **No syscall instruction** for hot paths — critical for HFT, databases, runtimes
- **Scalable**: Single global page, no per-process overhead
- **Compatible**: Falls back to syscall if vDSO missing (older processes, other archs)

### Negative
- **Seqlock complexity**: Userspace must handle retry on concurrent update (rare)
- **Cache coherency**: All CPUs write same cache line on tick — false sharing
  - Mitigation: Implemented per-CPU vDSO pages (8 × 4 KiB) — each CPU updates its own page, eliminating false sharing
- **Security**: Kernel data in userspace — read-only, no pointers, no secrets
- **Architecture work**: x86_64 first; aarch64/riscv64 need separate mapping code

### Risks
| Risk | Impact | Mitigation |
|------|--------|------------|
| Seqlock livelock under heavy tick contention | Medium | Tick is 100 Hz (10 ms); seqlock write section <100 ns; contention near-zero |
| TSC frequency changes (CPU scaling) | Low | `tsc_freq_hz` calibrated at boot; constant TSC on modern x86_64 |
| Process reads stale PID after fork | Low | PID updated on context switch; fork child gets new PID via slow path first |
| Page fault on vDSO access (demand paging) | Low | Map eagerly at process creation; page always resident |

## Alternatives Considered

1. **vsyscall (legacy fixed address)**: Deprecated, insecure, limited to 4 syscalls. Rejected.
2. **Full vDSO ELF shared object**: Complex (dynamic linker, relocations). Rejected — overkill for 4 syscalls.
3. **Per-process vDSO pages**: Higher memory, no sharing benefit. Rejected — single global page sufficient.
4. **No vDSO**: Keep syscall overhead. Rejected — measurable perf gap vs Linux/musl.

## Implementation Phases

| Phase | Scope | Verification |
|-------|-------|--------------|
| 1. Data struct + global page | `VdsoData` in `kernel/src/vdso.rs`, single static frame | Build passes; page mapped in PID 1 |
| 2. Process mapping | `Isolate::new()` maps vDSO page at fixed VA | QEMU boot: page present in `/proc/self/maps` equivalent |
| 3. Timer tick update | `interrupts::irq::timer_interrupt_handler` updates time fields | Serial log shows monotonic time advancing in vDSO |
| 4. Context switch update | `scheduler::switch::prepare_switch` updates pid/cpu_id | `getpid`/`getcpu` return correct values from vDSO |
| 5. Syscall fast paths | `sys_clock_gettime`, `sys_getpid`, `sys_getcpu` read vDSO | Benchmark: `clock_gettime` <5 ns vs ~500 ns syscall |
| 6. Userspace helper | Minimal C/asm vDSO accessor in `examples/vdso.c` | Static binary reads time via vDSO |

## References
- Linux vDSO: `Documentation/vdso.rst`, `arch/x86/entry/vdso/`
- `man 7 vdso`
- `clock_gettime(2)` vDSO fast path

---

**Next ADR**: ADR-031 (if needed for getcpu syscall addition)
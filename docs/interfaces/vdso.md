# vDSO ABI Specification

## Overview

The vDSO (Virtual Dynamic Shared Object) provides lock-free, syscall-free access to time, PID, and CPU ID from userspace. Each CPU gets its own 4 KiB page mapped at a fixed virtual address range.

## Memory Layout

### Base Address
```
VDSO_BASE = 0x0000_7FFF_FFFC_0000  // 256 KiB below USER_MMAP_TOP
```

### Per-CPU Pages
Each CPU has a page at:
```
vdso_virt_addr(cpu_id) = VDSO_BASE + cpu_id * 4096
```

### Data Structure (4096 bytes, cache-line aligned)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0x000 | 8 | `seq` | Seqlock counter (even = consistent, odd = updating) |
| 0x008 | 8 | `monotonic_sec` | Monotonic clock seconds |
| 0x010 | 4 | `monotonic_nsec` | Monotonic clock nanoseconds |
| 0x014 | 4 | `_pad1` | Padding |
| 0x018 | 8 | `realtime_sec` | Real-time clock seconds |
| 0x020 | 4 | `realtime_nsec` | Real-time clock nanoseconds |
| 0x024 | 4 | `_pad2` | Padding |
| 0x028 | 8 | `tsc_freq_hz` | TSC frequency in Hz (read-only after init) |
| 0x030 | 4 | `tsc_mult` | TSC multiplier (read-only after init) |
| 0x034 | 4 | `tsc_shift` | TSC shift (read-only after init) |
| 0x038 | 8 | `tsc_base` | TSC base value at last sync (AtomicU64) |
| 0x040 | 8 | `tsc_base_origin` | Original TSC base at boot (preserved across syncs) |
| 0x048 | 8 | `pid_cpu_seq` | Seqlock for PID/CPU fields |
| 0x050 | 8 | `pid` | Current process ID (AtomicU64) |
| 0x058 | 4 | `cpu_id` | Current CPU ID (AtomicU32) |
| 0x05C | 4 | `_pad3` | Padding |
| 0x060 | 8 | `thread_cputime_ns` | Thread CPU time for CLOCK_SKYIOUS_THREAD_CPUTIME (100) |
| 0x068 | 8 | `kernel_flags` | Kernel feature flags for CLOCK_SKYIOUS_KERNEL_FLAGS (103) |
| 0x070 | 8 | `process_cgroup_id` | Cgroup ID for CLOCK_SKYIOUS_PROCESS_INFO (104) |
| 0x078 | 8 | `process_namespace_flags` | Namespace flags for CLOCK_SKYIOUS_PROCESS_INFO (104) |
| 0x080 | 8 | `process_cap_effective` | Effective capabilities for CLOCK_SKYIOUS_PROCESS_INFO (104) |
| 0x088 | 4 | `tsc_sync_ack_count` | TSC sync IPI acknowledgment count |
| 0x08C | 4 | `tsc_sync_seq` | TSC sync sequence number |
| 0x090 | 4 | `vdso_abi_version` | vDSO ABI version (monotonic integer) |
| 0x094+ | ~3948 | `_reserved` | Reserved for future use |

## Seqlock Protocol

### Time Fields (seq)
- Writer (timer tick): increments to odd, writes data, increments to even
- Reader: reads seq (Acquire), reads data (Relaxed), reads seq again (Acquire), retries if seq changed or odd

### PID/CPU Fields (pid_cpu_seq)
- Writer (context switch): increments to odd, writes PID/CPU, increments to even
- Reader: same protocol as time fields

### RTC Cached Sample (CACHED_RTC_SEQ)
- Writer (RTC update thread): uses `compare_exchange_weak` with AcqRel ordering to serialize across CPUs; writes sec, nsec, and TSC timestamp atomically; increments to even
- Reader: same protocol as time fields; reads (sec, nsec, tsc_timestamp) as a consistent triple

## Auxiliary Vector Entry

### AT_SYSINFO_EHDR (type 33)
Value: `VDSO_BASE` (CPU 0's page address)

Userspace computes per-CPU page address:
```
vdso_page(cpu_id) = AT_SYSINFO_EHDR + cpu_id * 4096
```

## Supported Clocks (Fast Path)

| Clock ID | Name | Fast Path | Notes |
|----------|------|-----------|-------|
| 0 | CLOCK_REALTIME | ✅ TSC + cached RTC | Interpolates from cached RTC + TSC delta |
| 1 | CLOCK_MONOTONIC | ✅ TSC-based | TSC elapsed since `tsc_base` |
| 5 | CLOCK_REALTIME_COARSE | ✅ Tick-based | 10ms resolution, no seqlock |
| 6 | CLOCK_MONOTONIC_COARSE | ✅ Tick-based | 10ms resolution, no seqlock |
| 7 | CLOCK_BOOTTIME | ✅ Same as MONOTONIC | Uses cached monotonic snapshot |
| 4 | CLOCK_MONOTONIC_RAW | ✅ Same as MONOTONIC | Uses cached monotonic snapshot |
| 102 | CLOCK_SKYIOUS_BOOT_NS | ✅ TSC + `tsc_base_origin` | Raw TSC ns since boot (preserved across TSC syncs) |
| 103 | CLOCK_SKYIOUS_KERNEL_FLAGS | ✅ Direct atomic load | Kernel feature bitfield |
| 104 | CLOCK_SKYIOUS_PROCESS_INFO | ✅ Direct atomic loads | Cgroup ID, namespace flags, capabilities |
| 105 | CLOCK_SKYIOUS_VDSO_VERSION | ✅ Direct atomic load | vDSO ABI version (monotonic integer) |

Other clocks (2=CLOCK_PROCESS_CPUTIME_ID, 3=CLOCK_THREAD_CPUTIME_ID, 100=CLOCK_SKYIOUS_THREAD_CPUTIME, 101=CLOCK_SKYIOUS_SCHED_DEADLINE) fall back to syscall because:
- Thread CPU time (3, 100) omits the current running slice in the vDSO fast path
- Process CPU time (2) requires kernel aggregation
- Scheduler deadline (101) is not exposed via vDSO

## TSC Time Calculation

### CLOCK_MONOTONIC (fast path)
```
elapsed = rdtsc() - tsc_base
ns = (elapsed * tsc_mult) >> tsc_shift
sec = ns / 1e9
nsec = ns % 1e9
```



### CLOCK_REALTIME (fast path)
```
elapsed = rdtsc() - cached_rtc_tsc
ns = (elapsed * tsc_mult) >> tsc_shift
base_ns = cached_rtc_sec * 1e9 + cached_rtc_nsec  // paired with cached_rtc_tsc under the RTC seqlock
total_ns = base_ns + ns
sec = total_ns / 1e9
nsec = total_ns % 1e9
```



### CLOCK_SKYIOUS_BOOT_NS (fast path)
```
elapsed_boot = rdtsc() - tsc_base_origin  // preserved across TSC syncs
ns = (elapsed_boot * tsc_mult) >> tsc_shift
sec = ns / 1e9
nsec = ns % 1e9
```

### Kernel interpolation (vdso_read_tsc_time)
The kernel-side `vdso_read_tsc_time()` implements the above. For CLOCK_REALTIME it reads a seqlock-protected RTC sample (sec, nsec, TSC timestamp) and interpolates from that timestamp. TSC synchronization does not change the cached sample or its timestamp. `get_cached_rtc()` returns the time fields of the same sample. The RTC seqlock now uses `compare_exchange_weak` for writer serialization across CPUs.

Userspace libc reads the cached `realtime_sec`/`realtime_nsec` fields directly (seqlock-protected by `seq`), which are updated by the timer tick handler at 100 Hz.

TSC calibration uses Linux algorithm: `mult = (1e9 << shift) / freq` where `shift` is chosen so `mult` fits in u32.

## Userspace Access Example

```c
// Get vDSO base from auxv
extern uint64_t __vdso_base;

struct vdso_data {
    volatile uint64_t seq;
    volatile uint64_t monotonic_sec;
    volatile uint32_t monotonic_nsec;
    volatile uint64_t realtime_sec;
    volatile uint32_t realtime_nsec;
    volatile uint64_t tsc_freq_hz;
    volatile uint32_t tsc_mult;
    volatile uint32_t tsc_shift;
    volatile uint64_t tsc_base;
    volatile uint64_t tsc_base_origin;  /* Original TSC base at boot */
    volatile uint64_t pid_cpu_seq;
    volatile uint64_t pid;
    volatile uint32_t cpu_id;
    uint32_t _pad3;
    volatile uint64_t thread_cputime_ns;   /* CLOCK_SKYIOUS_THREAD_CPUTIME (100) */
    volatile uint64_t kernel_flags;        /* CLOCK_SKYIOUS_KERNEL_FLAGS (103) */
    volatile uint64_t process_cgroup_id;   /* CLOCK_SKYIOUS_PROCESS_INFO (104) */
    volatile uint64_t process_namespace_flags;
    volatile uint64_t process_cap_effective;
    volatile uint32_t tsc_sync_ack_count;
    volatile uint32_t tsc_sync_seq;
};

struct vdso_data *vdso = (struct vdso_data *)__vdso_base;

uint64_t read_monotonic() {
    uint64_t seq, sec, nsec;
    do {
        seq = __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE);
        if (seq & 1) continue;  // Writer in progress
        sec = __atomic_load_n(&vdso->monotonic_sec, __ATOMIC_RELAXED);
        nsec = __atomic_load_n(&vdso->monotonic_nsec, __ATOMIC_RELAXED);
    } while (seq != __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE) || (seq & 1));
    return sec * 1000000000 + nsec;
}
```

## Per-CPU Access

```c
struct vdso_data *get_vdso_page(int cpu_id) {
    return (struct vdso_data *)(__vdso_base + cpu_id * 4096);
}
```

## Version

Current ABI version: 1 (`vdso_abi_version = 1`)
- Kernel: Vahi Kernel v0.3.0+
- Architecture: x86_64
- Policy: Monotonic integer. Incremented ONLY on incompatible layout/semantic changes.
  Compatible additions (new fields at end, new clock IDs, new kernel_flags bits) do NOT bump version.
- Userspace reads version at startup via `CLOCK_SKYIOUS_VDSO_VERSION` (105); mismatch → syscall-only fallback.
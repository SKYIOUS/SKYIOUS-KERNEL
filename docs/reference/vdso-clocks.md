# vDSO Clock ID Registry

This document catalogs all clock IDs supported by the SKYIOUS Kernel vDSO,
their fast-path availability, and implementation details.

## Clock ID Table

| ID | Name | vDSO Fast Path | Source | Resolution | Linux Compat |
|----|------|----------------|--------|------------|--------------|
| 0 | `CLOCK_REALTIME` | Yes | TSC + cached RTC | 1 ns | Yes |
| 1 | `CLOCK_MONOTONIC` | Yes | TSC | 1 ns | Yes |
| 2 | `CLOCK_PROCESS_CPUTIME_ID` | No | Syscall (scheduler) | 1 ns | Yes |
| 3 | `CLOCK_THREAD_CPUTIME_ID` | No | Syscall (scheduler) | 1 ns | Yes |
| 4 | `CLOCK_MONOTONIC_RAW` | Yes | TSC (same as MONOTONIC) | 1 ns | Yes |
| 5 | `CLOCK_REALTIME_COARSE` | **Yes (P1)** | Ticks (10 ms) | 10 ms | Yes |
| 6 | `CLOCK_MONOTONIC_COARSE` | **Yes (P1)** | Ticks (10 ms) | 10 ms | Yes |
| 7 | `CLOCK_BOOTTIME` | Yes | TSC (same as MONOTONIC) | 1 ns | Yes |
| 8 | `CLOCK_REALTIME_ALARM` | No | Not implemented | — | Yes |
| 9 | `CLOCK_BOOTTIME_ALARM` | No | Not implemented | — | Yes |

## SKYIOUS-Native Clocks (100+)

| ID | Name | vDSO Fast Path | Source | Resolution |
|----|------|----------------|--------|------------|
| 100 | `CLOCK_SKYIOUS_THREAD_CPUTIME` | No | Syscall (thread_cputime_ns omits running slice) | 1 ns |
| 101 | `CLOCK_SKYIOUS_SCHED_DEADLINE` | No | Syscall (scheduler) | N/A |
| 102 | `CLOCK_SKYIOUS_BOOT_NS` | Yes | TSC + `tsc_base_origin` | 1 ns |
| 103 | `CLOCK_SKYIOUS_KERNEL_FLAGS` | Yes | `kernel_flags` atomic load | N/A |
| 104 | `CLOCK_SKYIOUS_PROCESS_INFO` | Yes | Process info atomic loads | N/A |
| 105 | `CLOCK_SKYIOUS_VDSO_VERSION` | **Yes (P1)** | `vdso_abi_version` atomic load | N/A |

## Implementation Details

### CLOCK_REALTIME (0) — TSC + Cached RTC

- **Fast path**: Reads `realtime_sec`/`realtime_nsec` from vDSO page (updated at 100 Hz by timer tick)
- **Seqlock**: Protected by `seq` (even = consistent)
- **Precision**: 1 ns (TSC interpolation between ticks)
- **RTC sync**: Kernel caches consistent (RTC sec, RTC nsec, TSC) triple under separate seqlock (`CACHED_RTC_SEQ`). Timer tick reads this cached sample, so TSC synchronization does not step wall-clock time.

### CLOCK_MONOTONIC (1) — Pure TSC

- **Fast path**: `elapsed = rdtsc() - tsc_base`; `ns = (elapsed * tsc_mult) >> tsc_shift`
- **Seqlock**: Protected by `seq` (reads `monotonic_sec`/`monotonic_nsec` from vDSO page)
- **Precision**: 1 ns
- **Monotonic guarantee**: Never goes backward. TSC is invariant on modern x86_64.

### CLOCK_REALTIME_COARSE (5) — Tick-Based (P1 New)

- **Fast path**: Reads `ticks` counter directly: `sec = ticks * 10 / 1000`, `nsec = (ticks * 10 % 1000) * 1_000_000` + cached RTC offset
- **Seqlock**: **None needed** — single atomic read of `ticks` is consistent on x86_64
- **Resolution**: 10 ms (10,000,000 ns)
- **Use case**: Timeout calculations, polling loops where 10 ms precision is sufficient

### CLOCK_MONOTONIC_COARSE (6) — Tick-Based (P1 New)

- **Fast path**: Reads `ticks` counter directly: `sec = ticks * 10 / 1000`, `nsec = (ticks * 10 % 1000) * 1_000_000`
- **Seqlock**: **None needed** — single atomic read of `ticks` is consistent on x86_64
- **Resolution**: 10 ms (10,000,000 ns)
- **Use case**: Timeout calculations, polling loops

### CLOCK_SKYIOUS_BOOT_NS (102) — Raw TSC Since Boot

- **Fast path**: `elapsed_boot = rdtsc() - tsc_base_origin`; `ns = (elapsed_boot * tsc_mult) >> tsc_shift`
- **Seqlock**: None (read-only after init)
- **Precision**: 1 ns
- **Key property**: Uses `tsc_base_origin` (preserved across TSC synchronizations), so boot-relative time never steps.

### CLOCK_SKYIOUS_KERNEL_FLAGS (103) — Feature Bitfield

- **Fast path**: Direct atomic load of `kernel_flags`
- **Seqlock**: None (read-only after init, updated only by drivers via atomic OR)
- **Bits**:
  - Bit 0: `HAS_AVX2` (CPUID leaf 7, EBX bit 5)
  - Bit 1: `HAS_NVMe`
  - Bit 2: `HAS_xHCI`
  - Bit 3: `HAS_VIRTIO_GPU`
  - Bits 16-31: Net stack version (high 16 of tv_nsec)
  - Bits 0-15: FS features (low 16 of tv_nsec)

### CLOCK_SKYIOUS_PROCESS_INFO (104) — Per-Process Info

- **Fast path**: Direct atomic loads of `process_cgroup_id`, `process_namespace_flags`, `process_cap_effective`
- **Seqlock**: None (updated at fork/exec/cred change via `vdso_update_process_info`)
- **Return format**:
  - `tv_sec`: cgroup_id (low 32 bits)
  - `tv_nsec`: namespace_flags (low 32) | (cap_effective << 32)

### CLOCK_SKYIOUS_VDSO_VERSION (105) — ABI Version (P1 New)

- **Fast path**: Direct atomic load of `vdso_abi_version`
- **Seqlock**: None (read-only after init)
- **Value**: Monotonic integer (1 = current layout)
- **Policy**: Increment ONLY on incompatible layout/semantic changes

## Resolution Values (clock_getres)

| Clock ID | tv_sec | tv_nsec |
|----------|--------|---------|
| 0, 1, 4, 7, 102, 103, 104, 105 | 0 | 1 |
| 5, 6 | 0 | 10,000,000 |
| 2, 3, 100, 101 | 0 | 1 |
| 8, 9 | 0 | 0 (EINVAL) |

## vDSO Data Structure Offsets

| Offset | Field | Clock IDs |
|--------|-------|-----------|
| 0x000 | `seq` | 0, 1, 4, 7 |
| 0x008 | `monotonic_sec` | 1, 4, 7 |
| 0x010 | `monotonic_nsec` | 1, 4, 7 |
| 0x018 | `realtime_sec` | 0 |
| 0x020 | `realtime_nsec` | 0 |
| 0x028 | `tsc_freq_hz` | 0, 1, 4, 7, 102 |
| 0x030 | `tsc_mult` | 0, 1, 4, 7, 102 |
| 0x034 | `tsc_shift` | 0, 1, 4, 7, 102 |
| 0x038 | `tsc_base` | 0, 1, 4, 7 |
| 0x040 | `tsc_base_origin` | 102 |
| 0x048 | `pid_cpu_seq` | getpid, getcpu |
| 0x050 | `pid` | getpid |
| 0x058 | `cpu_id` | getcpu |
| 0x060 | `thread_cputime_ns` | 100 |
| 0x068 | `kernel_flags` | 103 |
| 0x070 | `process_cgroup_id` | 104 |
| 0x078 | `process_namespace_flags` | 104 |
| 0x080 | `process_cap_effective` | 104 |
| 0x088 | `tsc_sync_ack_count` | — |
| 0x08C | `tsc_sync_seq` | — |
| 0x090 | `vdso_abi_version` | 105 |

## TSC Conversion Algorithm

From Linux kernel (`arch/x86/kernel/tsc.c`):

```c
// mult = (1e9 << shift) / freq
// Choose shift so mult fits in u32
if (freq <= 1_000_000_000) {
    shift = 0;
    mult = 1_000_000_000 / freq;
} else {
    shift = 32 - fls(freq / 1_000_000_000);
    mult = (1_000_000_000ULL << shift) / freq;
}
```

Then: `ns = (elapsed * mult) >> shift`

## Adding New Clocks

1. Assign next available ID (106+ for SKYIOUS-native, or Linux-compatible)
2. Add to `clock_resolution()` in `syscalls/misc.rs`
3. Add fast path in `vdso_read_tsc_time()` (vdso.rs) if TSC-based
4. Add syscall dispatch in `sys_clock_gettime_kernel()` and `sys_clock_gettime()`
5. Add userspace implementation in `vdso_libc.c`
6. Add selftest in `vdso_tests.rs`
7. Update this document

## References

- Linux vDSO: `arch/x86/entry/vdso/vclock_gettime.c`
- `vdso.md` — ABI specification
- `vdso-design.md` — Design rationale
- `vdso-userspace.md` — Integration guide
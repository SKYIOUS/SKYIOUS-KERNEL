# vDSO libc - vDSO as PRIMARY ABI for SKYIOUS KERNEL

## Overview

This is a minimal libc wrapper that demonstrates **vDSO as the PRIMARY ABI** for SKYIOUS KERNEL.

Unlike Linux where vDSO is an optimization (syscalls are primary, vDSO is fallback), in SKYIOUS KERNEL:
- **vDSO is the PRIMARY ABI** - all time/PID/CPU operations use vDSO directly
- Syscalls are ONLY used as fallback when vDSO is unavailable

## Files

- `vdso_libc.c` - vDSO libc implementation (vDSO primary ABI for clock_gettime, getpid, getcpu, clock_getres)
- `vdso_libc_test.c` - Test program demonstrating vDSO as primary ABI
- `Makefile` - Build with musl-gcc

## Supported Clocks (vDSO Primary)

| Clock ID | Value | Description |
|----------|-------|-------------|
| `CLOCK_REALTIME` | 0 | Wall-clock time |
| `CLOCK_MONOTONIC` | 1 | Monotonic time (vDSO primary) |
| `CLOCK_PROCESS_CPUTIME_ID` | 2 | Process CPU time |
| `CLOCK_THREAD_CPUTIME_ID` | 3 | Thread CPU time |
| `CLOCK_MONOTONIC_RAW` | 4 | Raw monotonic |
| `CLOCK_BOOTTIME` | 7 | Boottime |
| `CLOCK_SKYIOUS_THREAD_CPUTIME` | 100 | SKYIOUS: thread CPU time |
| `CLOCK_SKYIOUS_SCHED_DEADLINE` | 101 | SKYIOUS: scheduler deadline |
| `CLOCK_SKYIOUS_BOOT_NS` | 102 | SKYIOUS: raw TSC ns since boot |
| `CLOCK_SKYIOUS_KERNEL_FLAGS` | 103 | SKYIOUS: kernel feature flags |
| `CLOCK_SKYIOUS_PROCESS_INFO` | 104 | SKYIOUS: per-process info (cgroup, namespaces, caps) |

## vDSO Primary ABI Functions

| Function | vDSO Primary? | Fallback |
|----------|---------------|----------|
| `clock_gettime()` | ✅ Yes | Syscall |
| `clock_getres()` | ✅ Yes | Syscall |
| `getpid()` | ✅ Yes | Syscall |
| `getcpu()` | ✅ Yes | Syscall |
| `clock_getres()` | ✅ Yes | Syscall |

## Building

```bash
# Requires musl-gcc (static linking)
make
```

## Testing in QEMU

```bash
# 1. Build the test
make

# 2. Copy to ramdisk/initrd as /bin/init
# 3. Boot kernel with initrd
# 4. Or run manually in QEMU shell: ./vdso_libc_test
```

## Expected Output

```
=== vDSO as PRIMARY ABI Demo ===
CLOCK_MONOTONIC: 123.456789012
CLOCK_REALTIME: 1702345678.123456789
CLOCK_SKYIOUS_BOOT_NS: 123.456789012
CLOCK_SKYIOUS_KERNEL_FLAGS: sec=0x7 nsec=0x10001
CLOCK_SKYIOUS_PROCESS_INFO: cgroup=0x1 ns=0x1
getpid(): 1
getcpu(): 0
clock_getres(CLOCK_MONOTONIC): 1 ns
clock_getres(SKYIOUS_BOOT_NS): 1 ns
TSC time (ns since boot): 123456789012

All operations used vDSO as PRIMARY ABI!
```

## Design Principles

1. **vDSO is PRIMARY** - All clock/PID/CPU operations try vDSO first
2. **Syscalls are FALLBACK** - Only used when vDSO unavailable (freq=0, page not mapped)
3. **No syscall overhead** for common operations in steady state
4. **SKYIOUS extensions** - Native clocks for kernel flags, process info, boot time

## Architecture Support

- x86_64: TSC (rdtsc) with mult/shift conversion
- aarch64: CNTVCT_EL0 (ARM generic timer)
- riscv64: `time` CSR / CLINT mtime

## Integration with SKYIOUS KERNEL

The kernel provides:
- Per-CPU vDSO pages at `VDSO_BASE + cpu_id * 4096`
- Seqlock-protected time/PID/CPU fields
- TSC frequency calibration at boot
- TSC drift compensation via `tsc_offset`
- Kernel feature flags in `kernel_flags`
- Per-process info in `process_cgroup_id`, `process_namespace_flags`, `process_cap_effective`

## Testing

The test program demonstrates all vDSO primary ABI functions working without syscall overhead in the common case.

```bash
# In QEMU with Vahi kernel:
./vdso_libc_test
```
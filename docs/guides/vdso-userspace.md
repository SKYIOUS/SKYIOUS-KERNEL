# vDSO Userspace Integration Guide

## Overview

This guide describes how to integrate the SKYIOUS Kernel vDSO as the **primary
ABI** for time, PID, and CPU ID queries in userspace applications.

Unlike Linux where vDSO is an optional optimization and syscalls remain the
primary interface, SKYIOUS Kernel is designed so that **vDSO is the primary
ABI**. The provided `vdso_libc` implements all time/PID/CPU functions using
vDSO fast paths, with syscalls only as fallback when the vDSO page is
unavailable.

## Quick Start

### 1. Link Against vdso_libc

```bash
# Static linking (recommended for init/sash)
musl-gcc -static -Os -I/path/to/userspace/libc \
  -o myapp myapp.c /path/to/userspace/libc/vdso_libc.a
```

### 2. Include the Header

```c
#include "vdso_libc.h"
```

### 3. Use Standard Functions

The following functions use vDSO as primary ABI:

```c
int clock_gettime(int clock_id, struct timespec *tp);
int clock_getres(int clock_id, struct timespec *tp);
long getpid(void);
int getcpu(unsigned *cpu, unsigned *node);
uint64_t get_time_ns_tsc(void);  // SKYIOUS extension: TSC nanoseconds since boot
```

## Migration from Syscall-Only Code

### Before (syscall-only)

```c
#include <linux/unistd.h>
#include <sys/syscall.h>
#include <time.h>

long my_getpid() {
    return syscall(SYS_getpid);
}

int my_clock_gettime(clockid_t clk, struct timespec *ts) {
    return syscall(SYS_clock_gettime, clk, ts);
}
```

### After (vDSO-primary with vdso_libc)

```c
#include "vdso_libc.h"

long my_getpid() {
    return getpid();  // vDSO fast path, syscall fallback automatic
}

int my_clock_gettime(clockid_t clk, struct timespec *ts) {
    return clock_gettime(clk, ts);  // vDSO fast path, syscall fallback automatic
}
```

**No code changes needed** — just link against `vdso_libc.a` instead of
calling syscalls directly. The API is identical.

## Supported Clocks (vDSO Fast Path)

| Clock ID | Constant | Resolution | Notes |
|----------|----------|------------|-------|
| 0 | `CLOCK_REALTIME` | 1 ns | TSC + cached RTC |
| 1 | `CLOCK_MONOTONIC` | 1 ns | TSC-based |
| 5 | `CLOCK_REALTIME_COARSE` | 10 ms | Tick-based, no seqlock |
| 6 | `CLOCK_MONOTONIC_COARSE` | 10 ms | Tick-based, no seqlock |
| 7 | `CLOCK_BOOTTIME` | 1 ns | Same as MONOTONIC |
| 4 | `CLOCK_MONOTONIC_RAW` | 1 ns | Same as MONOTONIC |
| 102 | `CLOCK_SKYIOUS_BOOT_NS` | 1 ns | Raw TSC since boot |
| 103 | `CLOCK_SKYIOUS_KERNEL_FLAGS` | N/A | Feature bitfield |
| 104 | `CLOCK_SKYIOUS_PROCESS_INFO` | N/A | cgroup/ns/caps |
| 105 | `CLOCK_SKYIOUS_VDSO_VERSION` | N/A | ABI version |

Clocks 2, 3, 100, 101 use syscall fallback.

## Fallback Behavior

`vdso_libc` automatically handles vDSO unavailability:

1. **First access** — Kernel lazily maps vDSO page on first fault. Works transparently.
2. **Page unmapped** — If `mprotect`/`munmap` removes the page, `vdso_is_mapped()` detects it via `mincore()` and falls back to syscall.
3. **Migration** — If thread migrates between CPUs, `vdso_page_current()` detects CPU mismatch and falls back to syscall (or retries with new CPU's page).
4. **Logging** — First fallback per process logs once to kernel serial console: `[vdso_libc] <function>: vDSO unavailable, using syscall fallback`

### Forcing Fallback (Testing)

```c
// Force fallback by unmapping vDSO page
mprotect((void*)VDSO_BASE, 4096, PROT_NONE);
clock_gettime(CLOCK_MONOTONIC, &ts);  // Uses syscall
mprotect((void*)VDSO_BASE, 4096, PROT_READ);  // Restore
```

## Building vdso_libc

### Prerequisites

- `musl-gcc` (recommended) or `gcc -static` with appropriate sysroot
- Target: `x86_64-linux-musl` or `x86_64-linux-gnu`

### Build Commands

```bash
cd userspace/libc

# Build static archive
musl-gcc -static -Os -Wall -Wextra -c vdso_libc.c -o vdso_libc.o
ar rcs vdso_libc.a vdso_libc.o

# Build test program
musl-gcc -static -Os -Wall -Wextra -D VDSO_LIBC_TEST \
  -o vdso_libc_test vdso_libc_test.c vdso_libc.a
```

### Makefile Targets

```make
all: vdso_libc.a vdso_libc_test

vdso_libc.a: vdso_libc.o
	ar rcs $@ $<

vdso_libc.o: vdso_libc.c vdso_libc.h
	$(CC) $(CFLAGS) -c $< -o $@

vdso_libc_test: vdso_libc_test.c vdso_libc.a
	$(CC) $(CFLAGS) -o $@ $^
```

## Integration Checklist

- [ ] Link application against `vdso_libc.a`
- [ ] Verify `clock_gettime(CLOCK_MONOTONIC, ...)` returns valid time
- [ ] Verify `getpid()` returns correct PID
- [ ] Verify `getcpu(&cpu, NULL)` returns valid CPU ID
- [ ] Test fallback: `mprotect` vDSO page `PROT_NONE`, verify syscall path works
- [ ] Check kernel serial for fallback log message (should appear once)
- [ ] Benchmark: vDSO path should be <5ns p99 on real hardware (TCG: warn-only)

## Troubleshooting

### "vDSO unavailable, using syscall fallback" appears repeatedly

The log-once mechanism uses a process-local atomic flag. If you see repeated
logs, check that:
- The atomic store uses `__ATOMIC_RELEASE`
- The flag is not being reset across fork (child inherits parent's flag)

### Wrong time values

- Ensure TSC is calibrated (kernel prints `[VDSO] TSC freq from timer:` at boot)
- Check `tsc_freq_hz` in vDSO page is non-zero
- Verify seqlock retry loop is correctly implemented

### getcpu returns wrong CPU

- Validate CPU identity check: `vdso->cpu_id` must match `getcpu` syscall result
- If mismatch, retry with new CPU's page: `VDSO_BASE + new_cpu * 4096`

## Architecture Notes

### Per-CPU Pages

Each CPU has its own vDSO page to eliminate cache-line false sharing on timer
tick updates (100 Hz). Timer tick on CPU N updates only page N. Context switch
on CPU N updates PID/CPU on page N.

### Seqlock Protocol

Two independent seqlocks:
- `seq` protects time fields (monotonic/realtime sec/nsec)
- `pid_cpu_seq` protects pid and cpu_id

Writer uses Release ordering, reader uses Acquire. On x86_64 (TSO), this is
optimal.

### TSC Synchronization

TSC is synchronized across CPUs via IPI broadcast every timer tick. The
`tsc_base_origin` field preserves the original boot TSC base, so
`CLOCK_SKYIOUS_BOOT_NS` remains continuous across synchronizations.

## See Also

- `vdso.7` — Man page with full ABI specification
- `vdso.md` — Interface specification
- `vdso-design.md` — Design rationale
- `vdso-clocks.md` — Complete clock ID registry
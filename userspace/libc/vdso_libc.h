/*
 * vDSO libc wrapper - vDSO as PRIMARY ABI for SKYIOUS KERNEL
 *
 * This is a minimal libc wrapper that uses vDSO as the PRIMARY ABI.
 * Syscalls are ONLY used as fallback when vDSO is unavailable.
 *
 * Compile: gcc -static -nostdlib -nostartfiles -D VDSO_LIBC_TEST -o vdso_libc_test vdso_libc.c vdso_libc_test.c
 * Run in Vahi kernel: ./vdso_libc_test
 *
 * Design principle: vDSO is the PRIMARY ABI, syscalls are fallback only.
 * This is the opposite of Linux where syscalls are primary and vDSO is an optimization.
 */

#ifndef VDSO_LIBC_H
#define VDSO_LIBC_H

#include <stdint.h>

/* Type definitions for standalone compilation */
struct timespec {
    long tv_sec;
    long tv_nsec;
};

/* vDSO data structure (matches kernel's VdsoData) - internal, not exposed */

/* Architecture-specific timer source */
enum vdso_timer_source {
    VDSO_TIMER_TSC,      /* x86_64 TSC */
    VDSO_TIMER_CNTRVCT,  /* aarch64 CNTVCT_EL0 */
    VDSO_TIMER_RISCV,    /* riscv64 `time` CSR / CLINT mtime */
};

/* Fixed vDSO constants (from kernel) */
#define VDSO_BASE       0x00007FFFFFFC0000ULL
#define MAX_CPUS        8
#define PAGE_SIZE       4096

/* Clock IDs (Linux-compatible + SKYIOUS extensions) */
#define CLOCK_REALTIME              0
#define CLOCK_MONOTONIC             1
#define CLOCK_PROCESS_CPUTIME_ID    2
#define CLOCK_THREAD_CPUTIME_ID     3
#define CLOCK_MONOTONIC_RAW         4
#define CLOCK_REALTIME_COARSE       5
#define CLOCK_MONOTONIC_COARSE      6
#define CLOCK_BOOTTIME              7
#define CLOCK_REALTIME_ALARM        8
#define CLOCK_BOOTTIME_ALARM        9
#define CLOCK_SKYIOUS_THREAD_CPUTIME  100
#define CLOCK_SKYIOUS_SCHED_DEADLINE 101
#define CLOCK_SKYIOUS_BOOT_NS       102
#define CLOCK_SKYIOUS_KERNEL_FLAGS  103
#define CLOCK_SKYIOUS_PROCESS_INFO  104

/* Syscall numbers (x86_64 Linux-compatible) */
#define SYS_clock_gettime   228
#define SYS_getpid          39
#define SYS_getcpu          309
#define SYS_clock_getres    229

/* ============================================================
 * Function declarations (implementations in vdso_libc.c)
 * ============================================================ */

/* vDSO Primary ABI: clock_gettime */
int clock_gettime(int clock_id, struct timespec *tp);

/* vDSO Primary ABI: clock_getres */
int clock_getres(int clock_id, struct timespec *tp);

/* vDSO Primary ABI: getpid */
long getpid(void);

/* vDSO Primary ABI: getcpu */
int getcpu(unsigned *cpu, unsigned *node);

/* High-resolution TSC time (SKYIOUS extension) */
uint64_t get_time_ns_tsc(void);

#endif /* VDSO_LIBC_H */
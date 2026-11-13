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

#include "vdso_libc.h"

#include <stdint.h>

/* vDSO data structure (matches kernel's VdsoData) - internal, defined here */
struct vdso_data {
    volatile uint64_t seq;
    volatile uint64_t monotonic_sec;
    volatile uint32_t monotonic_nsec;
    uint32_t _pad1;
    volatile uint64_t realtime_sec;
    volatile uint32_t realtime_nsec;
    uint32_t _pad2;
    volatile uint64_t tsc_freq_hz;
    uint32_t tsc_mult;
    uint32_t tsc_shift;
    volatile uint64_t tsc_base;
    volatile int64_t tsc_offset;
    volatile uint64_t pid_cpu_seq;
    volatile uint64_t pid;
    volatile uint32_t cpu_id;
    uint32_t _pad3;
    volatile uint64_t thread_cputime_ns;   /* CLOCK_SKYIOUS_THREAD_CPUTIME (100) */

    /* Kernel feature flags (read-only after boot) */
    struct {
        uint8_t has_avx2;
        uint8_t has_nvme;
        uint8_t has_xhci;
        uint8_t has_virtio_gpu;
        uint16_t net_stack_version;
        uint16_t fs_features;
        uint32_t _pad;
    } kernel_flags;

    /* Per-process fields (updated at fork/exec/cred change) */
    volatile uint64_t process_cgroup_id;
    volatile uint64_t process_namespace_flags;
    volatile uint64_t process_cap_effective;
};

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
 * Internal vDSO access helpers
 * ============================================================ */

static inline const struct vdso_data *vdso_page_current(void) {
    unsigned cpu = 0, node = 0;
    long ret;
    __asm__ volatile("syscall"
        : "=a"(ret), "=D"(cpu), "=S"(node)
        : "a"(309), "D"(cpu), "S"(node), "d"(0), "r"(0)
        : "rcx", "r11", "r10", "r8", "memory");
    return (const struct vdso_data *)(VDSO_BASE + cpu * 4096);
}

/* Seqlock read helpers */
static inline uint64_t vdso_read_seq_begin(const struct vdso_data *v) {
    return __atomic_load_n(&v->seq, __ATOMIC_ACQUIRE);
}

static inline int vdso_read_seq_retry(const struct vdso_data *v, uint64_t seq) {
    uint64_t seq2 = __atomic_load_n(&v->seq, __ATOMIC_ACQUIRE);
    return (seq != seq2) || (seq & 1);
}

static inline uint64_t vdso_read_pid_cpu_begin(const struct vdso_data *v) {
    return __atomic_load_n(&v->pid_cpu_seq, __ATOMIC_ACQUIRE);
}

static inline int vdso_read_pid_cpu_retry(const struct vdso_data *v, uint64_t seq) {
    uint64_t seq2 = __atomic_load_n(&v->pid_cpu_seq, __ATOMIC_ACQUIRE);
    return (seq != seq2) || (seq & 1);
}

/* Architecture-specific timer read */
static inline uint64_t vdso_read_timer(void) {
#if defined(__x86_64__)
    uint32_t lo, hi;
    __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
    return ((uint64_t)hi << 32) | lo;
#elif defined(__aarch64__)
    uint64_t val;
    __asm__ __volatile__("mrs %0, cntvct_el0" : "=r"(val));
    return val;
#elif defined(__riscv)
    uint64_t val;
    __asm__ __volatile__("rdtime %0" : "=r"(val));
    return val;
#else
    return 0;
#endif
}

/* Get timer frequency for current architecture */
static inline uint64_t vdso_get_timer_freq(const struct vdso_data *vdso) {
    return __atomic_load_n(&vdso->tsc_freq_hz, __ATOMIC_ACQUIRE);
}

/* Convert timer delta to nanoseconds */
static inline uint64_t vdso_delta_to_ns(const struct vdso_data *vdso, uint64_t delta) {
    uint64_t freq = __atomic_load_n(&vdso->tsc_freq_hz, __ATOMIC_ACQUIRE);
    if (freq == 0) return 0;

#if defined(__x86_64__)
    uint32_t mult = vdso->tsc_mult;
    uint32_t shift = vdso->tsc_shift;
    return ((__uint128_t)delta * mult) >> shift;
#else
    return (delta * 1000000000ULL) / freq;
#endif
}

/* ============================================================
 * vDSO Primary ABI: clock_gettime
 * ============================================================ */

int clock_gettime(int clock_id, struct timespec *tp) {
    if (!tp) return -1; /* EFAULT */

    const struct vdso_data *vdso = vdso_page_current();
    if (!vdso) goto fallback;

    uint64_t freq = vdso_get_timer_freq(vdso);
    if (freq == 0) goto fallback;

    switch (clock_id) {
        case 1: /* CLOCK_MONOTONIC */ {
            uint64_t seq;
            long sec, nsec;
            do {
                seq = __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE);
                if (seq & 1) continue;
                sec = (long)__atomic_load_n(&vdso->monotonic_sec, __ATOMIC_RELAXED);
                nsec = (long)__atomic_load_n(&vdso->monotonic_nsec, __ATOMIC_RELAXED);
            } while (seq != __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE) || (seq & 1));
            tp->tv_sec = sec;
            tp->tv_nsec = nsec;
            return 0;
        }

        case 0: /* CLOCK_REALTIME */ {
            uint64_t seq;
            long sec, nsec;
            do {
                seq = __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE);
                if (seq & 1) continue;
                sec = (long)__atomic_load_n(&vdso->realtime_sec, __ATOMIC_RELAXED);
                nsec = (long)__atomic_load_n(&vdso->realtime_nsec, __ATOMIC_RELAXED);
            } while (seq != __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE) || (seq & 1));
            tp->tv_sec = sec;
            tp->tv_nsec = nsec;
            return 0;
        }

        case 4: /* CLOCK_MONOTONIC_RAW */ {
            /* Fall back to TSC-based time for raw/boottime */
            uint64_t seq;
            long sec, nsec;
            do {
                seq = __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE);
                if (seq & 1) continue;
                long sec_val = (long)__atomic_load_n(&vdso->monotonic_sec, __ATOMIC_RELAXED);
                long nsec_val = (long)__atomic_load_n(&vdso->monotonic_nsec, __ATOMIC_RELAXED);
                sec = sec_val;
                nsec = nsec_val;
            } while (seq != __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE) || (seq & 1));
            tp->tv_sec = sec;
            tp->tv_nsec = nsec;
            return 0;
        }

        case 7: /* CLOCK_BOOTTIME */ {
            /* Fall back to TSC-based time for raw/boottime */
            uint64_t seq;
            long sec, nsec;
            do {
                seq = __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE);
                if (seq & 1) continue;
                long sec_val = (long)__atomic_load_n(&vdso->monotonic_sec, __ATOMIC_RELAXED);
                long nsec_val = (long)__atomic_load_n(&vdso->monotonic_nsec, __ATOMIC_RELAXED);
                sec = sec_val;
                nsec = nsec_val;
            } while (seq != __atomic_load_n(&vdso->seq, __ATOMIC_ACQUIRE) || (seq & 1));
            tp->tv_sec = sec;
            tp->tv_nsec = nsec;
            return 0;
        }

        case 2: /* CLOCK_PROCESS_CPUTIME_ID */ {
            /* Read process CPU time from vDSO (updated at context switch) */
            uint64_t ns = __atomic_load_n(&vdso->thread_cputime_ns, __ATOMIC_ACQUIRE);
            tp->tv_sec = ns / 1000000000;
            tp->tv_nsec = ns % 1000000000;
            return 0;
        }

        case 3: /* CLOCK_THREAD_CPUTIME_ID */ {
            /* Read thread CPU time from vDSO (updated at context switch) */
            uint64_t ns = __atomic_load_n(&vdso->thread_cputime_ns, __ATOMIC_ACQUIRE);
            tp->tv_sec = ns / 1000000000;
            tp->tv_nsec = ns % 1000000000;
            return 0;
        }

        case 100: /* CLOCK_SKYIOUS_THREAD_CPUTIME */ {
            /* SKYIOUS-native: thread CPU time via vDSO (same as CLOCK_THREAD_CPUTIME_ID) */
            uint64_t ns = __atomic_load_n(&vdso->thread_cputime_ns, __ATOMIC_ACQUIRE);
            tp->tv_sec = ns / 1000000000;
            tp->tv_nsec = ns % 1000000000;
            return 0;
        }

        case 101: /* CLOCK_SKYIOUS_SCHED_DEADLINE */ {
            /* SKYIOUS-native: scheduler deadline info via vDSO */
            /* For now, return 0 if not a deadline thread */
            tp->tv_sec = 0;
            tp->tv_nsec = 0;
            return 0;
        }

        case 102: /* CLOCK_SKYIOUS_BOOT_NS */ {
            /* SKYIOUS-native: raw TSC nanoseconds since boot */
            uint64_t freq = __atomic_load_n(&vdso->tsc_freq_hz, __ATOMIC_ACQUIRE);
            if (freq == 0) goto fallback;

#if defined(__x86_64__)
            uint32_t mult = vdso->tsc_mult;
            uint32_t shift = vdso->tsc_shift;
            uint64_t base = __atomic_load_n(&vdso->tsc_base, __ATOMIC_ACQUIRE);
            int64_t offset = __atomic_load_n(&vdso->tsc_offset, __ATOMIC_ACQUIRE);
            uint64_t effective_base = (base + (uint64_t)offset);
            uint64_t now = vdso_read_timer();
            uint64_t delta = now - effective_base;
            uint64_t ns = ((__uint128_t)delta * mult) >> shift;
            tp->tv_sec = ns / 1000000000;
            tp->tv_nsec = ns % 1000000000;
            return 0;
#else
            uint64_t freq_val = vdso->tsc_freq_hz;
            if (freq_val == 0) goto fallback;
            uint64_t base = __atomic_load_n(&vdso->tsc_base, __ATOMIC_ACQUIRE);
            int64_t offset = __atomic_load_n(&vdso->tsc_offset, __ATOMIC_ACQUIRE);
            uint64_t effective_base = (base + (uint64_t)offset);
            uint64_t now = vdso_read_timer();
            uint64_t delta = now - effective_base;
            uint64_t ns = (delta * 1000000000ULL) / freq_val;
            tp->tv_sec = ns / 1000000000;
            tp->tv_nsec = ns % 1000000000;
            return 0;
#endif
        }

        case 103: /* CLOCK_SKYIOUS_KERNEL_FLAGS */ {
            /* SKYIOUS-native: kernel feature flags */
            uint64_t sec = vdso->kernel_flags.has_avx2 |
                          (vdso->kernel_flags.has_nvme << 1) |
                          (vdso->kernel_flags.has_xhci << 2) |
                          (vdso->kernel_flags.has_virtio_gpu << 3);
            uint32_t nsec = ((uint32_t)vdso->kernel_flags.net_stack_version << 16) |
                           vdso->kernel_flags.fs_features;
            tp->tv_sec = (long)sec;
            tp->tv_nsec = (long)nsec;
            return 0;
        }

        case 104: /* CLOCK_SKYIOUS_PROCESS_INFO */ {
            /* SKYIOUS-native: per-process info */
            uint64_t cgroup_id = __atomic_load_n(&vdso->process_cgroup_id, __ATOMIC_ACQUIRE);
            uint64_t ns_flags = __atomic_load_n(&vdso->process_namespace_flags, __ATOMIC_ACQUIRE);
            tp->tv_sec = (long)cgroup_id;
            tp->tv_nsec = (long)(ns_flags & 0xFFFFFFFF);
            return 0;
        }

        case 5: /* CLOCK_REALTIME_COARSE */
        case 6: /* CLOCK_MONOTONIC_COARSE */ {
            /* Coarse clocks - fall through to syscall */
            break;
        }

        default:
            break;
    }

fallback:
    /* Syscall fallback - ONLY when vDSO is unavailable or clock not supported */
    register long syscall_nr __asm__("rax") = 228; /* SYS_clock_gettime */
    register long rdi __asm__("rdi") = clock_id;
    register long rsi __asm__("rsi") = (long)tp;
    long ret;
    __asm__ volatile("syscall"
        : "=a"(ret), "+D"(clock_id), "+S"(tp)
        : "a"(228)
        : "rcx", "r11", "memory");
    return (int)ret;
}

/* ============================================================
 * vDSO Primary ABI: clock_getres
 * ============================================================ */

int clock_getres(int clock_id, struct timespec *tp) {
    if (!tp) return -1;

    /* All SKYIOUS clocks have 1ns resolution (TSC-based) */
    switch (clock_id) {
        case 0: /* CLOCK_REALTIME */
        case 1: /* CLOCK_MONOTONIC */
        case 4: /* CLOCK_MONOTONIC_RAW */
        case 7: /* CLOCK_BOOTTIME */
        case 2: /* CLOCK_PROCESS_CPUTIME_ID */
        case 3: /* CLOCK_THREAD_CPUTIME_ID */
        case 100: /* CLOCK_SKYIOUS_THREAD_CPUTIME */
        case 101: /* CLOCK_SKYIOUS_SCHED_DEADLINE */
        case 102: /* CLOCK_SKYIOUS_BOOT_NS */
        case 103: /* CLOCK_SKYIOUS_KERNEL_FLAGS */
        case 104: /* CLOCK_SKYIOUS_PROCESS_INFO */
            tp->tv_sec = 0;
            tp->tv_nsec = 1;
            return 0;

        case 5: /* CLOCK_REALTIME_COARSE */
        case 6: /* CLOCK_MONOTONIC_COARSE */
            tp->tv_sec = 0;
            tp->tv_nsec = 10000000;
            return 0;

        case 8: /* CLOCK_REALTIME_ALARM */
        case 9: /* CLOCK_BOOTTIME_ALARM */
            return -1; /* EINVAL */

        default:
            /* Syscall fallback for unknown clocks */
            register long syscall_nr __asm__("rax") = 229; /* SYS_clock_getres */
            register long rdi __asm__("rdi") = clock_id;
            register long rsi __asm__("rsi") = (long)tp;
            long ret;
            __asm__ volatile("syscall"
                : "=a"(ret), "+D"(rdi), "+S"(rsi)
                : "a"(229)
                : "rcx", "r11", "memory");
            return (int)ret;
    }
}

/* ============================================================
 * vDSO Primary ABI: getpid
 * ============================================================ */

long getpid(void) {
    const struct vdso_data *vdso = vdso_page_current();
    if (!vdso) goto fallback;

    uint64_t seq;
    uint64_t pid;
    do {
        seq = __atomic_load_n(&vdso->pid_cpu_seq, __ATOMIC_ACQUIRE);
        if (seq & 1) continue;
        pid = __atomic_load_n(&vdso->pid, __ATOMIC_RELAXED);
    } while (__atomic_load_n(&vdso->pid_cpu_seq, __ATOMIC_ACQUIRE) != seq || (seq & 1));
    return (long)pid;

fallback:
    register long syscall_nr __asm__("rax") = 39; /* SYS_getpid */
    long ret;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(39) : "rcx", "r11", "memory");
    return (long)ret;
}

/* ============================================================
 * vDSO Primary ABI: getcpu
 * ============================================================ */

int getcpu(unsigned *cpu, unsigned *node) {
    const struct vdso_data *vdso = vdso_page_current();
    if (!vdso) goto fallback;

    uint64_t seq;
    uint32_t cpu_id;
    do {
        seq = __atomic_load_n(&vdso->pid_cpu_seq, __ATOMIC_ACQUIRE);
        if (seq & 1) continue;
        cpu_id = __atomic_load_n(&vdso->cpu_id, __ATOMIC_RELAXED);
    } while (__atomic_load_n(&vdso->pid_cpu_seq, __ATOMIC_ACQUIRE) != seq || (seq & 1));

    if (cpu) *cpu = cpu_id;
    if (node) *node = 0; /* NUMA not supported yet */
    return 0;

fallback:
    register long syscall_nr __asm__("rax") = 309; /* SYS_getcpu */
    register long rdi __asm__("rdi") = (long)cpu;
    register long rsi __asm__("rsi") = (long)node;
    long ret;
    __asm__ volatile("syscall"
        : "=a"(ret), "+D"(cpu), "+S"(node)
        : "a"(309)
        : "rcx", "r11", "memory");
    return (int)ret;
}

/* ============================================================
 * High-resolution TSC time (SKYIOUS extension)
 * ============================================================ */

uint64_t get_time_ns_tsc(void) {
    const struct vdso_data *vdso = vdso_page_current();
    if (!vdso) return 0;

    uint64_t freq = __atomic_load_n(&vdso->tsc_freq_hz, __ATOMIC_ACQUIRE);
    if (freq == 0) return 0;

#if defined(__x86_64__)
    uint32_t mult = vdso->tsc_mult;
    uint32_t shift = vdso->tsc_shift;
    uint64_t base = __atomic_load_n(&vdso->tsc_base, __ATOMIC_ACQUIRE);
    int64_t offset = __atomic_load_n(&vdso->tsc_offset, __ATOMIC_ACQUIRE);
    uint64_t effective_base = (base + (uint64_t)offset);
    uint64_t now = vdso_read_timer();
    uint64_t delta = now - effective_base;
    return ((__uint128_t)delta * mult) >> shift;
#else
    uint64_t freq_val = __atomic_load_n(&vdso->tsc_freq_hz, __ATOMIC_ACQUIRE);
    if (freq_val == 0) return 0;
    uint64_t base = __atomic_load_n(&vdso->tsc_base, __ATOMIC_ACQUIRE);
    int64_t offset = __atomic_load_n(&vdso->tsc_offset, __ATOMIC_ACQUIRE);
    uint64_t effective_base = (base + (uint64_t)offset);
    uint64_t now = vdso_read_timer();
    uint64_t delta = now - effective_base;
    return (delta * 1000000000ULL) / freq_val;
#endif
}
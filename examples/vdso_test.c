/*
 * vDSO userspace helper — minimal C implementation for reading vDSO data.
 *
 * Compile: gcc -nostdlib -static -o vdso_test vdso_test.c
 * Run in Vahi kernel: ./vdso_test
 *
 * Per-CPU vDSO pages: each CPU has its own page at VDSO_BASE + cpu_id * 4096
 * Userspace reads from the page corresponding to its current CPU (via getcpu).
 */

#include <stdint.h>
#include <stdio.h>

/* vDSO data structure (must match kernel's VdsoData) */
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
    volatile uint64_t pid;
    volatile uint32_t cpu_id;
    uint32_t _pad3;
};

/* Fixed base virtual address of vDSO pages (from kernel) */
#define VDSO_BASE 0x00007FFFFFC0000ULL

/* Max CPUs */
#define MAX_CPUS 8

/* Seqlock read: retry until consistent (even seq, no change during read) */
static inline uint64_t vdso_read_begin(const struct vdso_data *v) {
    return __atomic_load_n(&v->seq, __ATOMIC_ACQUIRE);
}

static inline int vdso_read_retry(const struct vdso_data *v, uint64_t seq) {
    uint64_t seq2 = __atomic_load_n(&v->seq, __ATOMIC_ACQUIRE);
    return (seq != seq2) || (seq & 1);
}

/* Get current CPU ID via syscall (for finding the right vDSO page) */
int getcpu_syscall(unsigned *cpu, unsigned *node) {
    register long r10 __asm__("r10") = 0;
    register long r8 __asm__("r8") = 0;
    register long syscall_nr __asm__("rax") = 309;
    long ret;
    __asm__ volatile("syscall"
        : "=a"(ret), "+D"(cpu), "+S"(node), "+d"(r10), "+r"(r8)
        : "a"(syscall_nr)
        : "rcx", "r11", "memory");
    return (int)ret;
}

/* Get pointer to vDSO page for a given CPU */
static const struct vdso_data *vdso_page_for_cpu(int cpu) {
    if (cpu < 0 || cpu >= MAX_CPUS)
        cpu = 0;
    return (const struct vdso_data *)(VDSO_BASE + cpu * 4096);
}

/* Get pointer to vDSO page for current CPU */
static const struct vdso_data *vdso_page_current(void) {
    unsigned cpu = 0, node = 0;
    getcpu_syscall(&cpu, &node);
    return vdso_page_for_cpu(cpu);
}

/* Seqlock read helper */
static inline int vdso_read_begin(const struct vdso_data *v, uint64_t *seq) {
    *seq = __atomic_load_n(&v->seq, __ATOMIC_ACQUIRE);
    return 0;
}

static inline int vdso_read_retry(const struct vdso_data *v, uint64_t seq) {
    uint64_t seq2 = __atomic_load_n(&v->seq, __ATOMIC_ACQUIRE);
    return (seq != seq2) || (seq & 1);
}

/* Read monotonic time from vDSO */
int clock_gettime_monotonic_vdso(long *sec, long *nsec) {
    const struct vdso_data *vdso = vdso_page_current();
    uint64_t seq;
    do {
        seq = vdso_read_begin(vdso);
        *sec = (long)__atomic_load_n(&vdso->monotonic_sec, __ATOMIC_RELAXED);
        *nsec = (long)__atomic_load_n(&vdso->monotonic_nsec, __ATOMIC_RELAXED);
    } while (vdso_read_retry(vdso, seq));
    return 0;
}

/* Read realtime from vDSO */
int clock_gettime_realtime_vdso(long *sec, long *nsec) {
    const struct vdso_data *vdso = vdso_page_current();
    uint64_t seq;
    do {
        seq = vdso_read_begin(vdso);
        *sec = (long)__atomic_load_n(&vdso->realtime_sec, __ATOMIC_RELAXED);
        *nsec = (long)__atomic_load_n(&vdso->realtime_nsec, __ATOMIC_RELAXED);
    } while (vdso_read_retry(vdso, seq));
    return 0;
}

/* Read PID from vDSO */
int getpid_vdso(void) {
    const struct vdso_data *vdso = vdso_page_current();
    uint64_t seq;
    uint64_t pid;
    do {
        seq = vdso_read_begin(vdso);
        pid = __atomic_load_n(&vdso->pid, __ATOMIC_RELAXED);
    } while (vdso_read_retry(vdso, seq));
    return (int)pid;
}

/* Read CPU ID from vDSO */
int getcpu_vdso(void) {
    const struct vdso_data *vdso = vdso_page_current();
    uint64_t seq;
    uint32_t cpu;
    do {
        seq = vdso_read_begin(vdso);
        cpu = __atomic_load_n(&vdso->cpu_id, __ATOMIC_RELAXED);
    } while (vdso_read_retry(vdso, seq));
    return (int)cpu;
}

/* Read TSC frequency */
uint64_t get_tsc_freq_vdso(void) {
    const struct vdso_data *vdso = vdso_page_current();
    return __atomic_load_n(&vdso->tsc_freq_hz, __ATOMIC_RELAXED);
}

/* High-resolution time via TSC (nanoseconds since boot) */
uint64_t get_time_ns_tsc(void) {
    const struct vdso_data *vdso = vdso_page_current();
    uint64_t freq = __atomic_load_n(&vdso->tsc_freq_hz, __ATOMIC_RELAXED);
    uint32_t mult = vdso->tsc_mult;
    uint32_t shift = vdso->tsc_shift;
    uint64_t base = __atomic_load_n(&vdso->tsc_base, __ATOMIC_RELAXED);

    if (freq == 0 || mult == 0)
        return 0;

    uint32_t lo, hi;
    __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
    uint64_t now = ((uint64_t)hi << 32) | lo;
    uint64_t elapsed = now - base;
    return (elapsed * mult) >> shift;
}

/* Test all CPU pages are consistent */
void test_all_cpu_pages(void) {
    printf("Testing all %d CPU pages...\n", MAX_CPUS);
    for (int cpu = 0; cpu < MAX_CPUS; cpu++) {
        const struct vdso_data *vdso = vdso_page_for_cpu(cpu);
        
        // Check cpu_id field
        uint32_t page_cpu = __atomic_load_n(&vdso->cpu_id, __ATOMIC_RELAXED);
        if (page_cpu != (uint32_t)cpu) {
            printf("  CPU %d page: cpu_id=%u (expected %d) MISMATCH\n", cpu, page_cpu, cpu);
        } else {
            printf("  CPU %d page: cpu_id=%u OK\n", cpu, page_cpu);
        }
        
        // Check time fields are readable
        uint64_t seq = vdso_read_begin(vdso);
        uint64_t sec = __atomic_load_n(&vdso->monotonic_sec, __ATOMIC_RELAXED);
        uint32_t nsec = __atomic_load_n(&vdso->monotonic_nsec, __ATOMIC_RELAXED);
        if (!vdso_read_retry(vdso, seq)) {
            printf("  CPU %d page: time=%lu.%09u\n", cpu, sec, nsec);
        } else {
            printf("  CPU %d page: time read retry\n", cpu);
        }
    }
}

int main(void) {
    long sec, nsec;
    int pid, cpu;

    printf("vDSO test starting...\n");
    printf("VDSO base address: 0x%llx\n", (unsigned long long)VDSO_BASE);
    
    unsigned cpu, node;
    getcpu_syscall(&cpu, &node);
    printf("Current CPU (via syscall): %u\n", cpu);
    printf("TSC freq: %llu Hz\n", (unsigned long long)get_tsc_freq_vdso());

    /* Test monotonic time */
    clock_gettime_monotonic_vdso(&sec, &nsec);
    printf("CLOCK_MONOTONIC: %ld.%09ld\n", sec, nsec);

    /* Test realtime */
    clock_gettime_realtime_vdso(&sec, &nsec);
    printf("CLOCK_REALTIME: %ld.%09ld\n", sec, nsec);

    /* Test PID */
    pid = getpid_vdso();
    printf("PID: %d\n", pid);

    /* Test CPU ID */
    cpu = getcpu_vdso();
    printf("CPU (via vDSO): %d\n", cpu);

    /* Test TSC time */
    uint64_t ns = get_time_ns_tsc();
    printf("TSC time (ns since boot): %llu\n", (unsigned long long)ns);

    /* Test all CPU pages */
    test_all_cpu_pages();

    /* Loop to show time advancing */
    for (int i = 0; i < 5; i++) {
        clock_gettime_monotonic_vdso(&sec, &nsec);
        printf("MONOTONIC: %ld.%09ld\n", sec, nsec);
        for (volatile int j = 0; j < 1000000; j++);
    }

    printf("vDSO test complete\n");
    return 0;
}
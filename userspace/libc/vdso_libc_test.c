/*
 * vDSO libc test program - demonstrates vDSO as PRIMARY ABI
 *
 * Compile: gcc -static -nostdlib -nostartfiles -T vdso_test.ld -o vdso_libc_test vdso_libc_test.c vdso_libc.c
 * Run in Vahi kernel: ./vdso_libc_test
 */

#include <stdint.h>
#include <stddef.h>
#include "vdso_libc.h"

/* Kernel serial output functions (provided by kernel at runtime) */
extern void vfs_serial_putc(char c) __attribute__((weak));
extern void vfs_serial_write(const char *str) __attribute__((weak));

/* Stub for standalone compilation - will be overridden by kernel at runtime */
void vfs_serial_putc(char c) {
    (void)c;
}
void vfs_serial_write(const char *str) {
    (void)str;
}

static void puts_custom(const char *s) {
    while (*s) vfs_serial_putc(*s++);
    vfs_serial_putc('\n');
}

/* Exit syscall - we need this to properly exit the test */
static void sys_exit(int code) {
    register long syscall_nr __asm__("rax") = 60; /* SYS_exit */
    register long rdi __asm__("rdi") = code;
    __asm__ volatile("syscall" : : "a"(60), "D"(code) : "rcx", "r11", "memory");
}

/* Main test function */
static int run_tests(void) {
    struct timespec ts;
    struct timespec res;
    uint64_t ns;
    long pid;
    unsigned cpu;

    puts_custom("=== vDSO as PRIMARY ABI Demo ===");

    /* clock_gettime (vDSO primary) */
    clock_gettime(1, &ts); /* CLOCK_MONOTONIC */
    puts_custom("CLOCK_MONOTONIC: done");

    clock_gettime(0, &ts); /* CLOCK_REALTIME */
    puts_custom("CLOCK_REALTIME: done");

    clock_gettime(102, &ts); /* CLOCK_SKYIOUS_BOOT_NS */
    puts_custom("CLOCK_SKYIOUS_BOOT_NS: done");

    clock_gettime(103, &ts); /* CLOCK_SKYIOUS_KERNEL_FLAGS */
    puts_custom("CLOCK_SKYIOUS_KERNEL_FLAGS: done");

    clock_gettime(104, &ts); /* CLOCK_SKYIOUS_PROCESS_INFO */
    puts_custom("CLOCK_SKYIOUS_PROCESS_INFO: done");

    /* getpid (vDSO primary) */
    pid = getpid();
    puts_custom("getpid: done");

    /* getcpu (vDSO primary) */
    cpu = 0;
    getcpu(&cpu, NULL);
    puts_custom("getcpu: done");

    /* clock_getres (vDSO primary) */
    clock_getres(1, &res); /* CLOCK_MONOTONIC */
    puts_custom("clock_getres(CLOCK_MONOTONIC): done");
    clock_getres(102, &res); /* CLOCK_SKYIOUS_BOOT_NS */
    puts_custom("clock_getres(SKYIOUS_BOOT_NS): done");

    /* High-res TSC time */
    ns = get_time_ns_tsc();
    puts_custom("TSC time (ns since boot): done");

    puts_custom("All operations used vDSO as PRIMARY ABI!");
    return 0;
}

/* Entry point - minimal startup code - placed in .text._start section */
void _start(void) __attribute__((section(".text._start")));
void _start(void) {
    int ret = run_tests();
    sys_exit(ret);
}
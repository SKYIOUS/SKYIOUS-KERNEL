/*
 * vDSO libc test program - demonstrates vDSO as PRIMARY ABI
 *
 * Compile: gcc -static -nostdlib -nostartfiles -T vdso_test.ld -o vdso_libc_test vdso_libc_test.c vdso_libc.c
 * Run in Vahi kernel: ./vdso_libc_test
 */

#include <stdint.h>
#include <stddef.h>
#include "vdso_libc.h"

/* Kernel serial output syscall */
static void vfs_serial_write(const char *str) {
    size_t len = 0;
    while (str[len]) len++;
    register long syscall_nr __asm__("rax") = 400; /* SYS_serial_write */
    register long rdi __asm__("rdi") = (long)str;
    register long rsi __asm__("rsi") = (long)len;
    __asm__ volatile("syscall" : : "a"(400), "D"(str), "S"(len) : "rcx", "r11", "memory");
}

static void vfs_serial_putc(char c) {
    char buf[2] = {c, 0};
    vfs_serial_write(buf);
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
    int ret;

    puts_custom("=== vDSO as PRIMARY ABI Demo ===");

    /* clock_gettime (vDSO primary) */
    ret = clock_gettime(1, &ts); /* CLOCK_MONOTONIC */
    if (ret != 0 || ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1000000000) return -1;
    puts_custom("CLOCK_MONOTONIC: done");

    ret = clock_gettime(0, &ts); /* CLOCK_REALTIME */
    if (ret != 0 || ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1000000000) return -1;
    puts_custom("CLOCK_REALTIME: done");

    ret = clock_gettime(102, &ts); /* CLOCK_SKYIOUS_BOOT_NS */
    if (ret != 0 || ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1000000000) return -1;
    puts_custom("CLOCK_SKYIOUS_BOOT_NS: done");

    ret = clock_gettime(103, &ts); /* CLOCK_SKYIOUS_KERNEL_FLAGS */
    if (ret != 0) return -1;
    puts_custom("CLOCK_SKYIOUS_KERNEL_FLAGS: done");

    ret = clock_gettime(104, &ts); /* CLOCK_SKYIOUS_PROCESS_INFO */
    if (ret != 0) return -1;
    puts_custom("CLOCK_SKYIOUS_PROCESS_INFO: done");

    /* getpid (vDSO primary) */
    pid = getpid();
    if (pid <= 0) return -1;
    puts_custom("getpid: done");

    /* getcpu (vDSO primary) */
    cpu = 0;
    ret = getcpu(&cpu, NULL);
    if (ret != 0 || cpu >= 8) return -1;
    puts_custom("getcpu: done");

    /* clock_getres (vDSO primary) */
    ret = clock_getres(1, &res); /* CLOCK_MONOTONIC */
    if (ret != 0 || res.tv_sec != 0 || res.tv_nsec != 1) return -1;
    puts_custom("clock_getres(CLOCK_MONOTONIC): done");
    ret = clock_getres(102, &res); /* CLOCK_SKYIOUS_BOOT_NS */
    if (ret != 0 || res.tv_sec != 0 || res.tv_nsec != 1) return -1;
    puts_custom("clock_getres(SKYIOUS_BOOT_NS): done");

    /* High-res TSC time */
    ns = get_time_ns_tsc();
    if (ns == 0) return -1;
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
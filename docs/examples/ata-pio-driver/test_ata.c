// ── ATA driver self-test — runs ata_selftest(), outputs via Bochs E9 port ──
// Build as a flat binary loaded at 1 MiB, runnable via qemu -kernel.
// ponytail: E9 port for output avoids full VGA/console setup.
#include "ata.h"

static void e9_putc(char c) {
    __asm__ __volatile__("out %0, $0xE9" : : "a"(c));
}
static void e9_puts(const char *s) {
    while (*s) e9_putc(*s++);
}
static void e9_puthex(uint32_t n) {
    for (int i = 28; i >= 0; i -= 4) {
        uint8_t d = (n >> i) & 0xF;
        e9_putc(d < 10 ? '0' + d : 'A' + d - 10);
    }
}

void _start(void) {
    e9_puts("ATA selftest...\n");
    int r = ata_selftest();
    if (r == 0) {
        e9_puts("PASS\n");
    } else {
        e9_puts("FAIL rc=");
        e9_puthex(r);
        e9_putc('\n');
    }
    // ponytail: QEMU shutdown via 0x604 (acpi) or 0x8900 (isa); 0x2000 is pm1a
    __asm__ __volatile__("outl %0, %1" : : "a"(0x2000), "d"((uint16_t)0x604));
    for (;;) __asm__ __volatile__("hlt");
}

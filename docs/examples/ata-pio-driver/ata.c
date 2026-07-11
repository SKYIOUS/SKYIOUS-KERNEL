// ── ATA PIO driver: init, read/write sector, IRQ handling, retry ──
//
// Ports for primary (0x1F0-0x1F7) and secondary (0x170-0x177) channels.
// ponytail: PIO only, no DMA; add if throughput matters.

#include "ata.h"
#include <stddef.h>

// ── I/O ports ──
#define REG_DATA(ch)   ((ch) ? 0x170 : 0x1F0)
#define REG_ERROR(ch)  ((ch) ? 0x171 : 0x1F1)
#define REG_COUNT(ch)  ((ch) ? 0x172 : 0x1F2)
#define REG_LBA_LO(ch) ((ch) ? 0x173 : 0x1F3)
#define REG_LBA_MD(ch) ((ch) ? 0x174 : 0x1F4)
#define REG_LBA_HI(ch) ((ch) ? 0x175 : 0x1F5)
#define REG_DRIVE(ch)  ((ch) ? 0x176 : 0x1F6)
#define REG_CMD(ch)    ((ch) ? 0x177 : 0x1F7)
#define REG_STAT(ch)   ((ch) ? 0x177 : 0x1F7)

// ── ATA commands ──
#define CMD_READ       0x20
#define CMD_READ_EXT   0x24    // 48-bit LBA
#define CMD_WRITE      0x30
#define CMD_WRITE_EXT  0x34
#define CMD_FLUSH      0xE7
#define CMD_IDENTIFY   0xEC
#define CMD_SETFEAT    0xEF

// ── Status bits ──
#define STAT_ERR       0x01
#define STAT_IDX       0x02
#define STAT_CORR      0x04
#define STAT_DRQ       0x08
#define STAT_SKC       0x10
#define STAT_SRV       0x10    // overlap mode
#define STAT_DF        0x20
#define STAT_RDY       0x40
#define STAT_BSY       0x80

// ── Retry constants ──
#define MAX_RETRIES    3
#define TIMEOUT_BUSY   10000000
#define TIMEOUT_DRQ    10000000
#define TIMEOUT_IRQ    50000000

// ── internal state ──
static int      drv_present = 0;        // bit 0 = primary master, bit 1 = primary slave, …
static volatile int irq_received = 0;

// ── inline I/O helpers (replace with your kernel's in/out) ──
static inline uint8_t  inb(uint16_t port) {
    uint8_t v;
    __asm__ __volatile__("in %1, %0" : "=a"(v) : "d"(port));
    return v;
}
static inline void outb(uint16_t port, uint8_t val) {
    __asm__ __volatile__("out %0, %1" : : "a"(val), "d"(port));
}
static inline uint16_t inw(uint16_t port) {
    uint16_t v;
    __asm__ __volatile__("in %1, %0" : "=a"(v) : "d"(port));
    return v;
}
static inline void outw(uint16_t port, uint16_t val) {
    __asm__ __volatile__("out %0, %1" : : "a"(val), "d"(port));
}

// ── poll until BSY clears or timeout ──
static int wait_busy(int ch) {
    for (int i = 0; i < TIMEOUT_BUSY; i++) {
        if (!(inb(REG_STAT(ch)) & STAT_BSY)) return 0;
    }
    return -1;  // timeout
}

// ── poll until DRQ or ERR ──
static int wait_drq(int ch) {
    for (int i = 0; i < TIMEOUT_DRQ; i++) {
        uint8_t s = inb(REG_STAT(ch));
        if (s & STAT_DRQ) return 0;
        if (s & STAT_ERR) return -1;
    }
    return -1;
}

// ── probe a single drive ──
static int probe_drive(int ch, int slave) {
    outb(REG_DRIVE(ch), 0xA0 | (slave << 4));  // select drive
    // ponytail: delay loop; a real driver reads the alternate status port
    for (volatile int d = 0; d < 4; d++) inb(REG_STAT(ch));

    outb(REG_COUNT(ch), 0);
    outb(REG_LBA_LO(ch), 0);
    outb(REG_LBA_MD(ch), 0);
    outb(REG_LBA_HI(ch), 0);
    outb(REG_CMD(ch), CMD_IDENTIFY);

    uint8_t s = inb(REG_STAT(ch));
    if (!s) return 0;               // no drive

    if (wait_busy(ch)) return 0;

    // Check signature
    uint16_t sig_lo = inw(REG_DATA(ch));       // should be 0x0001
    uint16_t sig_hi = inw(REG_DATA(ch));       // should be 0x0000  (ATA)
    if (sig_lo != 0x0001 || sig_hi != 0x0000) return 0;

    // Read identify data (256 words)
    if (wait_drq(ch)) return 0;
    uint16_t id_buf[256];
    for (int i = 0; i < 256; i++) id_buf[i] = inw(REG_DATA(ch));

    // bit 15 of word 0 = ATAPI (0) / ATA (1)  →  actually word 0 bits are device type
    uint8_t type = (id_buf[0] >> 8) & 0x1F;
    if (type != 0x00 && type != 0x14) return 0;  // only ATA direct-access

    return 1;  // drive present
}

// ── public: init ──
int ata_init(void) {
    drv_present = 0;
    for (int ch = 0; ch < 2; ch++) {
        for (int slave = 0; slave < 2; slave++) {
            if (probe_drive(ch, slave)) {
                drv_present |= (1 << (ch * 2 + slave));
            }
        }
    }
    return drv_present ? 0 : -1;
}

// ── internal read/write with retry ──
static int ata_rw(uint32_t lba, void *buf, int is_write) {
    int ch = 0;         // ponytail: always primary channel; add channel selection from partition info
    if (!(drv_present & 0x01)) return -1;     // no primary master

    for (int retry = 0; retry < MAX_RETRIES; retry++) {
        irq_received = 0;

        if (wait_busy(ch)) continue;

        // Select drive + LBA
        outb(REG_DRIVE(ch), 0xE0 | ((lba >> 24) & 0x0F));  // LBA mode, master
        outb(REG_COUNT(ch), 1);                              // sector count = 1
        outb(REG_LBA_LO(ch), (uint8_t)(lba));
        outb(REG_LBA_MD(ch), (uint8_t)(lba >> 8));
        outb(REG_LBA_HI(ch), (uint8_t)(lba >> 16));

        // Issue command
        outb(REG_CMD(ch), is_write ? CMD_WRITE : CMD_READ);

        if (is_write) {
            // Wait for DRQ before writing data
            if (wait_drq(ch)) continue;
            uint16_t *w = (uint16_t *)buf;
            for (int i = 0; i < 256; i++) outw(REG_DATA(ch), w[i]);
            // Flush write cache
            outb(REG_CMD(ch), CMD_FLUSH);
            if (wait_busy(ch)) continue;
        } else {
            // Wait for IRQ or poll
            // ponytail: poll + IRQ; real driver uses IRQ to avoid busy-wait
            for (int i = 0; i < TIMEOUT_IRQ; i++) {
                uint8_t s = inb(REG_STAT(ch));
                if (s & STAT_ERR) break;
                if (s & STAT_DRQ) {
                    uint16_t *r = (uint16_t *)buf;
                    for (int j = 0; j < 256; j++) r[j] = inw(REG_DATA(ch));
                    // Read trailing status to ack IRQ
                    inb(REG_STAT(ch));
                    return 0;
                }
            }
            continue;   // timeout → retry
        }
        return 0;
    }
    return -1;
}

// ── public ──
int ata_read_sector(uint32_t lba, void *buf) {
    return ata_rw(lba, buf, 0);
}

int ata_write_sector(uint32_t lba, const void *buf) {
    return ata_rw(lba, (void *)buf, 1);
}

// ── IRQ handler (called from interrupt stub) ──
void ata_irq_handler(void) {
    irq_received = 1;
    // Read status to clear the interrupt
    // (the read/write functions already consume status; this is a fallback)
    (void)inb(REG_STAT(0));
}

// ── self-test (intended for kernel init, not userspace) ──
// Writes a pattern to sector 0, reads it back, compares.
int ata_selftest(void) {
    if (ata_init()) return -1;

    uint16_t pattern[256];
    uint16_t readback[256];

    for (int i = 0; i < 256; i++) pattern[i] = (uint16_t)(0xABCD + i);

    if (ata_write_sector(0, pattern)) return -1;
    if (ata_read_sector(0, readback)) return -1;

    for (int i = 0; i < 256; i++) {
        // ponytail: simple incrementing pattern; checksums would catch more
        if (readback[i] != (uint16_t)(0xABCD + i)) return -1;
    }
    return 0;  // pass
}

#ifndef ATA_H
#define ATA_H

#include <stdint.h>

int  ata_init(void);                              // probe both channels, autodetect drive
int  ata_read_sector(uint32_t lba, void *buf);    // 1 sector = 512 bytes
int  ata_write_sector(uint32_t lba, const void *buf);
void ata_irq_handler(void);                       // call from IRQ 14 (primary) or 15 (secondary)
int  ata_selftest(void);                          // write pattern → read back → compare

#endif

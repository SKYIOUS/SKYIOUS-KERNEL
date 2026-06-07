# x86 I/O Port Map

Reference of I/O ports used by SkyOS kernel drivers.

## Port Map

| Port Range | Device | Description |
|------------|--------|-------------|
| 0x0000-0x001F | DMA Controller 1 | DMA channel 0-3 |
| 0x0020-0x0021 | PIC 1 | Master PIC (if legacy mode) |
| 0x0040-0x0043 | PIT | Programmable Interval Timer |
| 0x0060 | PS/2 | Keyboard data / mouse data |
| 0x0064 | PS/2 | Keyboard status / command |
| 0x0070-0x0071 | CMOS/RTC | CMOS memory and RTC |
| 0x0080-0x008F | DMA Page Registers | DMA page addresses |
| 0x00A0-0x00A1 | PIC 2 | Slave PIC |
| 0x00CF8 | PCI | PCI config address register |
| 0x00CFC | PCI | PCI config data register |
| 0x03F8-0x03FF | COM1 | Serial port (debug console) |
| 0x02F8-0x02FF | COM2 | Serial port |
| 0x03E8-0x03EF | COM3 | Serial port |
| 0x02E8-0x02EF | COM4 | Serial port |

## Serial Port (COM1) Registers

| Offset | DLAB | Register |
|--------|------|----------|
| +0 | 0 | Data register (read/write) |
| +0 | 1 | Divisor latch low byte |
| +1 | 0 | Interrupt enable register |
| +1 | 1 | Divisor latch high byte |
| +2 | - | Interrupt identification register |
| +3 | - | Line control register |
| +4 | - | Modem control register |
| +5 | - | Line status register |
| +6 | - | Modem status register |

## PIT Registers

| Port | Purpose |
|------|---------|
| 0x40 | Counter 0 data (system timer) |
| 0x41 | Counter 1 data (refresh) |
| 0x42 | Counter 2 data (speaker) |
| 0x43 | Mode/command register |

## CMOS/RTC Registers

| Port | Purpose |
|------|---------|
| 0x70 | CMOS index (bit 7 = NMI disable) |
| 0x71 | CMOS data |
| 0x72 | CMOS extended index |
| 0x73 | CMOS extended data |

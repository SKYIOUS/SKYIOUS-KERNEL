# IRQ Vector Assignments

This page documents the interrupt vector assignments used by SkyOS.

## Vector Ranges

| Range | Purpose | Count |
|-------|---------|-------|
| 0-31 | CPU exceptions (x86_64 reserved) | 32 |
| 32-47 | ISA/PCI IRQs (remapped from PIC) | 16 |
| 48-191 | Kernel use (device interrupts) | 144 |
| 192-255 | Software interrupts / IPIs / spurious | 64 |

## CPU Exception Vectors

| Vector | Name | Type | Error Code |
|--------|------|------|------------|
| 0 | Divide-by-zero | Fault | No |
| 1 | Debug | Trap/Fault | No |
| 2 | NMI | Interrupt | No |
| 3 | Breakpoint | Trap | No |
| 4 | Overflow | Trap | No |
| 5 | Bound range | Fault | No |
| 6 | Invalid opcode | Fault | No |
| 7 | Device not available | Fault | No |
| 8 | Double fault | Abort | Yes (0) |
| 10 | Invalid TSS | Fault | Yes |
| 11 | Segment not present | Fault | Yes |
| 12 | Stack-segment fault | Fault | Yes |
| 13 | General protection | Fault | Yes |
| 14 | Page fault | Fault | Yes |
| 16 | x87 FPU error | Fault | No |
| 17 | Alignment check | Fault | Yes |
| 18 | Machine check | Abort | No |
| 19 | SIMD floating-point | Fault | No |
| 20 | Virtualization | Fault | No |
| 30 | Security | Trap | Yes |

## IRQ to Vector Mapping

| IRQ | Vector | Device |
|-----|--------|--------|
| IRQ 0 | 32 | PIT Timer |
| IRQ 1 | 33 | PS/2 Keyboard |
| IRQ 2 | 34 | Cascade (slave PIC) |
| IRQ 3 | 35 | COM2 |
| IRQ 4 | 36 | COM1 |
| IRQ 5 | 37 | LPT2 / Sound |
| IRQ 6 | 38 | Floppy controller |
| IRQ 7 | 39 | LPT1 / Spurious |
| IRQ 8 | 40 | RTC |
| IRQ 9 | 41 | ACPI / SCI |
| IRQ 10 | 42 | PCI (available) |
| IRQ 11 | 43 | PCI (available) |
| IRQ 12 | 44 | PS/2 Mouse |
| IRQ 13 | 45 | FPU |
| IRQ 14 | 46 | Primary ATA |
| IRQ 15 | 47 | Secondary ATA |

## IPI Vectors

| Vector | Purpose |
|--------|---------|
| 251 | TLB shootdown |
| 252 | Reschedule |
| 253 | Call function |
| 254 | Halt |
| 255 | Spurious interrupt |

## APIC Timer Vector

The local APIC timer uses vector 48 by default (configurable via the APIC timer LVT entry).

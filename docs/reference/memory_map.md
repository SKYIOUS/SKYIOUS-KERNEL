# Kernel Memory Layout Map

This page documents the virtual memory layout used by SkyOS.

## Virtual Address Space Layout

On x86_64 with 4-level paging, the virtual address space is split:
- **User space**: 0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF (128 TiB)
- **Kernel space**: 0xFFFF_8000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF (128 TiB)

## Kernel Layout (x86_64)

| Address Range | Size | Description |
|---------------|------|-------------|
| 0xFFFF_8000_0000_0000 | 1 GiB | Kernel text (code, rodata) |
| 0xFFFF_8000_4000_0000 | 1 GiB | Kernel data (bss, heap) |
| 0xFFFF_8000_8000_0000 | 1 GiB | Kernel heap (buddy allocator) |
| 0xFFFF_8000_C000_0000 | 1 GiB | Kernel stack (per-CPU) |
| 0xFFFF_8800_0000_0000 | 32 TiB | Physical memory direct map |
| 0xFFFF_8880_0000_0000 | 512 GiB | Page table pool |
| 0xFFFF_8900_0000_0000 | 512 GiB | MMIO regions |
| 0xFFFF_8A00_0000_0000 | 512 GiB | Frame buffer mapping |
| 0xFFFF_FFFF_8000_0000 | 2 MiB | Recursive page table |
| 0xFFFF_FFFF_8020_0000 | 2 MiB | APIC (local and I/O) |
| 0xFFFF_FFFF_8040_0000 | 1 MiB | ACPI tables |
| 0xFFFF_FFFF_8050_0000 | 1 MiB | HPET |

## Physical Memory Direct Map

The direct map (`0xFFFF_8800_0000_0000`) maps all physical memory 1:1. Physical address `P` maps to `0xFFFF_8800_0000_0000 + P`. This allows the kernel to access any physical page by adding the direct map offset.

## Stack Layout (x86_64)

| Address | Description |
|---------|-------------|
| Kernel stack top | RSP on kernel entry |
| Kernel stack bottom | -16 KiB from top |
| Interrupt stack 1 | Double fault handler (8 KiB) |
| Interrupt stack 2 | NMI handler (8 KiB) |
| Interrupt stack 3 | General interrupt (8 KiB) |

## User Space Layout

| Address Range | Size | Description |
|---------------|------|-------------|
| 0x0000_0000_0000_0000 | 64 KiB | Unmapped (null pointer guard) |
| 0x0000_0000_0001_0000 | (varies) | Program text (code) |
| ... | | Program data (rodata, data, bss) |
| 0x0000_0000_0040_0000 | 2 MiB | Heap (grows up) |
| 0x0000_7FFF_XXXX_0000 | 8 MiB | Stack (grows down) |
| 0x0000_7FFF_FFFF_F000 | 4 KiB | vDSO page |

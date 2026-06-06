# Skyious Kernel Memory Map

This document defines the virtual address space layout for the Skyious kernel.

## Virtual Address Regions

| Start Address           | End Address             | Size      | Description                |
|-------------------------|-------------------------|-----------|----------------------------|
| `0xFFFF_8000_0000_0000` | `0xFFFF_8FFF_FFFF_FFFF` | 1 TB      | Physical Memory Mapping    |
| `0xFFFF_C000_0000_0000` | `0xFFFF_C000_007F_FFFF` | 8 MB      | Kernel Heap (Planned)      |
| `0xFFFF_D000_0000_0000` | `0xFFFF_DFFF_FFFF_FFFF` | 1 TB      | Kernel Stacks (Planned)    |
| `0xFFFF_E000_0000_0000` | `0xFFFF_EFFF_FFFF_FFFF` | 1 TB      | VMALLOC Region (Planned)   |

## Current Implementation Details

- **Physical Memory Mapping**: Currently uses the offset provided by the bootloader at initialization. This is stored in `PHYSICAL_MEMORY_OFFSET`.
- **Kernel Heap**: 
    - **Current**: `0xFFFF_C000_0000_0000` (8 MB)
- **Dynamic Allocations**: Handled by the Slab Allocator (for small objects) and the Buddy Allocator (for physical frames).

## Design Goals

1. **Higher-Half Kernel**: Map the kernel and all its data structures in the upper half of the virtual address space (canonical addresses starting with `0xFFFF...`).
2. **Standardization**: Follow established patterns for region naming and placement to ease future migrations (e.g., to Limine).
3. **Protection**: Implement guard pages between kernel stacks (Phase I3).

# Memory Management Architecture

SkyOS implements a multi-layered memory management system built on x86_64 hardware paging.

## Paging

The kernel uses 4-level page tables (PML4, PDPT, PD, PT) with 4 KiB page sizes for general allocations and 2 MiB huge pages for large contiguous regions. The page table manager handles all mappings, including recursive page table access for efficient TLB management.

```rust
pub struct PageTable {
    level4: PhysFrame,
    mapper: Mapper,
    flush_tlb: bool,
}
```

## Physical Memory Manager

A bitmap-based frame allocator tracks physical page usage. The kernel maintains a statically allocated bitmap at a known physical address, with one bit per 4 KiB page. On systems with 4 GiB of RAM, this requires 128 KiB of bitmap storage.

## Virtual Memory Allocator

Virtual memory is managed through a slab allocator for small allocations (8-2048 bytes) and a region-based allocator for larger mappings. The slab allocator is NUMA-aware and uses per-CPU caches to reduce contention.

## Kernel Heap

The kernel heap uses a buddy allocator backed by mapped physical frames. The heap starts at a fixed virtual address `KERNEL_HEAP_BASE` and can grow dynamically through `brk()`-style expansion. Allocations are aligned to 16 bytes by default.

## Userspace Memory

Each process has a dedicated address space with:
- A fixed-size stack region (default 8 MiB)
- Heap region managed through `mmap()`/`munmap()`
- Code and data segments loaded by the ELF loader
- Guard pages to detect stack overflow

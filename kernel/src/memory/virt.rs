#![allow(dead_code)]

use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        OffsetPageTable, PageTableFlags as Flags,
        PhysFrame, Size4KiB, Page, Mapper, FrameAllocator,
    },
};

/// The kernel's higher-half virtual base.
pub const KERNEL_BASE: u64 = 0xFFFF_FFFF_8000_0000;

/// Kernel heap virtual start.
pub const HEAP_START: u64 = 0xFFFF_C000_0000_0000;
pub const HEAP_SIZE: u64 = 128 * 1024 * 1024; // 128 MiB

/// Identity-map a physical range in the active page table.
///
/// # Safety
/// Caller must ensure `mapper` is the active page table and the range
/// does not overlap with already-mapped regions in conflicting ways.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn identity_map(
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    phys_start: PhysAddr,
    size: u64,
    flags: Flags,
) {
    let start_page: Page = Page::containing_address(VirtAddr::new(phys_start.as_u64()));
    let end_page: Page = Page::containing_address(VirtAddr::new(phys_start.as_u64() + size - 1));
    for page in Page::range(start_page, end_page + 1) {
        let frame = PhysFrame::containing_address(PhysAddr::new(page.start_address().as_u64()));
        mapper.map_to(page, frame, flags, frame_allocator)
            .expect("identity_map failed")
            .ignore();
    }
}

/// Map a range of virtual pages to contiguous physical frames.
///
/// # Safety
/// Caller must ensure the virtual range is unused and physical frames are available.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn map_contiguous(
    mapper: &mut OffsetPageTable,
    virt_start: VirtAddr,
    phys_start: PhysAddr,
    page_count: u64,
    flags: Flags,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let start_page: Page = Page::containing_address(virt_start);
    for i in 0..page_count {
        let page = start_page + i;
        let frame = PhysFrame::containing_address(PhysAddr::new(phys_start.as_u64() + i * 4096));
        mapper.map_to(page, frame, flags, frame_allocator)
            .expect("map_contiguous failed")
            .ignore();
    }
}

/// Map the kernel heap region by allocating fresh frames.
///
/// # Safety
/// Must only be called once during boot, before the heap allocator is initialized.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn map_heap(
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    use x86_64::structures::paging::PageTableFlags;
    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + HEAP_SIZE - 1u64;
    let start_page: Page = Page::containing_address(heap_start);
    let end_page: Page = Page::containing_address(heap_end);

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE | PageTableFlags::GLOBAL;

    for page in Page::range(start_page, end_page + 1) {
        let frame = frame_allocator
            .allocate_frame()
            .expect("map_heap: out of memory");
        mapper.map_to(page, frame, flags, frame_allocator)
            .expect("map_heap: map_to failed")
            .ignore();
    }
}

/// Map a device MMIO region into the virtual address space using the physical memory offset.
///
/// Returns the virtual address where the device memory is mapped.
/// For most x86_64 kernels using a direct physical map, this is simply
/// `physical_memory_offset + phys_addr`.
pub fn map_device(phys_addr: PhysAddr, _size: u64) -> VirtAddr {
    let offset = crate::memory::PHYSICAL_MEMORY_OFFSET.get()
        .expect("physical memory offset not initialized");
    VirtAddr::new(*offset) + phys_addr.as_u64()
}

// ponytail: quick sanity test for page alignment invariants
pub fn test_page_constants() -> Result<(), &'static str> {
    if HEAP_START & 0xFFF != 0 {
        return Err("HEAP_START not page-aligned");
    }
    if KERNEL_BASE & 0xFFF != 0 {
        return Err("KERNEL_BASE not page-aligned");
    }
    if HEAP_SIZE % 4096 != 0 {
        return Err("HEAP_SIZE not multiple of page size");
    }
    Ok(())
}

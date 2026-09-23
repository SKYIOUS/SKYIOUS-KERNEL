use crate::memory::slab::{FixedSizeBlockAllocator, Locked};
use core::sync::atomic::{AtomicUsize, Ordering};
use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

/// Actual mapped heap size in bytes (set by init_heap). Readers before
/// init_heap see 0; runtime_heap_size() falls back to HEAP_SIZE then.
pub static RUNTIME_HEAP_SIZE: AtomicUsize = AtomicUsize::new(0);

/// Actual heap size in bytes, valid after init_heap.
pub fn runtime_heap_size() -> usize {
    match RUNTIME_HEAP_SIZE.load(Ordering::Relaxed) {
        0 => HEAP_SIZE,
        n => n,
    }
}

pub const HEAP_START: usize = 0xFFFF_C000_0000_0000;
pub const HEAP_SIZE: usize = 128 * 1024 * 1024; // 128 MiB upper bound

/// Minimum heap we are willing to boot with. Below this the kernel heap
/// itself cannot hold its boot-time data structures.
const HEAP_MIN_SIZE: usize = 8 * 1024 * 1024;

/// K-03 (FB-005 root cause): the heap size is chosen from the Limine memory
/// map instead of being a fixed 128 MiB. The old code eagerly mapped 128 MiB
/// unconditionally, which on a 128 MiB machine exhausts physical frames
/// during boot (reproduced: `heap initialization failed:
/// FrameAllocationFailed`, tests/k03_prod_m128.log) before userspace ever
/// runs. Quarter of managed physical memory, clamped to [8 MiB, 128 MiB],
/// keeps 512 MiB machines at the historical 128 MiB behavior.
fn heap_size_for_system() -> usize {
    let managed_pages = crate::memory::overcommit::total_physical_pages();
    if managed_pages == 0 {
        // Memory map not yet parsed (should not happen: init_frame_allocator
        // runs before init_heap) — fall back to the historical constant.
        return HEAP_SIZE;
    }
    let wanted_pages = HEAP_SIZE / 4096;
    let max_pages = managed_pages / 4;
    let min_pages = HEAP_MIN_SIZE / 4096;
    let pages = wanted_pages.min(max_pages).max(min_pages);
    pages * 4096
}

#[global_allocator]
pub(crate) static ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let heap_size = heap_size_for_system();
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + (heap_size as u64) - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        // SAFETY: mapper.map_to is safe when frame is valid and flags are appropriate
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    // SAFETY: ALLOCATOR.init is safe when HEAP_START and heap_size describe a
    // valid, unused range that was fully mapped by the loop above.
    unsafe {
        ALLOCATOR.lock().init(HEAP_START, heap_size);
    }
    RUNTIME_HEAP_SIZE.store(heap_size, Ordering::Relaxed);

    crate::interrupts::serial_fmt(format_args!(
        "[BOOT] heap: {} MiB (adaptive from memory map)\n",
        heap_size / (1024 * 1024)
    ));

    Ok(())
}

#[alloc_error_handler]
pub fn handle_alloc_error(_layout: core::alloc::Layout) -> ! {
    crate::oom_kill();
}

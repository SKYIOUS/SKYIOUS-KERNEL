#![allow(dead_code)]

use bootloader_api::info::MemoryRegions;
use core::sync::atomic::{AtomicBool, Ordering};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn init(_memory_regions: &'static MemoryRegions) {
    // ponytail: consolidated — phys delegates to buddy; no separate bitmap
    INITIALIZED.store(true, Ordering::SeqCst);
}

pub fn alloc_frame() -> Option<u64> {
    crate::memory::buddy::BUDDY_ALLOCATOR
        .lock()
        .allocate_contiguous(0)
        .map(|a| a.as_u64())
}

pub fn free_frame(phys_addr: u64) {
    // ponytail: guard double-free — buddy would otherwise re-insert and inflate count
    if is_free(phys_addr) {
        return;
    }
    use x86_64::{PhysAddr, structures::paging::PhysFrame};
    let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
    crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
}

pub fn is_free(phys_addr: u64) -> bool {
    crate::memory::buddy::BUDDY_ALLOCATOR
        .lock()
        .is_free(x86_64::PhysAddr::new(phys_addr))
}

pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

pub fn total_free_frames() -> usize {
    crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages()
}

// ponytail: naive test, does not exhaust the allocator
pub fn test_alloc_free() -> Result<(), &'static str> {
    if !is_initialized() {
        return Err("phys allocator not initialized");
    }
    let f1 = alloc_frame().ok_or("alloc_frame returned None")?;
    let f2 = alloc_frame().ok_or("alloc_frame returned None on second call")?;
    if f1 == f2 {
        return Err("alloc_frame returned duplicate addresses");
    }
    if f1 & 0xFFF != 0 || f2 & 0xFFF != 0 {
        return Err("alloc_frame returned non-4K-aligned address");
    }
    free_frame(f1);
    free_frame(f2);
    if !is_free(f1) || !is_free(f2) {
        return Err("frames not free after free_frame");
    }
    Ok(())
}

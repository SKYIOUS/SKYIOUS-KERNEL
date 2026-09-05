//! # vahi-memory — Memory Management Subsystem
//!
//! Physical and virtual memory management: buddy allocator for page-level
//! allocation, slab allocator for fixed-size kernel objects, page table
//! manipulation, copy-on-write (CoW) for fork(), swap support, and
//! per-process VM isolation.
//!
//! ## Exports
//!
//! | Module | Purpose |
//! |--------|---------|
//! | `buddy` | Power-of-two page allocator (`BUDDY_ALLOCATOR` global) |
//! | `slab` | Fixed-size block allocator for kernel structs |
//! | `paging` | Page table manipulation: map, unmap, `AddressSpace` |
//! | `phys` | Physical address translation (virt_to_phys) |
//! | `virt` | Virtual memory helpers (HHDM offset) |
//! | `swap` | Page-out to disk and page-in on fault |
//! | `isolate` | Per-process address space isolation |
//! | `frame_info` | High-water-mark allocation tracking (increment/decrement/count) |
//! | `stack` | Kernel stack allocation for threads |
//! | `overcommit` | Memory overcommit policy |
//!
//! ## Kernel Integration
//!
//! The kernel re-exports this crate via `pub use vahi_memory::*` in
//! `kernel/src/memory/mod.rs`. The `frame_info` module provides allocation
//! tracking counters that the kernel calls on every alloc/dealloc.
//!
//! ## Invariants
//!
//! - All pages returned by the buddy allocator are zeroed
//! - Page table entries use proper NX/Write/Present bits
//! - CoW pages are shared until write fault, then duplicated
//! - Swap-in must be atomic with respect to the page table lock

#![no_std]

extern crate alloc;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
pub mod buddy;
pub mod frame_info;
pub mod isolate;
pub mod overcommit;
pub mod paging;
pub mod phys;
pub mod slab;
pub mod stack;
pub mod swap;
#[cfg(not(target_arch = "aarch64"))]
pub mod virt;

pub mod allocator {
    use crate::slab::{FixedSizeBlockAllocator, Locked};
    pub static ALLOCATOR: Locked<FixedSizeBlockAllocator> =
        Locked::new(FixedSizeBlockAllocator::new());
}

pub mod smp {
    pub fn broadcast_tlb_flush(_addr: u64) {}
}

// VfsNode removed — use vahi_vfs::defs::VfsNode directly.

pub fn serial_write(_msg: &str) {}

pub static PHYSICAL_MEMORY_OFFSET: spin::Once<u64> = spin::Once::new();

pub fn physical_memory_offset() -> u64 {
    *PHYSICAL_MEMORY_OFFSET
        .get()
        .expect("PHYSICAL_MEMORY_OFFSET not initialized")
}

#[cfg(not(target_arch = "aarch64"))]
/// # Safety
/// `physical_memory_offset` must be the correct identity map offset. Must only be called once.
pub unsafe fn init(
    physical_memory_offset: x86_64::VirtAddr,
) -> x86_64::structures::paging::OffsetPageTable<'static> {
    PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset.as_u64());
    let level_4_table = active_level_4_table(physical_memory_offset);
    x86_64::structures::paging::OffsetPageTable::new(level_4_table, physical_memory_offset)
}

#[cfg(target_arch = "aarch64")]
/// # Safety
/// `physical_memory_offset` must be the correct identity map offset.
pub unsafe fn init_aarch64(physical_memory_offset: u64) -> u64 {
    PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset);
    physical_memory_offset
}

#[cfg(not(target_arch = "aarch64"))]
pub fn virt_to_phys(virt: x86_64::VirtAddr) -> Option<x86_64::PhysAddr> {
    use x86_64::structures::paging::Translate;
    let offset_val = *PHYSICAL_MEMORY_OFFSET.get()?;
    let offset = x86_64::VirtAddr::new(offset_val);
    let level_4_table = unsafe { active_level_4_table(offset) };
    let mapper = unsafe { x86_64::structures::paging::OffsetPageTable::new(level_4_table, offset) };
    mapper.translate_addr(virt)
}

#[cfg(target_arch = "aarch64")]
pub fn virt_to_phys(virt: u64) -> Option<u64> {
    crate::aarch64::virt_to_phys(virt)
}

#[cfg(not(target_arch = "aarch64"))]
pub fn virt_to_phys_dma(virt: x86_64::VirtAddr) -> x86_64::PhysAddr {
    virt_to_phys(virt).unwrap_or_else(|| {
        panic!(
            "virt_to_phys_dma failed for {:?} — heap not mapped in page table?",
            virt
        )
    })
}

#[cfg(target_arch = "aarch64")]
pub fn virt_to_phys_dma(virt: u64) -> u64 {
    virt_to_phys(virt)
        .unwrap_or_else(|| panic!("virt_to_phys_dma failed for {:#x} — heap not mapped?", virt))
}

/// # Safety
/// `user_ptr` must be a valid user-space pointer for `len` bytes.
pub unsafe fn _copy_from_user(kernel_buf: &mut [u8], user_ptr: *const u8, len: usize) {
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("stac", options(nostack, preserves_flags));
    core::ptr::copy_nonoverlapping(user_ptr, kernel_buf.as_mut_ptr(), len);
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("clac", options(nostack, preserves_flags));
}

/// # Safety
/// `user_ptr` must be a valid user-space pointer for `len` bytes.
pub unsafe fn copy_to_user(user_ptr: *mut u8, kernel_buf: &[u8], len: usize) {
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("stac", options(nostack, preserves_flags));
    core::ptr::copy_nonoverlapping(kernel_buf.as_ptr(), user_ptr, len);
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("clac", options(nostack, preserves_flags));
}

#[cfg(not(target_arch = "aarch64"))]
#[doc(hidden)]
pub unsafe fn active_level_4_table(
    physical_memory_offset: x86_64::VirtAddr,
) -> &'static mut x86_64::structures::paging::PageTable {
    use x86_64::registers::control::Cr3;
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut x86_64::structures::paging::PageTable = virt.as_mut_ptr();
    &mut *page_table_ptr
}

/// # Safety
/// Must only be called once during boot after the Limine memory map is available.
pub unsafe fn init_frame_allocator_limine() {
    let mut buddy = crate::buddy::BUDDY_ALLOCATOR.lock();
    let mut total_pages: usize = 0;
    for (base, end) in vahi_limine::iter_usable_regions() {
        buddy.add_region(x86_64::PhysAddr::new(base), x86_64::PhysAddr::new(end));
        total_pages += ((end - base) / 4096) as usize;
    }
    crate::phys::init_limine();
    crate::overcommit::init(total_pages);
}

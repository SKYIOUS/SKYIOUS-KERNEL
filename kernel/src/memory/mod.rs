use x86_64::{
    structures::paging::{PageTable, OffsetPageTable},
    PhysAddr, VirtAddr,
};
use bootloader_api::info::{MemoryRegions, MemoryRegionKind};
use spin::Once;

pub mod buddy;
pub mod slab;
pub mod paging;
pub mod frame_info;
pub mod stack;

pub static PHYSICAL_MEMORY_OFFSET: Once<u64> = Once::new();

/// Initialize a new OffsetPageTable.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset.as_u64());
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Translates a virtual address to the mapped physical address.
/// Translates a virtual address to the mapped physical address.
pub fn virt_to_phys(virt: VirtAddr) -> Option<PhysAddr> {
    use x86_64::structures::paging::Translate;

    let offset_val = *PHYSICAL_MEMORY_OFFSET.get()?;
    let offset = VirtAddr::new(offset_val);
    
    let level_4_table = unsafe { active_level_4_table(offset) };
    let mapper = unsafe { OffsetPageTable::new(level_4_table, offset) };
    
    mapper.translate_addr(virt)
}

pub fn virt_to_phys_dma(virt: VirtAddr) -> PhysAddr {
    virt_to_phys(virt).unwrap_or_else(|| {
        panic!("virt_to_phys_dma failed for {:?} — heap not mapped in page table?", virt)
    })
}

/// Copies bytes from user space to a kernel buffer.
/// Uses STAC/CLAC to temporarily allow SMAP bypass.
///
/// # Safety
/// `user_ptr` must be a valid user-space pointer for `len` bytes.
pub unsafe fn _copy_from_user(kernel_buf: &mut [u8], user_ptr: *const u8, len: usize) {
    core::arch::asm!("stac", options(nostack, preserves_flags));
    core::ptr::copy_nonoverlapping(user_ptr, kernel_buf.as_mut_ptr(), len);
    core::arch::asm!("clac", options(nostack, preserves_flags));
}

/// Copies bytes from a kernel buffer to user space.
pub unsafe fn copy_to_user(user_ptr: *mut u8, kernel_buf: &[u8], len: usize) {
    core::arch::asm!("stac", options(nostack, preserves_flags));
    core::ptr::copy_nonoverlapping(kernel_buf.as_ptr(), user_ptr, len);
    core::arch::asm!("clac", options(nostack, preserves_flags));
}

/// Validates a user pointer is within a valid VMA of the current process.
pub fn _verify_user_ptr(addr: u64, len: usize) -> Result<(), crate::syscalls::errno::Errno> {
    use crate::task::process::CURRENT_PROCESS;
    let proc = CURRENT_PROCESS.lock();
    let proc = proc.as_ref().ok_or(crate::syscalls::errno::Errno::EFAULT)?;
    let end = addr.checked_add(len as u64).ok_or(crate::syscalls::errno::Errno::EFAULT)?;
    
    let vmas = proc.vmas.lock();
    for vma in vmas.iter() {
        if addr >= vma.start && end <= vma.end {
            return Ok(());
        }
    }
    Err(crate::syscalls::errno::Errno::EFAULT)
}

pub(crate) unsafe fn active_level_4_table(physical_memory_offset: VirtAddr)
    -> &'static mut PageTable
{
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    &mut *page_table_ptr
}

pub unsafe fn init_frame_allocator(memory_regions: &'static MemoryRegions) {
    // Initialize Buddy Allocator with usable regions
    let mut buddy = buddy::BUDDY_ALLOCATOR.lock();
    for region in memory_regions.iter() {
        if region.kind == MemoryRegionKind::Usable {
            buddy.add_region(
                PhysAddr::new(region.start),
                PhysAddr::new(region.end)
            );
        }
    }
}



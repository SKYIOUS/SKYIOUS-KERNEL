use spin::Once;

#[cfg(not(target_arch = "aarch64"))]
use x86_64::{
    structures::paging::{PageTable, OffsetPageTable},
    PhysAddr, VirtAddr,
};

pub mod buddy;
pub mod slab;
pub mod paging;
pub mod frame_info;
pub mod stack;
pub mod phys;
pub mod swap;
pub mod isolate;
#[cfg(not(target_arch = "aarch64"))]
pub mod virt;
#[cfg(target_arch = "aarch64")]
pub mod aarch64;

pub static PHYSICAL_MEMORY_OFFSET: Once<u64> = Once::new();

pub fn physical_memory_offset() -> u64 {
    *PHYSICAL_MEMORY_OFFSET.get().expect("PHYSICAL_MEMORY_OFFSET not initialized")
}

/// Initialize a new OffsetPageTable (x86_64 only).
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset.as_u64());
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Initialize memory for aarch64.
#[cfg(target_arch = "aarch64")]
pub unsafe fn init_aarch64(physical_memory_offset: u64) -> u64 {
    PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset);
    physical_memory_offset
}

/// Translates a virtual address to the mapped physical address.
#[cfg(not(target_arch = "aarch64"))]
pub fn virt_to_phys(virt: VirtAddr) -> Option<PhysAddr> {
    use x86_64::structures::paging::Translate;
    let offset_val = *PHYSICAL_MEMORY_OFFSET.get()?;
    let offset = VirtAddr::new(offset_val);
    let level_4_table = unsafe { active_level_4_table(offset) };
    let mapper = unsafe { OffsetPageTable::new(level_4_table, offset) };
    mapper.translate_addr(virt)
}

/// aarch64 virt_to_phys via page table walk.
#[cfg(target_arch = "aarch64")]
pub fn virt_to_phys(virt: u64) -> Option<u64> {
    crate::memory::aarch64::virt_to_phys(virt)
}

#[cfg(not(target_arch = "aarch64"))]
pub fn virt_to_phys_dma(virt: VirtAddr) -> PhysAddr {
    virt_to_phys(virt).unwrap_or_else(|| {
        panic!("virt_to_phys_dma failed for {:?} — heap not mapped in page table?", virt)
    })
}

#[cfg(target_arch = "aarch64")]
pub fn virt_to_phys_dma(virt: u64) -> u64 {
    virt_to_phys(virt).unwrap_or_else(|| {
        panic!("virt_to_phys_dma failed for {:#x} — heap not mapped?", virt)
    })
}

/// Copies bytes from user space to a kernel buffer.
/// x86_64: uses STAC/CLAC for SMAP bypass.
/// aarch64: direct access (PAN not enabled).
///
/// # Safety
/// `user_ptr` must be a valid user-space pointer for `len` bytes.
pub unsafe fn _copy_from_user(kernel_buf: &mut [u8], user_ptr: *const u8, len: usize) {
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("stac", options(nostack, preserves_flags));
    core::ptr::copy_nonoverlapping(user_ptr, kernel_buf.as_mut_ptr(), len);
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("clac", options(nostack, preserves_flags));
}

/// Copies bytes from a kernel buffer to user space.
#[allow(dead_code)]
pub unsafe fn copy_to_user(user_ptr: *mut u8, kernel_buf: &[u8], len: usize) {
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("stac", options(nostack, preserves_flags));
    core::ptr::copy_nonoverlapping(kernel_buf.as_ptr(), user_ptr, len);
    #[cfg(not(target_arch = "aarch64"))]
    core::arch::asm!("clac", options(nostack, preserves_flags));
}

/// Validates a user pointer is within a valid VMA of the current process.
pub fn _verify_user_ptr(addr: u64, len: usize) -> Result<(), crate::syscalls::errno::Errno> {
    use crate::task::process::CURRENT_PROCESS;
    let proc = CURRENT_PROCESS.lock();
    let proc = proc.as_ref().ok_or(crate::syscalls::errno::Errno::EFAULT)?;
    let end = addr.checked_add(len as u64).ok_or(crate::syscalls::errno::Errno::EFAULT)?;
    
    let vmas = proc.memory.lock().vmas.clone();
    for vma in vmas.iter() {
        if addr >= vma.start && end <= vma.end {
            return Ok(());
        }
    }
    Err(crate::syscalls::errno::Errno::EFAULT)
}

#[cfg(not(target_arch = "aarch64"))]
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

/// Initialize memory for aarch64.
/// Returns a u64 representing the physical memory offset (architecture-neutral wrapper).
#[cfg(target_arch = "aarch64")]
pub unsafe fn init_aarch64(physical_memory_offset: u64) -> u64 {
    PHYSICAL_MEMORY_OFFSET.call_once(|| physical_memory_offset);
    // aarch64 uses TTBR1_EL1 for kernel page tables; no OffsetPageTable needed.
    // The aarch64 module provides virt_to_phys via table walk.
    physical_memory_offset
}

pub unsafe fn init_frame_allocator_limine() {
    // Initialize Buddy Allocator with usable regions from Limine memory map
    let mut buddy = buddy::BUDDY_ALLOCATOR.lock();
    for (base, end) in crate::limine::iter_usable_regions() {
        buddy.add_region(PhysAddr::new(base), PhysAddr::new(end));
    }
    // Initialize bitmap page frame allocator alongside buddy
    phys::init_limine();
}



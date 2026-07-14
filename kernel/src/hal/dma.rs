use x86_64::PhysAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
}

#[allow(dead_code)]
pub struct DmaBuf {
    phys_addr: u64,
    virt_addr: *mut u8,
    size: usize,
    order: usize,
    direction: DmaDirection,
}

fn size_to_order(size: usize) -> usize {
    let page_align = size.next_power_of_two();
    let order = page_align.trailing_zeros().saturating_sub(12) as usize;
    order.min(crate::memory::buddy::MAX_ORDER)
}

impl DmaBuf {
    pub fn allocate(size: usize) -> Option<Self> {
        let order = size_to_order(size);
        let phys = crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .allocate_contiguous(order)?;
        let phys_u64 = phys.as_u64();
        let virt = (phys_u64 + crate::memory::PHYSICAL_MEMORY_OFFSET.get().copied()?) as *mut u8;
        unsafe { core::ptr::write_bytes(virt, 0, 4096usize << order) };
        Some(DmaBuf {
            phys_addr: phys_u64,
            virt_addr: virt,
            size,
            order,
            direction: DmaDirection::Bidirectional,
        })
    }

    pub fn phys(&self) -> u64 { self.phys_addr }
    pub fn virt(&self) -> *mut u8 { self.virt_addr }
    pub fn size(&self) -> usize { self.size }
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.virt_addr, self.size) }
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.virt_addr, self.size) }
    }
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        let mut buddy = crate::memory::buddy::BUDDY_ALLOCATOR.lock();
        buddy.deallocate_contiguous(PhysAddr::new(self.phys_addr), self.order);
    }
}

unsafe impl Send for DmaBuf {}
unsafe impl Sync for DmaBuf {}

pub trait DmaCacheOps {
    unsafe fn clean_invalidate_range(phys_addr: u64, size: usize);
    unsafe fn invalidate_range(phys_addr: u64, size: usize);
    unsafe fn clean_range(phys_addr: u64, size: usize);
}

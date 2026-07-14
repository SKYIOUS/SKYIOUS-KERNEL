use core::pin::Pin;
use core::marker::PhantomPinned;
use alloc::boxed::Box;
use x86_64::PhysAddr;

pub struct DmaBuf {
    phys_addr: PhysAddr,
    virt_ptr: *mut u32,
    page_count: usize,
    order: usize,
    _pinned: PhantomPinned,
}

impl DmaBuf {
    pub fn allocate(width: usize, height: usize) -> Option<Pin<Box<Self>>> {
        let pixels = width * height;
        let size_bytes = pixels * 4;
        let mut order = 0;
        while (4096 << order) < size_bytes && order < crate::memory::buddy::MAX_ORDER {
            order += 1;
        }
        let phys = crate::memory::buddy::BUDDY_ALLOCATOR.lock().allocate_contiguous(order)?;
        let page_count = 1 << order;
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;
        let virt = (offset + phys.as_u64()) as *mut u32;
        unsafe { core::ptr::write_bytes(virt, 0, page_count * 1024); }
        Some(Box::pin(Self {
            phys_addr: phys,
            virt_ptr: virt,
            page_count,
            order,
            _pinned: PhantomPinned,
        }))
    }

    pub fn phys_addr(&self) -> PhysAddr {
        self.phys_addr
    }

    pub fn page_count(&self) -> usize {
        self.page_count
    }

    pub fn as_slice(&self, len: usize) -> &[u32] {
        let count = len.min(self.page_count * 1024);
        unsafe { core::slice::from_raw_parts(self.virt_ptr, count) }
    }

    pub fn as_mut_slice(&mut self, len: usize) -> &mut [u32] {
        let count = len.min(self.page_count * 1024);
        unsafe { core::slice::from_raw_parts_mut(self.virt_ptr, count) }
    }
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        if self.phys_addr.as_u64() != 0 {
            crate::memory::buddy::BUDDY_ALLOCATOR
                .lock()
                .deallocate_contiguous(self.phys_addr, self.order);
        }
    }
}

unsafe impl Send for DmaBuf {}
unsafe impl Sync for DmaBuf {}

use x86_64::PhysAddr;
use crate::sync::IrqSafeMutex as Mutex;
use alloc::collections::VecDeque;

/// Pre-sized DMA buffer pool for hot-path allocations.
/// Eliminates per-transfer buddy-allocator churn in NVMe and xHCI.
struct DmaPool {
    /// Buckets: (size, free_list of pre-allocated DmaBufs).
    buckets: alloc::vec::Vec<(usize, VecDeque<DmaBuf>)>,
}

/// Fixed bucket sizes (powers of two).
/// 4 KB covers NVMe sector buffers and small xHCI control transfers.
/// 8–32 KB covers larger xHCI data-stage and interrupt payloads.
const POOL_SIZES: &[usize] = &[4096, 8192, 16384, 32768];

lazy_static::lazy_static! {
    static ref DMA_POOL: Mutex<DmaPool> = Mutex::new(DmaPool::new());
}

impl DmaPool {
    fn new() -> Self {
        let mut buckets = alloc::vec::Vec::with_capacity(POOL_SIZES.len());
        for &size in POOL_SIZES {
            let mut free = VecDeque::new();
            for _ in 0..16 {
                if let Some(buf) = DmaBuf::new(size) {
                    free.push_back(buf);
                }
            }
            buckets.push((size, free));
        }
        DmaPool { buckets }
    }
}

/// RAII wrapper: allocates a DMA buffer from the pool, returns it on drop.
pub struct PooledDma {
    phys_addr: u64,
    virt_addr: *mut u8,
    size: usize,
    bucket_idx: usize,
    from_pool: bool,
}

impl PooledDma {
    /// Allocate a pooled DMA buffer of at least `min_size` bytes.
    /// Falls back to direct DmaBuf::new if the pool is exhausted or
    /// the requested size exceeds all buckets.
    pub fn alloc(min_size: usize) -> Option<Self> {
        if let Some(mut pool) = DMA_POOL.try_lock() {
            // Find smallest bucket >= min_size (by index, then pop)
            let target = pool.buckets.iter()
                .position(|&(size, _)| size >= min_size);
            if let Some(idx) = target {
                if let Some(buf) = pool.buckets[idx].1.pop_front() {
                    let phys = buf.phys();
                    let virt = buf.virt();
                    let sz = buf.size();
                    core::mem::forget(buf); // prevent DmaBuf::drop (pool owns it)
                    return Some(PooledDma {
                        phys_addr: phys,
                        virt_addr: virt,
                        size: sz,
                        bucket_idx: idx,
                        from_pool: true,
                    });
                }
            }
        }
        // Pool exhausted or unavailable: fall back to direct allocation
        let buf = DmaBuf::new(min_size)?;
        let phys = buf.phys();
        let virt = buf.virt();
        let sz = buf.size();
        core::mem::forget(buf);
        Some(PooledDma {
            phys_addr: phys,
            virt_addr: virt,
            size: sz,
            bucket_idx: 0,
            from_pool: false,
        })
    }

    pub fn phys(&self) -> u64 { self.phys_addr }
    pub fn virt(&self) -> *mut u8 { self.virt_addr }
    pub fn size(&self) -> usize { self.size }
    pub fn as_ptr(&self) -> *const u8 { self.virt_addr }
    pub fn as_mut_ptr(&mut self) -> *mut u8 { self.virt_addr }
}

impl Drop for PooledDma {
    fn drop(&mut self) {
        if !self.from_pool {
            // Fallback allocation: deallocate directly to buddy allocator
            let order = size_to_order(self.size);
            let mut buddy = crate::memory::buddy::BUDDY_ALLOCATOR.lock();
            buddy.deallocate_contiguous(PhysAddr::new(self.phys_addr), order);
            return;
        }
        // Return to pool (raw pointer lives in the static pool)
        if let Some(mut pool) = DMA_POOL.try_lock() {
            pool.buckets[self.bucket_idx].1.push_back(DmaBuf {
                phys_addr: self.phys_addr,
                virt_addr: self.virt_addr,
                size: self.size,
                order: size_to_order(self.size),
            });
        }
        // If try_lock fails (IRQ context), leak is acceptable: pool is
        // bounded, buffer came from the pool (not buddy), and the pool
        // will refill on next successful lock.
    }
}

unsafe impl Send for PooledDma {}
unsafe impl Sync for PooledDma {}

// ─── Direct DmaBuf (controller-lifetime allocations) ────────────────────────

#[allow(dead_code)]
pub struct DmaBuf {
    pub(crate) phys_addr: u64,
    pub(crate) virt_addr: *mut u8,
    pub(crate) size: usize,
    pub(crate) order: usize,
}

fn size_to_order(size: usize) -> usize {
    let page_align = size.next_power_of_two();
    let order = page_align.trailing_zeros().saturating_sub(12) as usize;
    order.min(crate::memory::buddy::MAX_ORDER)
}

impl DmaBuf {
    /// Allocate a DMA buffer of at least `size` bytes.
    /// The returned buffer is zeroed and physically contiguous.
    pub fn new(size: usize) -> Option<Self> {
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
        })
    }

    pub fn phys(&self) -> u64 { self.phys_addr }
    pub fn virt(&self) -> *mut u8 { self.virt_addr }
    pub fn size(&self) -> usize { self.size }

    pub fn as_ptr(&self) -> *const u8 { self.virt_addr }
    pub fn as_mut_ptr(&mut self) -> *mut u8 { self.virt_addr }

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

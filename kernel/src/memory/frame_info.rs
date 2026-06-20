use x86_64::PhysAddr;
use spin::Mutex;

struct RefCountTable {
    counts: &'static mut [u16],
    deferred: alloc::vec::Vec<PhysAddr>,
}

static REFCOUNTS: Mutex<Option<RefCountTable>> = Mutex::new(None);

pub fn init(max_phys: u64) {
    let num_frames = (max_phys as usize >> 12) + 1;
    let layout = alloc::alloc::Layout::array::<u16>(num_frames).unwrap();
    let ptr = unsafe { alloc::alloc::alloc(layout) as *mut u16 };
    if !ptr.is_null() {
        unsafe { core::ptr::write_bytes(ptr, 0, num_frames); }
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr, num_frames) };
        *REFCOUNTS.lock() = Some(RefCountTable { counts: slice, deferred: alloc::vec::Vec::new() });
    }
}

pub fn increment(phys: PhysAddr) {
    let mut table = REFCOUNTS.lock();
    if let Some(ref mut tbl) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < tbl.counts.len() {
            tbl.counts[i] = tbl.counts[i].saturating_add(1);
        }
    }
}

/// Decrements refcount for the given physical frame.
/// Returns the remaining refcount. When it reaches 0 the frame is queued
/// for deferred deallocation (inside the refcount lock) to prevent TOCTOU
/// races with concurrent COW handlers.
///
/// Deferred frames are freed by drain_deferred() called from the
/// scheduler idle path, which avoids the lock-ordering deadlock between
/// REFCOUNTS and BUDDY_ALLOCATOR.
pub fn decrement(phys: PhysAddr) -> u16 {
    let mut table = REFCOUNTS.lock();
    if let Some(ref mut tbl) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < tbl.counts.len() {
            if tbl.counts[i] > 1 {
                tbl.counts[i] -= 1;
                return tbl.counts[i];
            }
            tbl.counts[i] = 0;
            // Queue for deferred free while still holding the refcount lock.
            // This prevents another thread from allocating the same frame
            // before it's dequeued for actual deallocation.
            tbl.deferred.push(phys);
            return 0;
        }
    }
    0
}

/// Process the deferred-free queue. Must NOT be called while holding any
/// BUDDY_ALLOCATOR or REFCOUNTS lock.
pub fn drain_deferred() {
    let frames = {
        let mut table = REFCOUNTS.lock();
        if let Some(ref mut tbl) = *table {
            core::mem::take(&mut tbl.deferred)
        } else {
            return;
        }
    };
    for phys in &frames {
        let frame = x86_64::structures::paging::PhysFrame::containing_address(*phys);
        crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
    }
}

/// Returns the current refcount for a frame. Unmanaged frames return 1.
pub fn count(phys: PhysAddr) -> u16 {
    let table = REFCOUNTS.lock();
    if let Some(ref refcounts) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < refcounts.counts.len() { return refcounts.counts[i]; }
    }
    1
}

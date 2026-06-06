use x86_64::PhysAddr;
use spin::Mutex;

/// Flat array of refcounts indexed by physical frame number (phys >> 12).
/// Lazily initialized with max_phys at boot. Unmanaged frames (not in range) are
/// treated as having refcount 1, so decrement to 0 frees the frame.
static REFCOUNTS: Mutex<Option<&'static mut [u16]>> = Mutex::new(None);

pub fn init(max_phys: u64) {
    let num_frames = (max_phys as usize >> 12) + 1;
    let layout = alloc::alloc::Layout::array::<u16>(num_frames).unwrap();
    let ptr = unsafe { alloc::alloc::alloc(layout) as *mut u16 };
    if !ptr.is_null() {
        unsafe { core::ptr::write_bytes(ptr, 0, num_frames); }
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr, num_frames) };
        *REFCOUNTS.lock() = Some(slice);
    }
}

pub fn increment(phys: PhysAddr) {
    let mut table = REFCOUNTS.lock();
    if let Some(ref mut refcounts) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < refcounts.len() {
            refcounts[i] = refcounts[i].saturating_add(1);
        }
    }
}

/// Decrements refcount for the given physical frame.
/// Returns the remaining refcount. When it reaches 0 the frame is freed
/// back to the buddy allocator (after releasing the refcount lock to
/// avoid lock inversion with the buddy mutex).
pub fn decrement(phys: PhysAddr) -> u16 {
    let should_free = {
        let mut table = REFCOUNTS.lock();
        if let Some(ref mut refcounts) = *table {
            let i = (phys.as_u64() >> 12) as usize;
            if i < refcounts.len() {
                if refcounts[i] > 1 {
                    refcounts[i] -= 1;
                    return refcounts[i];
                }
                refcounts[i] = 0;
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if should_free {
        let frame = x86_64::structures::paging::PhysFrame::containing_address(phys);
        crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
    }
    0
}

/// Returns the current refcount for a frame. Unmanaged frames return 1.
pub fn count(phys: PhysAddr) -> u16 {
    let table = REFCOUNTS.lock();
    if let Some(ref refcounts) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < refcounts.len() { return refcounts[i]; }
    }
    1
}

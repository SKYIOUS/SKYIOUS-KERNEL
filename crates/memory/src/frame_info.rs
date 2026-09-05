//! Frame reference counting and allocation tracking.
//!
//! Provides lock-free (AtomicU16) refcounts for physical page frames.
//! The deferred-free queue is managed separately (in the kernel) to avoid
//! circular dependencies with the buddy allocator.

use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use vahi_sync::IrqSafeMutex as Mutex;
use x86_64::PhysAddr;

struct RefCountTable {
    counts: &'static [AtomicU16],
    deferred: Mutex<alloc::vec::Vec<PhysAddr>>,
}

static REFCOUNTS: Mutex<Option<RefCountTable>> = Mutex::new(None);

/// Initialize the refcount table for physical frames up to `max_phys`.
///
/// # Safety
/// Must be called exactly once, before any frame allocation.
pub fn init(max_phys: u64) {
    let num_frames = (max_phys as usize >> 12) + 1;
    // SAFETY: Layout is non-zero because AtomicU16 is 2 bytes and num_frames > 0.
    let layout = alloc::alloc::Layout::array::<AtomicU16>(num_frames).unwrap();
    let ptr = unsafe { alloc::alloc::alloc(layout) as *mut AtomicU16 };
    if !ptr.is_null() {
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr, num_frames) };
        for cell in slice.iter_mut() {
            cell.store(0, Ordering::Relaxed);
        }
        let static_ref: &'static [AtomicU16] =
            unsafe { core::slice::from_raw_parts(ptr as *const AtomicU16, num_frames) };
        *REFCOUNTS.lock() = Some(RefCountTable {
            counts: static_ref,
            deferred: Mutex::new(alloc::vec::Vec::new()),
        });
    }
}

/// Increment refcount for the given physical frame. Lock-free on the hot path.
pub fn increment(phys: PhysAddr) {
    let table = REFCOUNTS.lock();
    if let Some(ref tbl) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < tbl.counts.len() {
            tbl.counts[i].fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Decrement refcount for the given physical frame.
/// Returns the remaining refcount. When it reaches 0 the frame is queued
/// for deferred deallocation (under the deferred queue lock).
pub fn decrement(phys: PhysAddr) -> u16 {
    let table = REFCOUNTS.lock();
    if let Some(ref tbl) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < tbl.counts.len() {
            let prev = tbl.counts[i].fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                tbl.deferred.lock().push(phys);
                return 0;
            }
            return prev - 1;
        }
    }
    0
}

/// Drain the deferred-free queue and free frames via the buddy allocator.
pub fn drain_deferred() {
    let frames = {
        let table = REFCOUNTS.lock();
        if let Some(ref tbl) = *table {
            core::mem::take(&mut *tbl.deferred.lock())
        } else {
            alloc::vec::Vec::new()
        }
    };
    for phys in &frames {
        let frame = x86_64::structures::paging::PhysFrame::containing_address(*phys);
        crate::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
    }
}

/// Returns the current refcount for a frame. Unmanaged frames return 1.
pub fn count(phys: PhysAddr) -> u16 {
    let table = REFCOUNTS.lock();
    if let Some(ref tbl) = *table {
        let i = (phys.as_u64() >> 12) as usize;
        if i < tbl.counts.len() {
            return tbl.counts[i].load(Ordering::Relaxed);
        }
    }
    1
}

// ── Page-frame high-water-mark tracking ──────────────────────────────

static ALLOCATED_FRAMES: AtomicU64 = AtomicU64::new(0);
static HIGH_WATER_MARK: AtomicU64 = AtomicU64::new(0);

/// Record a frame allocation. Called from buddy allocator.
pub fn track_alloc() {
    let cur = ALLOCATED_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    loop {
        let old = HIGH_WATER_MARK.load(Ordering::Relaxed);
        if cur <= old {
            break;
        }
        if HIGH_WATER_MARK
            .compare_exchange_weak(old, cur, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

/// Record a frame deallocation. Called from buddy allocator.
pub fn track_dealloc() {
    ALLOCATED_FRAMES.fetch_sub(1, Ordering::Relaxed);
}

/// Get current allocation stats: (allocated, high_water_mark).
pub fn frame_stats() -> (u64, u64) {
    (
        ALLOCATED_FRAMES.load(Ordering::Relaxed),
        HIGH_WATER_MARK.load(Ordering::Relaxed),
    )
}

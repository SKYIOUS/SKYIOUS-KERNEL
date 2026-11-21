//! Frame reference counting and allocation tracking.
//!
//! Provides lock-free (AtomicU16) refcounts for physical page frames.
//! The deferred-free queue is managed separately (in the kernel) to avoid
//! circular dependencies with the buddy allocator.

use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use vahi_hal::sync::IrqSafeMutex as Mutex;

/// Opaque physical frame identifier (page frame number).
pub type FrameId = u64;

struct RefCountTable {
    counts: &'static [AtomicU16],
    deferred: Mutex<alloc::vec::Vec<FrameId>>,
}

static REFCOUNTS: Mutex<Option<RefCountTable>> = Mutex::new(None);

/// Initialize refcounts below `max_phys`, the exclusive end of usable RAM.
///
/// Must be called exactly once, after heap initialization and before any
/// reference-counted mappings are created.
pub fn init(max_phys: u64) {
    let num_frames = usize::try_from(max_phys.div_ceil(4096))
        .expect("frame refcount table exceeds addressable memory");
    let mut counts = alloc::vec::Vec::new();
    counts
        .try_reserve_exact(num_frames)
        .expect("frame refcount table allocation failed");
    counts.resize_with(num_frames, || AtomicU16::new(0));
    *REFCOUNTS.lock() = Some(RefCountTable {
        counts: counts.leak(),
        deferred: Mutex::new(alloc::vec::Vec::new()),
    });
}

/// Internal increment function using frame ID.
fn increment_inner(frame: FrameId) {
    let table = REFCOUNTS.lock();
    if let Some(ref tbl) = *table {
        let i = (frame >> 12) as usize;
        if i < tbl.counts.len() {
            tbl.counts[i].fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Internal decrement function using frame ID.
fn decrement_inner(frame: FrameId) -> u16 {
    let table = REFCOUNTS.lock();
    if let Some(ref tbl) = *table {
        let i = (frame >> 12) as usize;
        if i < tbl.counts.len() {
            let prev = tbl.counts[i].fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                tbl.deferred.lock().push(frame);
                return 0;
            }
            return prev - 1;
        }
    }
    0
}

/// Internal count function using frame ID.
fn count_inner(frame: FrameId) -> u16 {
    let table = REFCOUNTS.lock();
    if let Some(ref tbl) = *table {
        let i = (frame >> 12) as usize;
        if i < tbl.counts.len() {
            return tbl.counts[i].load(Ordering::Relaxed);
        }
    }
    1
}

// ── x86_64-specific API (backward compatible) ──────────────────────────

#[cfg(target_arch = "x86_64")]
/// Increment refcount for a physical address.
pub fn increment(addr: x86_64::PhysAddr) {
    increment_inner(addr.as_u64() >> 12);
}

#[cfg(target_arch = "x86_64")]
/// Decrement refcount for a physical address.
pub fn decrement(addr: x86_64::PhysAddr) -> u16 {
    decrement_inner(addr.as_u64() >> 12)
}

#[cfg(target_arch = "x86_64")]
/// Count refcount for a physical address.
pub fn count(addr: x86_64::PhysAddr) -> u16 {
    count_inner(addr.as_u64() >> 12)
}

#[cfg(target_arch = "x86_64")]
/// Increment refcount for a physical frame.
pub fn increment_frame<Size: x86_64::structures::paging::PageSize>(
    frame: x86_64::structures::paging::PhysFrame<Size>,
) {
    increment_inner(frame.start_address().as_u64() >> 12);
}

#[cfg(target_arch = "x86_64")]
/// Decrement refcount for a physical frame.
pub fn decrement_frame<Size: x86_64::structures::paging::PageSize>(
    frame: x86_64::structures::paging::PhysFrame<Size>,
) -> u16 {
    decrement_inner(frame.start_address().as_u64() >> 12)
}

#[cfg(target_arch = "x86_64")]
/// Count refcount for a physical frame.
pub fn count_frame<Size: x86_64::structures::paging::PageSize>(
    frame: x86_64::structures::paging::PhysFrame<Size>,
) -> u16 {
    count_inner(frame.start_address().as_u64() >> 12)
}

#[cfg(target_arch = "x86_64")]
/// Drain deferred queue with buddy allocator deallocation.
pub fn drain_deferred() {
    use crate::buddy::BUDDY_ALLOCATOR;
    let frames = {
        let table = REFCOUNTS.lock();
        if let Some(ref tbl) = *table {
            core::mem::take(&mut *tbl.deferred.lock())
        } else {
            alloc::vec::Vec::new()
        }
    };
    for frame_id in frames {
        let phys = x86_64::PhysAddr::new(frame_id << 12);
        let frame = x86_64::structures::paging::PhysFrame::<x86_64::structures::paging::Size4KiB>::containing_address(phys);
        BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
    }
}

// ── Platform-agnostic API (for aarch64/riscv64) ────────────────────────

#[cfg(not(target_arch = "x86_64"))]
/// Increment refcount for a frame ID.
pub fn increment(frame: FrameId) {
    increment_inner(frame);
}

#[cfg(not(target_arch = "x86_64"))]
/// Decrement refcount for a frame ID.
pub fn decrement(frame: FrameId) -> u16 {
    decrement_inner(frame)
}

#[cfg(not(target_arch = "x86_64"))]
/// Count refcount for a frame ID.
pub fn count(frame: FrameId) -> u16 {
    count_inner(frame)
}

#[cfg(not(target_arch = "x86_64"))]
/// Drain deferred queue with custom deallocator.
pub fn drain_deferred<F>(mut dealloc: F)
where
    F: FnMut(FrameId),
{
    let frames = {
        let table = REFCOUNTS.lock();
        if let Some(ref tbl) = *table {
            core::mem::take(&mut *tbl.deferred.lock())
        } else {
            alloc::vec::Vec::new()
        }
    };
    for frame in frames {
        dealloc(frame);
    }
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

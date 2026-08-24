#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn init_limine() {
    // ponytail: consolidated — phys delegates to buddy; no separate bitmap
    INITIALIZED.store(true, Ordering::SeqCst);
}

pub fn alloc_frame() -> Option<u64> {
    crate::memory::buddy::BUDDY_ALLOCATOR
        .lock()
        .allocate_contiguous(0)
        .map(|a| a.as_u64())
}

pub fn free_frame(phys_addr: u64) {
    // ponytail: guard double-free — buddy would otherwise re-insert and inflate count
    if is_free(phys_addr) {
        return;
    }
    use x86_64::{PhysAddr, structures::paging::PhysFrame};
    let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
    crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
}

pub fn is_free(phys_addr: u64) -> bool {
    crate::memory::buddy::BUDDY_ALLOCATOR
        .lock()
        .is_free(x86_64::PhysAddr::new(phys_addr))
}

pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

pub fn total_free_frames() -> usize {
    crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages()
}

// ---------------------------------------------------------------------------
// Memory leak auditing
// ---------------------------------------------------------------------------

/// High-water mark: lowest number of free frames seen (i.e., peak usage)
static FREE_FRAME_HWM: AtomicUsize = AtomicUsize::new(usize::MAX);
/// Low-water mark: highest number of free frames seen (i.e., minimum usage)
static FREE_FRAME_LWM: AtomicUsize = AtomicUsize::new(0);
/// Snapshot at last audit checkpoint
static FREE_FRAME_BASELINE: AtomicUsize = AtomicUsize::new(0);

/// Take a memory snapshot. Call at boot (after allocator init) to set baseline.
pub fn snapshot_baseline() {
    let free = total_free_frames();
    FREE_FRAME_BASELINE.store(free, Ordering::Relaxed);
    FREE_FRAME_HWM.store(free, Ordering::Relaxed);
    FREE_FRAME_LWM.store(free, Ordering::Relaxed);
}

/// Reset watermarks for a new measurement window.
pub fn reset_watermarks() {
    let free = total_free_frames();
    FREE_FRAME_HWM.store(free, Ordering::Relaxed);
    FREE_FRAME_LWM.store(free, Ordering::Relaxed);
    FREE_FRAME_BASELINE.store(free, Ordering::Relaxed);
}

/// Update high/low water marks. Call periodically or after stress operations.
pub fn update_watermarks() {
    let free = total_free_frames();
    FREE_FRAME_HWM.fetch_min(free, Ordering::Relaxed);
    FREE_FRAME_LWM.fetch_max(free, Ordering::Relaxed);
}

/// Get the current memory audit snapshot.
pub fn audit_snapshot() -> MemoryAudit {
    let free = total_free_frames();
    let baseline = FREE_FRAME_BASELINE.load(Ordering::Relaxed);
    let hwm = FREE_FRAME_HWM.load(Ordering::Relaxed);
    let lwm = FREE_FRAME_LWM.load(Ordering::Relaxed);
    MemoryAudit {
        free_frames: free,
        baseline_frames: baseline,
        peak_usage_frames: if baseline != usize::MAX { baseline.saturating_sub(hwm) } else { 0 },
        current_leak: if baseline != usize::MAX { baseline.saturating_sub(free) } else { 0 },
        min_free: lwm,
    }
}

/// Memory audit snapshot
pub struct MemoryAudit {
    pub free_frames: usize,
    pub baseline_frames: usize,
    pub peak_usage_frames: usize,
    pub current_leak: usize,
    pub min_free: usize,
}

impl MemoryAudit {
    pub fn report(&self) {
        crate::serial_write(&alloc::format!(
            "[MEM-AUDIT] free={} baseline={} peak_usage={} current_leak={} min_free={}\n",
            self.free_frames, self.baseline_frames, self.peak_usage_frames,
            self.current_leak, self.min_free
        ));
    }

    pub fn has_leak(&self) -> bool {
        // A leak is suspicious if we've lost more than 64 frames (256 KiB) from baseline
        // and haven't recovered. Allow some slack for caches and buffers.
        self.current_leak > 64
    }
}

// ponytail: naive test, does not exhaust the allocator
pub fn test_alloc_free() -> Result<(), &'static str> {
    if !is_initialized() {
        return Err("phys allocator not initialized");
    }
    let f1 = alloc_frame().ok_or("alloc_frame returned None")?;
    let f2 = alloc_frame().ok_or("alloc_frame returned None on second call")?;
    if f1 == f2 {
        return Err("alloc_frame returned duplicate addresses");
    }
    if f1 & 0xFFF != 0 || f2 & 0xFFF != 0 {
        return Err("alloc_frame returned non-4K-aligned address");
    }
    free_frame(f1);
    free_frame(f2);
    if !is_free(f1) || !is_free(f2) {
        return Err("frames not free after free_frame");
    }
    Ok(())
}

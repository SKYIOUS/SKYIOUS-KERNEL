//! Coverage bitmap for coverage-guided fuzzing.
//!
//! Maintains a 64 KiB coverage bitmap for tracking basic block coverage.
//! Each basic block maps to a byte in the bitmap via hash; toggling
//! a bit indicates new coverage.
//!
//! Design: same coverage bitmap layout as AFL/libFuzzer (64 KiB, XOR-based).

use core::sync::atomic::AtomicBool;

/// Size of the coverage bitmap in bytes (64 KiB — matches AFL).
pub const COVERAGE_BITMAP_SIZE: usize = 64 * 1024;

/// Coverage bitmap: XOR-folded hash of basic block addresses.
pub struct CoverageBitmap {
    bitmap: alloc::boxed::Box<[u8; COVERAGE_BITMAP_SIZE]>,
    unique_blocks: usize,
    total_hits: usize,
    initialized: AtomicBool,
}

// SAFETY: CoverageBitmap is only accessed from the boot CPU during init
// and from serial_write which is interrupt-safe.
unsafe impl Send for CoverageBitmap {}
unsafe impl Sync for CoverageBitmap {}

impl CoverageBitmap {
    /// Create a new coverage bitmap (heap-allocated).
    pub fn new() -> Self {
        let bitmap = alloc::boxed::Box::new([0u8; COVERAGE_BITMAP_SIZE]);
        CoverageBitmap {
            bitmap,
            unique_blocks: 0,
            total_hits: 0,
            initialized: AtomicBool::new(true),
        }
    }

    /// Record a basic block hit.
    #[inline(always)]
    pub fn record_block(&mut self, block_addr: u64) {
        let folded = ((block_addr >> 4) ^ (block_addr >> 16)) as usize;
        let idx = folded & (COVERAGE_BITMAP_SIZE - 1);

        let byte = &mut self.bitmap[idx];
        let old = *byte;
        *byte = old ^ ((block_addr & 0xF) as u8);

        self.total_hits += 1;
        if old == 0 && *byte != 0 {
            self.unique_blocks += 1;
        }
    }

    pub fn unique_blocks(&self) -> usize {
        self.unique_blocks
    }

    pub fn total_hits(&self) -> usize {
        self.total_hits
    }

    pub fn coverage_ratio(&self) -> f64 {
        self.unique_blocks as f64 / COVERAGE_BITMAP_SIZE as f64
    }

    pub fn snapshot(&self, buf: &mut [u8; COVERAGE_BITMAP_SIZE]) {
        buf.copy_from_slice(self.bitmap.as_slice());
    }

    pub fn reset(&mut self) {
        self.bitmap.fill(0);
        self.unique_blocks = 0;
        self.total_hits = 0;
    }

    pub fn diff(old: &[u8; COVERAGE_BITMAP_SIZE], new: &[u8; COVERAGE_BITMAP_SIZE]) -> usize {
        let mut new_bytes = 0;
        for i in 0..COVERAGE_BITMAP_SIZE {
            if old[i] == 0 && new[i] != 0 {
                new_bytes += 1;
            }
        }
        new_bytes
    }
}

// ── Global singleton ────────────────────────────────────────────────

use crate::sync::IrqSafeMutex as Mutex;
use spin::Once;

static COVERAGE: Once<Mutex<CoverageBitmap>> = Once::new();

/// Initialize the coverage bitmap. Called once during boot.
pub fn init() {
    COVERAGE.call_once(|| {
        let cov = CoverageBitmap::new();
        crate::serial_write(&alloc::format!(
            "[COVERAGE] Bitmap {} bytes (heap-allocated)\n",
            COVERAGE_BITMAP_SIZE,
        ));
        Mutex::new(cov)
    });
}

/// Record a basic block hit. Hot path — must be fast.
#[inline(always)]
pub fn record_block(block_addr: u64) {
    if let Some(cov) = COVERAGE.get() {
        cov.lock().record_block(block_addr);
    }
}

/// Get unique block count.
pub fn unique_blocks() -> usize {
    COVERAGE.get().map_or(0, |c| c.lock().unique_blocks())
}

/// Get total hits.
pub fn total_hits() -> usize {
    COVERAGE.get().map_or(0, |c| c.lock().total_hits())
}

/// Get coverage ratio.
pub fn coverage_ratio() -> f64 {
    COVERAGE.get().map_or(0.0, |c| c.lock().coverage_ratio())
}

/// Take a snapshot of the bitmap.
pub fn snapshot(buf: &mut [u8; COVERAGE_BITMAP_SIZE]) {
    if let Some(cov) = COVERAGE.get() {
        cov.lock().snapshot(buf);
    }
}

/// Reset the bitmap.
pub fn reset() {
    if let Some(cov) = COVERAGE.get() {
        cov.lock().reset();
    }
}

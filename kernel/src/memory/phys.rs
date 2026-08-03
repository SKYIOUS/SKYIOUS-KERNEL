#![allow(dead_code)]

use bootloader_api::info::{MemoryRegions, MemoryRegionKind};
use crate::sync::IrqSafeMutex as Mutex;
use core::sync::atomic::{AtomicBool, Ordering};

const MAX_FRAMES: usize = 1_048_576; // enough for 4 GB @ 4K pages
const BITMAP_WORDS: usize = MAX_FRAMES / 64;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static BITMAP: Mutex<[u64; BITMAP_WORDS]> = Mutex::new([0u64; BITMAP_WORDS]);
static TOTAL_FREE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static BITMAP_BASE_FRAME: Mutex<u64> = Mutex::new(0);

fn frame_index(phys_addr: u64) -> Option<usize> {
    let base = *BITMAP_BASE_FRAME.lock();
    let frame = phys_addr >> 12;
    if frame < base { return None; }
    let idx = (frame - base) as usize;
    if idx >= MAX_FRAMES { None } else { Some(idx) }
}

pub fn init(memory_regions: &'static MemoryRegions) {
    if INITIALIZED.swap(true, Ordering::SeqCst) { return; }

    let mut bitmap = BITMAP.lock();
    let mut base_frame = u64::MAX;

    // Find the lowest usable region to set as base
    for region in memory_regions.iter() {
        if region.kind == MemoryRegionKind::Usable {
            let frame = region.start >> 12;
            if frame < base_frame {
                base_frame = frame;
            }
        }
    }
    if base_frame == u64::MAX { base_frame = 0; }
    *BITMAP_BASE_FRAME.lock() = base_frame;

    // Mark everything used initially
    for word in bitmap.iter_mut() {
        *word = u64::MAX;
    }

    // Mark usable regions as free
    for region in memory_regions.iter() {
        if region.kind == MemoryRegionKind::Usable {
            let start_frame = region.start >> 12;
            let end_frame = (region.end - 1) >> 12;
            let start_idx = ((start_frame.saturating_sub(base_frame)) as usize).min(MAX_FRAMES);
            let end_idx = ((end_frame.saturating_sub(base_frame)) as usize).min(MAX_FRAMES);
            for i in start_idx..=end_idx {
                if i < MAX_FRAMES {
                    let word = &mut bitmap[i / 64];
                    let bit = i % 64;
                    *word &= !(1u64 << bit);
                    TOTAL_FREE.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

pub fn alloc_frame() -> Option<u64> {
    let mut bitmap = BITMAP.lock();
    let base = *BITMAP_BASE_FRAME.lock();
    for (word_idx, word) in bitmap.iter_mut().enumerate() {
        if *word != 0 {
            let bit = word.trailing_zeros() as usize;
            *word &= !(1u64 << bit);
            let frame_idx = word_idx * 64 + bit;
            if frame_idx < MAX_FRAMES {
                let phys = (base + frame_idx as u64) << 12;
                TOTAL_FREE.fetch_sub(1, Ordering::Relaxed);
                return Some(phys);
            }
        }
    }
    None
}

pub fn free_frame(phys_addr: u64) {
    if let Some(idx) = frame_index(phys_addr) {
        if idx < MAX_FRAMES {
            let mut bitmap = BITMAP.lock();
            let word = &mut bitmap[idx / 64];
            let bit = idx % 64;
            let mask = 1u64 << bit;
            if *word & mask == 0 {
                *word |= mask;
                TOTAL_FREE.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

pub fn is_free(phys_addr: u64) -> bool {
    if let Some(idx) = frame_index(phys_addr) {
        if idx < MAX_FRAMES {
            let bitmap = BITMAP.lock();
            let word = bitmap[idx / 64];
            let bit = idx % 64;
            return (word & (1u64 << bit)) != 0;
        }
    }
    false
}

pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

pub fn total_free_frames() -> usize {
    TOTAL_FREE.load(Ordering::Relaxed)
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

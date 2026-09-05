//! vm.overcommit_memory — memory overcommit policy enforcement.
//!
//! Three modes (matching Linux semantics):
//!
//! - **Mode 0 (Heuristic)**: Default. Overcommit is allowed if committed pages
//!   do not exceed total physical pages plus a small margin. Rejects allocations
//!   that would clearly exhaust memory.
//!
//! - **Mode 1 (Always)**: Always permit overcommit. For workloads that rely on
//!   optimistic allocation (e.g., JVM large heaps). OOM killer is the only
//!   safety net.
//!
//! - **Mode 2 (Strict)**: Never commit more than `total_physical * ratio / 100`.
//!   Hard cap — rejects allocations before memory exhaustion occurs. This is
//!   the mode that prevents memory exhaustion attacks.
//!
//! Commitment tracking: every VMA creation (mmap/brk) increments committed
//! pages; VMA destruction (munmap/brk shrink) decrements. The page fault
//! handler (demand paging) re-checks for mode 2 to enforce the cap at fault
//! time when pages are actually allocated.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Overcommit mode — matches Linux /proc/sys/vm/overcommit_memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OvercommitMode {
    /// Mode 0: heuristic — allow overcommit with sanity bounds.
    Heuristic = 0,
    /// Mode 1: always allow overcommit.
    Always = 1,
    /// Mode 2: strict — never exceed the commit limit.
    Strict = 2,
}

impl OvercommitMode {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(OvercommitMode::Heuristic),
            1 => Some(OvercommitMode::Always),
            2 => Some(OvercommitMode::Strict),
            _ => None,
        }
    }
}

// ── Global state ─────────────────────────────────────────────────────

/// Current overcommit mode (0, 1, or 2).
static OVERCOMMIT_MODE: AtomicUsize = AtomicUsize::new(0);

/// Overcommit ratio (percentage of total physical memory usable for commitment).
/// Default 50 matches Linux's default for mode 2.
/// Only meaningful in mode 2: commit_limit = total * ratio / 100.
static OVERCOMMIT_RATIO: AtomicUsize = AtomicUsize::new(50);

/// Total physical memory in pages, set once at boot from Limine memory map.
static TOTAL_PHYSICAL_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Sum of all committed virtual pages across all processes.
/// Incremented by mmap/brk growth, decremented by munmap/brk shrink.
static COMMITTED_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Page fault denials (mode 2 rejections at fault time).
static OVERCOMMIT_FAULT_DENIALS: AtomicUsize = AtomicUsize::new(0);

/// VMA-level denials (rejected at mmap/brk before mapping).
static OVERCOMMIT_VMA_DENIALS: AtomicUsize = AtomicUsize::new(0);

// ── Initialization ───────────────────────────────────────────────────

/// Initialize the overcommit subsystem. Called once from `init_frame_allocator_limine`
/// after the Limine memory map is available.
pub fn init(total_pages: usize) {
    TOTAL_PHYSICAL_PAGES.store(total_pages, Ordering::Relaxed);
}

// ── Mode getters / setters ───────────────────────────────────────────

pub fn set_mode(mode: u8) -> Result<(), &'static str> {
    let m = OvercommitMode::from_u8(mode).ok_or("invalid overcommit mode (0, 1, or 2)")?;
    OVERCOMMIT_MODE.store(m as usize, Ordering::Relaxed);
    Ok(())
}

pub fn get_mode() -> u8 {
    OVERCOMMIT_MODE.load(Ordering::Relaxed) as u8
}

pub fn set_ratio(ratio: usize) -> Result<(), &'static str> {
    if ratio > 100 {
        return Err("overcommit ratio must be 0..100");
    }
    OVERCOMMIT_RATIO.store(ratio, Ordering::Relaxed);
    Ok(())
}

pub fn get_ratio() -> usize {
    OVERCOMMIT_RATIO.load(Ordering::Relaxed)
}

// ── Commit limit computation ─────────────────────────────────────────

/// The hard commit limit: `total_physical_pages * ratio / 100`.
/// In mode 2, committed_pages must never exceed this.
pub fn commit_limit() -> usize {
    let total = TOTAL_PHYSICAL_PAGES.load(Ordering::Relaxed);
    let ratio = OVERCOMMIT_RATIO.load(Ordering::Relaxed);
    (total * ratio) / 100
}

/// Currently committed pages (sum across all processes).
pub fn committed_pages() -> usize {
    COMMITTED_PAGES.load(Ordering::Relaxed)
}

/// Total physical pages reported at boot.
pub fn total_physical_pages() -> usize {
    TOTAL_PHYSICAL_PAGES.load(Ordering::Relaxed)
}

// ── Commit / uncommit tracking ──────────────────────────────────────

/// Record `pages` new committed virtual pages (called from mmap/brk).
/// Returns Ok(()) if the commit is permitted, Err(ENOMEM) if denied.
pub fn commit_pages(pages: usize) -> Result<(), u64> {
    let mode = get_mode();
    match mode {
        0 => {
            // Heuristic: allow if committed + requested <= total + small margin.
            // The margin is 50% of total to permit classic overcommit while
            // catching truly catastrophic exhaustion.
            let total = TOTAL_PHYSICAL_PAGES.load(Ordering::Relaxed);
            let current = COMMITTED_PAGES.load(Ordering::Relaxed);
            let margin = total / 2;
            if current.saturating_add(pages) > total.saturating_add(margin) {
                OVERCOMMIT_VMA_DENIALS.fetch_add(1, Ordering::Relaxed);
                return Err(12); // ENOMEM
            }
            COMMITTED_PAGES.fetch_add(pages, Ordering::Relaxed);
            Ok(())
        }
        1 => {
            // Always overcommit — track but never deny.
            COMMITTED_PAGES.fetch_add(pages, Ordering::Relaxed);
            Ok(())
        }
        2 => {
            // Strict: committed + requested must not exceed commit_limit.
            let limit = commit_limit();
            let current = COMMITTED_PAGES.load(Ordering::Relaxed);
            if current.saturating_add(pages) > limit {
                OVERCOMMIT_VMA_DENIALS.fetch_add(1, Ordering::Relaxed);
                return Err(12); // ENOMEM
            }
            COMMITTED_PAGES.fetch_add(pages, Ordering::Relaxed);
            Ok(())
        }
        _ => {
            // Should never happen — fall back to deny.
            Err(12)
        }
    }
}

/// Release `pages` committed virtual pages (called from munmap/brk shrink).
pub fn uncommit_pages(pages: usize) {
    let prev = COMMITTED_PAGES.fetch_sub(
        pages.min(COMMITTED_PAGES.load(Ordering::Relaxed)),
        Ordering::Relaxed,
    );
    // Double-check we didn't underflow (defensive).
    if prev < pages {
        COMMITTED_PAGES.store(0, Ordering::Relaxed);
    }
}

/// Check whether a single-page fault allocation is permitted.
/// Called from the page fault handler. Returns true if the page may be
/// allocated, false if it should be denied (SIGBUS for mode 2).
///
/// In mode 2, we re-check at fault time because:
/// 1. The VMA was already committed at mmap/brk time.
/// 2. But the actual physical page is only allocated on first fault.
/// 3. We must enforce the hard limit even for pages that were "reserved"
///    by VMA creation — prevents a process from mapping a huge range and
///    then faulting it all in.
pub fn check_fault_commit() -> bool {
    let mode = get_mode();
    if mode == 2 {
        let limit = commit_limit();
        let current = COMMITTED_PAGES.load(Ordering::Relaxed);
        if current >= limit {
            OVERCOMMIT_FAULT_DENIALS.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // Allow the fault — committed is tracked at VMA level,
        // so we just need to ensure the hard cap isn't breached.
    }
    // Modes 0 and 1: no fault-time restriction.
    true
}

// ── Statistics for /proc/meminfo and /proc/sys/vm ────────────────────

pub fn denial_stats() -> (usize, usize) {
    (
        OVERCOMMIT_VMA_DENIALS.load(Ordering::Relaxed),
        OVERCOMMIT_FAULT_DENIALS.load(Ordering::Relaxed),
    )
}

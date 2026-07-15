use core::sync::atomic::{AtomicU64, Ordering};
use alloc::collections::VecDeque;

/// Pressure levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryPressure {
    None = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// A repossession event sent to userspace.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RepossessionEvent {
    /// Type of resource to reclaim: 1=file cache, 2=anonymous pages, 3=graphics buffers
    pub resource_type: u32,
    /// Amount to reclaim in bytes
    pub amount: u64,
    /// Current pressure level
    pub pressure_level: u32,
    /// Deadline in ticks by which reclamation should happen
    pub deadline_ticks: u64,
}

/// Per-process repossession state.
pub struct RepossessionState {
    /// Whether this process has registered for repossession notifications
    pub registered: bool,
    /// Userspace callback address (signal-like handler)
    pub handler_addr: Option<u64>,
    /// Pending repossession events not yet delivered
    pub pending: VecDeque<RepossessionEvent>,
    /// Number of outstanding reclamation requests
    pub outstanding: u32,
}

impl RepossessionState {
    pub fn new() -> Self {
        RepossessionState {
            registered: false,
            handler_addr: None,
            pending: VecDeque::new(),
            outstanding: 0,
        }
    }
}

// ── Global pressure monitoring ──────────────────────────────────

static PRESSURE_LEVEL: AtomicU64 = AtomicU64::new(MemoryPressure::None as u64);
static TOTAL_FREE_PAGES_HIGH: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREE_PAGES_MED: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREE_PAGES_LOW: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREE_PAGES_CRIT: AtomicU64 = AtomicU64::new(0);

/// Initialize pressure thresholds based on total system RAM.
pub fn init(total_pages: u64) {
    TOTAL_FREE_PAGES_CRIT.store(total_pages / 100, Ordering::Relaxed);
    TOTAL_FREE_PAGES_LOW.store(total_pages / 50, Ordering::Relaxed);
    TOTAL_FREE_PAGES_MED.store(total_pages / 20, Ordering::Relaxed);
    TOTAL_FREE_PAGES_HIGH.store(total_pages / 10, Ordering::Relaxed);
}

/// Update the current pressure level based on free pages.
pub fn update_pressure(free_pages: u64) {
    let level = if free_pages <= TOTAL_FREE_PAGES_CRIT.load(Ordering::Relaxed) {
        MemoryPressure::Critical
    } else if free_pages <= TOTAL_FREE_PAGES_LOW.load(Ordering::Relaxed) {
        MemoryPressure::High
    } else if free_pages <= TOTAL_FREE_PAGES_MED.load(Ordering::Relaxed) {
        MemoryPressure::Medium
    } else if free_pages <= TOTAL_FREE_PAGES_HIGH.load(Ordering::Relaxed) {
        MemoryPressure::Low
    } else {
        MemoryPressure::None
    };
    PRESSURE_LEVEL.store(level as u64, Ordering::Relaxed);
}

/// Get current pressure level.
pub fn current_pressure() -> MemoryPressure {
    match PRESSURE_LEVEL.load(Ordering::Relaxed) {
        1 => MemoryPressure::Low,
        2 => MemoryPressure::Medium,
        3 => MemoryPressure::High,
        4 => MemoryPressure::Critical,
        _ => MemoryPressure::None,
    }
}

/// Check if pressure requires action (Medium or above).
pub fn should_reclaim() -> bool {
    current_pressure() >= MemoryPressure::Medium
}

/// Notify the current process about memory pressure.
pub fn notify_repossession() {
    let pressure = current_pressure();
    let ticks = crate::interrupts::get_ticks();
    let deadline = ticks + 50;

    let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        let mut rstate = proc.repossession.lock();
        if rstate.registered {
            let event = RepossessionEvent {
                resource_type: 1,
                amount: 4096 * 64,
                pressure_level: pressure as u32,
                deadline_ticks: deadline,
            };
            rstate.pending.push_back(event);
            rstate.outstanding += 1;
        }
    }
}

/// Check if there are pending repossession events for the current process.
pub fn check_pending() -> Option<RepossessionEvent> {
    let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        let mut rstate = proc.repossession.lock();
        rstate.pending.pop_front()
    } else {
        None
    }
}

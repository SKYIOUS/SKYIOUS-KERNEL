//! mq-deadline Block I/O Scheduler
//!
//! A multi-queue deadline-based I/O scheduler that provides:
//! - Per-device queues for parallel I/O
//! - Deadline-based fairness (reads/writes don't starve)
//! - Priority classes (real-time, best-effort, idle)
//! - Batch merging (combine adjacent I/O requests)
//!
//! ## Design
//!
//! Each block device has its own scheduler instance with:
//! - Read queue: sorted by sector, deadline-based dispatch
//! - Write queue: sorted by sector, deadline-based dispatch
//! - Priority queues: real-time requests are dispatched first
//!
//! The scheduler ensures:
//! - No request waits longer than its deadline
//! - Reads are prioritized over writes (reduces latency)
//! - Adjacent requests are merged when possible

use alloc::vec::Vec;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;

/// I/O priority classes
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IoPriority {
    /// Real-time: highest priority, dispatched immediately
    Realtime = 0,
    /// Best-effort: normal priority, deadline-based
    BestEffort = 1,
    /// Idle: only dispatched when nothing else is pending
    Idle = 2,
}

impl IoPriority {
    fn from_u32(val: u32) -> Self {
        match val {
            0 => IoPriority::Realtime,
            1 => IoPriority::BestEffort,
            _ => IoPriority::Idle,
        }
    }
}

/// I/O request type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoDirection {
    Read,
    Write,
}

/// A block I/O request
#[derive(Debug, Clone)]
pub struct IoRequest {
    /// Device ID
    pub device_id: u32,
    /// Starting sector
    pub sector: u64,
    /// Number of sectors
    pub num_sectors: u32,
    /// Direction (read/write)
    pub direction: IoDirection,
    /// Priority class
    pub priority: IoPriority,
    /// Deadline (ticks): request must be dispatched before this
    pub deadline: u64,
    /// Sequence number (for ordering)
    pub seq: u64,
    /// Buffer pointer (kernel-side)
    pub buffer: *mut u8,
    /// Callback when complete (optional)
    pub callback: Option<fn(&IoRequest, bool)>,
}

// SAFETY: IoRequest is only accessed through the scheduler's IrqSafeMutex
unsafe impl Send for IoRequest {}
unsafe impl Sync for IoRequest {}

/// Per-device I/O scheduler
pub struct DeadlineScheduler {
    /// Device ID
    device_id: u32,
    /// Read queue sorted by sector
    read_queue: VecDeque<IoRequest>,
    /// Write queue sorted by sector
    write_queue: VecDeque<IoRequest>,
    /// Deadline window (in ticks): requests older than this are dispatched ASAP
    deadline_window: u64,
    /// Base latency target (in ticks)
    base_latency: u64,
    /// Batch merge window (in sectors): merge adjacent requests within this range
    batch_merge_sectors: u64,
    /// Next sequence number
    next_seq: AtomicU64,
    /// Statistics
    stats: IoStats,
}

/// I/O scheduler statistics
#[derive(Debug, Default)]
pub struct IoStats {
    pub reads_dispatched: usize,
    pub writes_dispatched: usize,
    pub reads_merged: usize,
    pub writes_merged: usize,
    pub reads_completed: usize,
    pub writes_completed: usize,
    pub total_ticks: u64,
}

impl DeadlineScheduler {
    /// Create a new scheduler for a device.
    pub fn new(device_id: u32) -> Self {
        Self {
            device_id,
            read_queue: VecDeque::new(),
            write_queue: VecDeque::new(),
            deadline_window: 5, // 5 ticks
            base_latency: 10,   // 10ms target
            batch_merge_sectors: 128, // merge within 128 sectors (64 KB)
            next_seq: AtomicU64::new(1),
            stats: IoStats::default(),
        }
    }

    /// Submit an I/O request.
    pub fn submit(&mut self, mut request: IoRequest) {
        request.seq = self.next_seq.fetch_add(1, Ordering::Relaxed);

        // Try to merge with existing requests
        if self.try_merge(&mut request) {
            match request.direction {
                IoDirection::Read => self.stats.reads_merged += 1,
                IoDirection::Write => self.stats.writes_merged += 1,
            }
            return;
        }

        // Insert into appropriate queue sorted by sector
        let queue = match request.direction {
            IoDirection::Read => &mut self.read_queue,
            IoDirection::Write => &mut self.write_queue,
        };

        let insert_pos = queue.iter().position(|r| r.sector > request.sector)
            .unwrap_or(queue.len());
        queue.insert(insert_pos, request);
    }

    /// Try to merge a request with an adjacent one.
    fn try_merge(&mut self, request: &mut IoRequest) -> bool {
        let queue = match request.direction {
            IoDirection::Read => &mut self.read_queue,
            IoDirection::Write => &mut self.write_queue,
        };

        for existing in queue.iter_mut() {
            if existing.sector + existing.num_sectors as u64 == request.sector
                && existing.priority == request.priority
            {
                // Merge: extend existing request
                existing.num_sectors += request.num_sectors;
                return true;
            }
            if request.sector + request.num_sectors as u64 == existing.sector
                && existing.priority == request.priority
            {
                // Merge: prepend to existing request
                existing.sector = request.sector;
                existing.num_sectors += request.num_sectors;
                return true;
            }
        }
        false
    }

    /// Dispatch the next request (called by the block layer).
    pub fn dispatch(&mut self, current_tick: u64) -> Option<IoRequest> {
        // 1. Real-time requests first
        if let Some(req) = self.dispatch_by_priority(IoPriority::Realtime, current_tick) {
            return Some(req);
        }

        // 2. Check for deadline-expired requests
        let deadline = current_tick.saturating_sub(self.deadline_window);

        // Check reads first (lower latency)
        if let Some(pos) = self.read_queue.iter().position(|r| r.deadline <= deadline) {
            self.stats.reads_dispatched += 1;
            return self.read_queue.remove(pos);
        }

        // Then writes
        if let Some(pos) = self.write_queue.iter().position(|r| r.deadline <= deadline) {
            self.stats.writes_dispatched += 1;
            return self.write_queue.remove(pos);
        }

        // 3. Normal dispatch: sector-sorted, reads first
        if let Some(req) = self.read_queue.pop_front() {
            self.stats.reads_dispatched += 1;
            return Some(req);
        }

        if let Some(req) = self.write_queue.pop_front() {
            self.stats.writes_dispatched += 1;
            return Some(req);
        }

        None
    }

    /// Dispatch requests of a specific priority.
    fn dispatch_by_priority(&mut self, priority: IoPriority, _current_tick: u64) -> Option<IoRequest> {
        // Check read queue
        if let Some(pos) = self.read_queue.iter().position(|r| r.priority == priority) {
            self.stats.reads_dispatched += 1;
            return self.read_queue.remove(pos);
        }

        // Check write queue
        if let Some(pos) = self.write_queue.iter().position(|r| r.priority == priority) {
            self.stats.writes_dispatched += 1;
            return self.write_queue.remove(pos);
        }

        None
    }

    /// Mark a request as completed.
    pub fn complete(&mut self, _request: &IoRequest) {
        self.stats.reads_completed += 1;
        self.stats.writes_completed += 1;
    }

    /// Get queue depths.
    pub fn queue_depths(&self) -> (usize, usize) {
        (self.read_queue.len(), self.write_queue.len())
    }

    /// Get statistics.
    pub fn stats(&self) -> &IoStats {
        &self.stats
    }

    /// Check if queues are empty.
    pub fn is_empty(&self) -> bool {
        self.read_queue.is_empty() && self.write_queue.is_empty()
    }
}

lazy_static! {
    pub static ref BLOCK_SCHEDULERS: Mutex<Vec<DeadlineScheduler>> = Mutex::new(Vec::new());
}

/// Initialize the block I/O scheduler for a device.
pub fn scheduler_init(device_id: u32) {
    let mut schedulers = BLOCK_SCHEDULERS.lock();
    schedulers.push(DeadlineScheduler::new(device_id));
    crate::serial_write("[I/O] Deadline scheduler for device ");
    crate::serial_write(&alloc::format!("{}\n", device_id));
}

/// Submit an I/O request to the scheduler.
pub fn scheduler_submit(request: IoRequest) {
    let mut schedulers = BLOCK_SCHEDULERS.lock();
    if let Some(sched) = schedulers.iter_mut().find(|s| s.device_id == request.device_id) {
        sched.submit(request);
    }
}

/// Dispatch the next I/O request for a device.
pub fn scheduler_dispatch(device_id: u32, current_tick: u64) -> Option<IoRequest> {
    let mut schedulers = BLOCK_SCHEDULERS.lock();
    schedulers.iter_mut()
        .find(|s| s.device_id == device_id)
        .and_then(|sched| sched.dispatch(current_tick))
}

/// Get queue depths for a device.
pub fn scheduler_queue_depths(device_id: u32) -> Option<(usize, usize)> {
    let schedulers = BLOCK_SCHEDULERS.lock();
    schedulers.iter()
        .find(|s| s.device_id == device_id)
        .map(|sched| sched.queue_depths())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(sector: u64, num_sectors: u32, dir: IoDirection) -> IoRequest {
        IoRequest {
            device_id: 0,
            sector,
            num_sectors,
            direction: dir,
            priority: IoPriority::BestEffort,
            deadline: 100,
            seq: 0,
            buffer: core::ptr::null_mut(),
            callback: None,
        }
    }

    #[test]
    fn test_submit_and_dispatch() {
        let mut sched = DeadlineScheduler::new(0);
        sched.submit(make_request(100, 8, IoDirection::Read));
        sched.submit(make_request(50, 8, IoDirection::Read));

        // Should dispatch in sector order
        let req = sched.dispatch(0).unwrap();
        assert_eq!(req.sector, 50);
    }

    #[test]
    fn test_read_priority_over_write() {
        let mut sched = DeadlineScheduler::new(0);
        sched.submit(make_request(100, 8, IoDirection::Write));
        sched.submit(make_request(200, 8, IoDirection::Read));

        let req = sched.dispatch(0).unwrap();
        assert_eq!(req.direction, IoDirection::Read);
    }

    #[test]
    fn test_merge_adjacent() {
        let mut sched = DeadlineScheduler::new(0);
        sched.submit(make_request(100, 8, IoDirection::Read));
        sched.submit(make_request(108, 8, IoDirection::Read));

        // Should merge into single request
        assert_eq!(sched.read_queue.len(), 1);
        assert_eq!(sched.read_queue[0].num_sectors, 16);
    }

    #[test]
    fn test_realtime_priority() {
        let mut sched = DeadlineScheduler::new(0);
        let mut rt_req = make_request(1000, 8, IoDirection::Read);
        rt_req.priority = IoPriority::Realtime;
        sched.submit(rt_req);
        sched.submit(make_request(0, 8, IoDirection::Read));

        let req = sched.dispatch(0).unwrap();
        assert_eq!(req.priority, IoPriority::Realtime);
    }

    #[test]
    fn test_empty_scheduler() {
        let mut sched = DeadlineScheduler::new(0);
        assert!(sched.dispatch(0).is_none());
        assert!(sched.is_empty());
    }

    #[test]
    fn test_statistics() {
        let mut sched = DeadlineScheduler::new(0);
        sched.submit(make_request(100, 8, IoDirection::Read));
        sched.submit(make_request(200, 8, IoDirection::Write));

        let _ = sched.dispatch(0);
        let _ = sched.dispatch(0);

        assert_eq!(sched.stats().reads_dispatched, 1);
        assert_eq!(sched.stats().writes_dispatched, 1);
    }
}

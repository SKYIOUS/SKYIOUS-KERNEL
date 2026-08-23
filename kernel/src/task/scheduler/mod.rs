//! Preemptive priority-based round-robin scheduler.
//!
//! ## Design
//! Each CPU owns a `PerCpuScheduler` with 8 priority-sorted ready queues (level 7 =
//! highest). The LAPIC timer fires at ~100 Hz; its handler calls `try_schedule()`,
//! which picks the highest-priority ready thread and context-switches to it.
//!
//! Pick order:
//!   1. Global pending queue (newly-spawned threads — drained first so
//!      spawn()/fork()/clone() work can never be starved by an
//!      always-runnable local pair).
//!   2. Local ready queues (highest priority first, round-robin within a level).
//!   3. Work stealing from other CPUs' highest-priority queues.
//!
//! Blocked threads are tracked in global sleep/futex/pipe queues and woken by
//! `tick()` (timer) or explicit `wake_*` calls.
//!
//! ## Thread States
//! - `Ready` — in a ready queue, eligible to run.
//! - `Running` — currently executing on a CPU.
//! - `Blocked` — waiting on a pipe, futex, or sleep timer.
//! - `Exited` — finished; cleaned up on next context switch.

pub mod tick;
pub mod switch;
pub mod spawn;

// Re-export public functions at the scheduler:: level for backward compatibility.
pub use tick::tick;
pub use switch::{schedule, try_schedule};
pub use spawn::{spawn, spawn_thread, block_on_pipe, wake_pipe, wake_futex,
    wake_process_futex, wake_process_blocked, boost_thread_priority,
    add_futex_thread, with_current_thread};

use alloc::collections::{VecDeque, BinaryHeap};
use crate::sync::IrqSafeMutex as Mutex;
use crate::task::thread::{Thread, ThreadId};
use alloc::boxed::Box;
use core::sync::atomic::AtomicBool;

// ─── Statics ────────────────────────────────────────────────────

/// When set, no CPU runs the scheduler (checked by `schedule()`/`try_schedule()`).
pub static SCHED_QUIESCE: AtomicBool = AtomicBool::new(false);

pub(crate) const MAX_CPUS: usize = 8;

lazy_static::lazy_static! {
    static ref PER_CPU: alloc::vec::Vec<Mutex<PerCpuScheduler>> = {
        let mut v = alloc::vec::Vec::with_capacity(MAX_CPUS);
        for _ in 0..MAX_CPUS {
            v.push(Mutex::new(PerCpuScheduler::new()));
        }
        v
    };
}

pub static GLOBAL: GlobalScheduler = GlobalScheduler::new();

// ─── Types ──────────────────────────────────────────────────────

/// Wrapper that orders threads by ascending `pass` so BinaryHeap (a max-heap)
/// gives us the min-pass thread.
pub struct PassOrd(pub Box<Thread>);

impl PartialEq for PassOrd {
    fn eq(&self, other: &Self) -> bool { self.0.pass == other.0.pass }
}
impl Eq for PassOrd {}
impl PartialOrd for PassOrd {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for PassOrd {
    /// Reverse ordering: smallest pass = highest priority in a max-heap.
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        other.0.pass.cmp(&self.0.pass)
    }
}

/// Per-CPU scheduler: a min-heap keyed by `pass` plus the 8 legacy
/// `ready_queues` used only by `wake_*()` helpers.
pub struct PerCpuScheduler {
    /// O(log N) stride heap — primary source of next thread.
    pub stride_heap: BinaryHeap<PassOrd>,
    /// Legacy queues kept for `wake_*()` / `tick()` API compatibility.
    /// Drained into `stride_heap` during `pick_next`.
    pub ready_queues: [VecDeque<Box<Thread>>; 8],
    /// Dirty flag to track if ready_queues need flushing - avoids O(k log N) when empty
    ready_queues_dirty: bool,
    pub current_thread: Option<Box<Thread>>,
    pub dummy: u64,
    /// Thread currently being switched away from — not yet in any ready queue
    /// so other CPUs can't steal it. Pushed to ready_queues after the context
    /// switch completes (in schedule()/try_schedule()).
    pub switching_old: Option<Box<Thread>>,
    /// Permanent per-CPU idle thread.
    pub idle: Option<Box<Thread>>,
    /// ThreadId of `idle`, used to recognize a preempted idle thread.
    idle_id: Option<ThreadId>,
}

impl PerCpuScheduler {
    fn new() -> Self {
        PerCpuScheduler {
            stride_heap: BinaryHeap::new(),
            ready_queues: [
                VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new(),
                VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new(),
            ],
            ready_queues_dirty: false,
            current_thread: None,
            dummy: 0,
            switching_old: None,
            idle: None,
            idle_id: None,
        }
    }

    /// Drain any threads that `wake_*()` placed in `ready_queues` into
    /// `stride_heap` so `pick_next` sees them.
    fn flush_ready_queues(&mut self) {
        if !self.ready_queues_dirty {
            return;
        }
        for q in &mut self.ready_queues {
            while let Some(t) = q.pop_front() {
                self.stride_heap.push(PassOrd(t));
            }
        }
        self.ready_queues_dirty = false;
    }

    /// Mark ready queues as dirty - should be called when adding to ready queues
    #[allow(dead_code)]
    pub fn mark_ready_queues_dirty(&mut self) {
        self.ready_queues_dirty = true;
    }

    /// Drop all runnable state (selftest).
    #[allow(dead_code)]
    pub fn reset_runnable_state(&mut self) {
        self.stride_heap.clear();
        for q in &mut self.ready_queues { q.clear(); }
        self.ready_queues_dirty = false;
    }

    /// Push a thread directly into the stride heap.
    #[allow(dead_code)]
    pub fn push_thread(&mut self, thread: Box<Thread>) {
        self.stride_heap.push(PassOrd(thread));
    }

    /// Generic drain-and-wake: pull threads from `queue`, wake up to
    /// `max_wake` matching ones, and push non-matching ones back.
    pub(crate) fn drain_wake(
        &mut self,
        queue: &mut VecDeque<Box<Thread>>,
        max_wake: u32,
        matches: impl Fn(&Thread) -> bool,
        mut clear_block: impl FnMut(&mut Thread),
    ) -> u32 {
        let mut woken = 0u32;
        let n = queue.len();
        for _ in 0..n {
            let Some(mut thread) = queue.pop_front() else { break };
            if woken < max_wake && matches(&thread) {
                clear_block(&mut thread);
                thread.status = crate::task::thread::ThreadStatus::Ready;
                let p = (thread.priority as usize).min(7);
                self.ready_queues[p].push_back(thread);
                woken += 1;
            } else {
                queue.push_back(thread);
            }
        }
        if woken > 0 {
            self.mark_ready_queues_dirty();
        }
        woken
    }

    /// Check scheduler invariants on a snapshot of pass values.
    #[cfg(feature = "verification")]
    fn check_pick_invariants(context: &str, selected_pass: u64, passes: &[u64]) {
        if passes.is_empty() { return; }
        if let Some(min_pass) = passes.iter().min() {
            if selected_pass > *min_pass {
                let mut runner = crate::verified::runner::VERIFICATION_RUNNER.lock();
                runner.record_failure(context, &alloc::format!(
                    "SelectedNotMinPass: selected={} min={} among {} ready threads",
                    selected_pass, min_pass, passes.len()
                ));
            }
        }
        if let (Some(max_pass), Some(min_pass)) = (passes.iter().max(), passes.iter().min()) {
            let limit = crate::verified::scheduler::STRIDE_MAX.saturating_mul(2);
            if max_pass.saturating_sub(*min_pass) > limit {
                let mut runner = crate::verified::runner::VERIFICATION_RUNNER.lock();
                runner.record_failure(context, &alloc::format!(
                    "StretchViolation: max={} min={} limit={}",
                    max_pass, min_pass, limit
                ));
            }
        }
        let total_pass: u64 = passes.iter().sum();
        let max_possible = crate::interrupts::get_ticks().saturating_mul(crate::verified::scheduler::STRIDE_MAX);
        if total_pass > max_possible.saturating_mul(2) {
            let mut runner = crate::verified::runner::VERIFICATION_RUNNER.lock();
            runner.record_failure(context, &alloc::format!(
                "PassSumMismatch: sum={} >> expected max={} (ticks={})",
                total_pass, max_possible, crate::interrupts::get_ticks()
            ));
        }
        if let Some(min_pass) = passes.iter().min() {
            for pass in passes {
                let gap = pass.saturating_sub(*min_pass);
                if gap > crate::verified::scheduler::STRIDE_MAX.saturating_mul(4) {
                    let mut runner = crate::verified::runner::VERIFICATION_RUNNER.lock();
                    runner.record_failure(context, &alloc::format!(
                        "StarvationRisk: pass={} min={} gap={}", pass, min_pass, gap
                    ));
                }
            }
        }
    }

    pub fn pick_next(&mut self) -> Option<Box<Thread>> {
        self.flush_ready_queues();

        // 1. Global pending queue first
        if let Some(t) = GLOBAL.pending_queue.try_lock().and_then(|mut q| q.pop_front()) {
            return Some(t);
        }

        // 2. Stride heap
        #[cfg(feature = "verification")]
        let snapshot_passes: alloc::vec::Vec<u64> = self.stride_heap.iter().map(|p| (&*p.0).pass).collect();
        if let Some(PassOrd(t)) = self.stride_heap.pop() {
            #[cfg(feature = "verification")]
            Self::check_pick_invariants("scheduler::pick_next", t.pass, &snapshot_passes);
            return Some(t);
        }

        // 3. Work stealing
        let current_cpu = core::cmp::min(crate::smp::get_cpu_id(), MAX_CPUS - 1);
        for i in 0..MAX_CPUS {
            if i == current_cpu { continue; }
            if let Some(mut other) = PER_CPU[i].try_lock() {
                other.flush_ready_queues();
                #[cfg(feature = "verification")]
                let stolen_passes: alloc::vec::Vec<u64> = other.stride_heap.iter().map(|p| (&*p.0).pass).collect();
                if let Some(PassOrd(t)) = other.stride_heap.pop() {
                    #[cfg(feature = "verification")]
                    Self::check_pick_invariants("scheduler::pick_next::work_steal", t.pass, &stolen_passes);
                    return Some(t);
                }
            }
        }
        None
    }
}

// ─── GlobalScheduler ────────────────────────────────────────────

pub struct GlobalScheduler {
    pub pending_queue: Mutex<VecDeque<Box<Thread>>>,
    pub sleep_queue: Mutex<VecDeque<Box<Thread>>>,
    pub block_queue: Mutex<VecDeque<Box<Thread>>>,
    pub futex_queue: Mutex<VecDeque<Box<Thread>>>,
}

impl GlobalScheduler {
    const fn new() -> Self {
        GlobalScheduler {
            pending_queue: Mutex::new(VecDeque::new()),
            sleep_queue: Mutex::new(VecDeque::new()),
            block_queue: Mutex::new(VecDeque::new()),
            futex_queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn add_sleeping_thread(&self, thread: Thread) {
        self.sleep_queue.lock().push_back(Box::new(thread));
    }

    pub fn add_futex_thread(&self, thread: Thread) {
        self.futex_queue.lock().push_back(Box::new(thread));
    }

    /// Wake threads blocked on a pipe key.
    pub fn wake_blocked_threads(&self, key: u64, max_wake: u32, target_ready: &mut PerCpuScheduler) -> u32 {
        let mut block = self.block_queue.lock();
        target_ready.drain_wake(&mut block, max_wake,
            |t| t.pipe_block_key == Some(key),
            |t| { t.pipe_block_key = None; },
        )
    }

    /// Wake threads waiting on a futex.
    pub fn wake_futex(&self, uaddr: u64, max_wake: u32, target_ready: &mut PerCpuScheduler) -> u32 {
        let mut futex = self.futex_queue.lock();
        target_ready.drain_wake(&mut futex, max_wake,
            |t| t.futex_wake_addr == Some(uaddr),
            |t| { t.futex_wake_addr = None; },
        )
    }
}

// ─── Accessors ──────────────────────────────────────────────────

pub fn cpu_sched(cpu_id: usize) -> Option<&'static Mutex<PerCpuScheduler>> {
    PER_CPU.get(cpu_id)
}

pub fn this_cpu_sched() -> &'static Mutex<PerCpuScheduler> {
    let cpu_id = crate::syscalls::get_per_cpu().cpu_id as usize;
    &PER_CPU[cpu_id]
}

/// Route a thread being switched away to the queue matching its blocking
/// criterion (sleep/futex/pipe), or back to the ready queues for preempted
/// threads.
pub(crate) fn route_outgoing(s: &mut PerCpuScheduler, mut switching: Box<Thread>) {
    if switching.sleep_until.is_some() {
        GLOBAL.sleep_queue.lock().push_back(switching);
    } else if switching.futex_wake_addr.is_some() {
        GLOBAL.futex_queue.lock().push_back(switching);
    } else if switching.pipe_block_key.is_some() {
        GLOBAL.block_queue.lock().push_back(switching);
    } else if switching.status != crate::task::thread::ThreadStatus::Exited {
        switching.status = crate::task::thread::ThreadStatus::Ready;
        let p_idx = (switching.priority as usize).min(7);
        s.ready_queues[p_idx].push_back(switching);
        s.mark_ready_queues_dirty();
    } else {
        // Exited: free the dying thread's stack and address space.
        if let Some(ref proc) = switching.process {
            crate::memory::paging::AddressSpace::destroy(&proc.address_space);
        }
        crate::memory::stack::free_stack(&switching.stack);
    }
}

/// Entry point of each CPU's permanent idle thread.
extern "C" fn cpu_idle_entry() -> ! {
    loop {
        if let Some(mut sched) = this_cpu_sched().try_lock() {
            route_switching_old(&mut sched);
        }
        x86_64::instructions::interrupts::enable_and_hlt();
        switch::try_schedule();
    }
}

/// Route the thread that just switched away to the appropriate queue.
pub(crate) fn route_switching_old(s: &mut PerCpuScheduler) {
    if let Some(switching) = s.switching_old.take() {
        route_outgoing(s, switching);
    }
}

pub fn init() {
    crate::println!("Scheduler: Initializing Thread Engine...");
    GLOBAL.pending_queue.lock().reserve(64);
    GLOBAL.sleep_queue.lock().reserve(64);
    GLOBAL.block_queue.lock().reserve(64);
    GLOBAL.futex_queue.lock().reserve(64);
    for c in 0..MAX_CPUS {
        if let Some(mut sched) = PER_CPU[c].try_lock() {
            sched.stride_heap.reserve(64);
            for q in sched.ready_queues.iter_mut() {
                q.reserve(64);
            }
            let idle = Box::new(Thread::new(cpu_idle_entry));
            sched.idle_id = Some(idle._id);
            sched.idle = Some(idle);
        }
    }
}

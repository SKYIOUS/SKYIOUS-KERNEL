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

// ─── Scheduling policy constants ────────────────────────────────
pub const SCHED_NORMAL: u32 = 0;
pub const SCHED_FIFO: u32 = 1;
pub const SCHED_RR: u32 = 2;
pub const SCHED_BATCH: u32 = 3;

/// Returns true if the scheduling class is real-time (FIFO or RR).
pub fn is_realtime(class: u32) -> bool {
    class == SCHED_FIFO || class == SCHED_RR
}

/// Returns true if `candidate_class` should preempt `current_class`.
pub fn should_preempt(current_class: u32, candidate_class: u32) -> bool {
    is_realtime(candidate_class) && !is_realtime(current_class)
}

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

// ─── Per-CPU Run Queues (additive SMP infrastructure) ─────────



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

        let current_cpu = core::cmp::min(crate::smp::get_cpu_id(), MAX_CPUS - 1);

        // 1. Global pending queue first
        if let Some(t) = GLOBAL.pending_queue.try_lock().and_then(|mut q| q.pop_front()) {
            return Some(t);
        }

        // 2. RT threads have strict priority over SCHED_OTHER.
        //    Search all priority levels for RT threads (FIFO=1, RR=2).
        //    RT threads with higher rt_priority (1-99) run first.
        let mut best_level: Option<usize> = None;
        let mut best_idx: Option<usize> = None;
        let mut best_policy: u32 = 0;
        let mut best_rt_prio: u32 = 0;
        for level in (0..8).rev() {
            for idx in 0..self.ready_queues[level].len() {
                let t = &self.ready_queues[level][idx];
                if t.policy == 0 { continue; } // Not RT
                if t.affinity_mask & (1u64 << current_cpu) == 0 { continue; }
                let dominates = match best_level {
                    None => true,
                    Some(_) => {
                        if t.policy == 1 && best_policy != 1 {
                            true // FIFO dominates RR
                        } else if t.policy == best_policy {
                            t.rt_priority > best_rt_prio
                        } else {
                            false
                        }
                    }
                };
                if dominates {
                    best_level = Some(level);
                    best_idx = Some(idx);
                    best_policy = t.policy;
                    best_rt_prio = t.rt_priority;
                }
            }
        }
        if let (Some(level), Some(idx)) = (best_level, best_idx) {
            if let Some(thread) = self.ready_queues[level].remove(idx) {
                return Some(thread);
            }
        }

        // 3. SCHED_OTHER stride heap — respect CPU affinity
        #[cfg(feature = "verification")]
        let snapshot_passes: alloc::vec::Vec<u64> = self.stride_heap.iter().map(|p| (&*p.0).pass).collect();
        while let Some(PassOrd(t)) = self.stride_heap.pop() {
            if t.affinity_mask & (1u64 << current_cpu) != 0 {
                #[cfg(feature = "verification")]
                Self::check_pick_invariants("scheduler::pick_next", t.pass, &snapshot_passes);
                return Some(t);
            }
            // Thread not on this CPU — save for later reinsertion
            self.stride_heap.push(PassOrd(t));
            break;
        }

        // 4. Work stealing — steal from other CPUs' RT queues first, then SCHED_OTHER
        for i in 0..MAX_CPUS {
            if i == current_cpu { continue; }
            if let Some(mut other) = PER_CPU[i].try_lock() {
                other.flush_ready_queues();
                // Steal RT threads first — find best candidate by index
                let mut steal_level: Option<usize> = None;
                let mut steal_idx: Option<usize> = None;
                let mut steal_prio: u32 = 0;
                for level in (0..8).rev() {
                    for idx in 0..other.ready_queues[level].len() {
                        let t = &other.ready_queues[level][idx];
                        if t.policy == 0 { continue; }
                        if t.affinity_mask & (1u64 << current_cpu) == 0 { continue; }
                        if steal_level.is_none() || t.rt_priority > steal_prio {
                            steal_level = Some(level);
                            steal_idx = Some(idx);
                            steal_prio = t.rt_priority;
                        }
                    }
                }
                if let (Some(level), Some(idx)) = (steal_level, steal_idx) {
                    if let Some(thread) = other.ready_queues[level].remove(idx) {
                        return Some(thread);
                    }
                }
                // Fall back to stride heap
                #[cfg(feature = "verification")]
                let stolen_passes: alloc::vec::Vec<u64> = other.stride_heap.iter().map(|p| (&*p.0).pass).collect();
                if let Some(PassOrd(t)) = other.stride_heap.pop() {
                    if t.affinity_mask & (1u64 << current_cpu) != 0 {
                        #[cfg(feature = "verification")]
                        Self::check_pick_invariants("scheduler::pick_next::work_steal", t.pass, &stolen_passes);
                        return Some(t);
                    }
                }
            }
        }
        None
    }

    /// Periodic load balancing: redistribute threads from overloaded CPUs to
    /// underloaded ones. Called from the timer tick every 10 ticks.
    pub fn load_balance() {
        let current_cpu = core::cmp::min(crate::smp::get_cpu_id(), MAX_CPUS - 1);
        // Count runnable threads per CPU
        let mut counts = [0usize; MAX_CPUS];
        for i in 0..MAX_CPUS {
            if let Some(sched) = PER_CPU[i].try_lock() {
                counts[i] = sched.stride_heap.len();
            }
        }
        let total: usize = counts.iter().sum();
        if total < 2 { return; }
        let avg = total / MAX_CPUS;
        // If this CPU has more than avg+2 threads, try to push to a CPU with fewer
        if let Some(mut sched) = PER_CPU[current_cpu].try_lock() {
            if sched.stride_heap.len() > avg + 2 {
                for target in 0..MAX_CPUS {
                    if target == current_cpu { continue; }
                    if counts[target] >= avg { continue; }
                    if let Some(mut target_sched) = PER_CPU[target].try_lock() {
                        // Steal one non-RT thread from this CPU's stride heap.
                        // RT threads (FIFO/RR) stay on their CPU — migrating them
                        // can break latency guarantees.
                        if let Some(PassOrd(t)) = sched.stride_heap.pop() {
                            if is_realtime(t.sched_class) {
                                // Put RT thread back — don't migrate it.
                                sched.stride_heap.push(PassOrd(t));
                            } else if t.affinity_mask & (1u64 << target) != 0 {
                                target_sched.stride_heap.push(PassOrd(t));
                                target_sched.mark_ready_queues_dirty();
                            } else {
                                sched.stride_heap.push(PassOrd(t));
                            }
                            break;
                        }
                    }
                }
            }
        }
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
            // Close all file descriptors (sockets, eventfd, signalfd, etc.)
            crate::syscalls::process_lifecycle::process_close_all_fds(proc);
            // Detach shared memory segments
            crate::syscalls::shm::shm_detach_all(proc);
            // Clean up POSIX message queue descriptors
            crate::syscalls::mqueue::mq_close_all(proc.id);
            // Destroy isolate if present (it calls address_space.destroy internally)
            if let Some(ref isolate) = proc.isolate {
                isolate.destroy();
            } else {
                // No isolate — destroy address space directly
                crate::memory::paging::AddressSpace::destroy(&proc.address_space);
            }
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

/// Yield the current thread, allowing other threads to run.
pub fn yield_now() {
    if let Some(mut sched) = PER_CPU[0].try_lock() {
        if let Some(current) = sched.current_thread.as_mut() {
            current.status = crate::task::thread::ThreadStatus::Ready;
            let p_idx = (current.priority as usize).min(7);
            if let Some(mut taken) = sched.pick_next() {
                taken.status = crate::task::thread::ThreadStatus::Ready;
                let p = (taken.priority as usize).min(7);
                sched.ready_queues[p].push_back(taken);
                sched.mark_ready_queues_dirty();
            }
        }
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

//! Preemptive priority-based round-robin scheduler.
//!
//! ## Design
//! Each CPU owns a `PerCpuScheduler` with 8 priority-sorted ready queues (level 7 =
//! highest). The LAPIC timer fires at ~100 Hz; its handler calls `try_schedule()`,
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
//! ## Context Switch
//! `switch_thread()` in `thread.rs` invokes inline assembly that saves/restores
//! all callee-saved registers (r15–r12, rbp, rbx, rflags) and the stack pointer.
//! The switch is triggered either preemptively (timer IRQ) or cooperatively
//! (`try_schedule()`, `sched_yield()` syscall).
//!
//! ## Thread States
//! - `Ready` — in a ready queue, eligible to run.
//! - `Running` — currently executing on a CPU.
//! - `Blocked` — waiting on a pipe, futex, or sleep timer.
//! - `Exited` — finished; cleaned up on next context switch.

use alloc::collections::{VecDeque, BinaryHeap};
use crate::sync::IrqSafeMutex as Mutex;
use crate::task::thread::{Thread, ThreadId};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

/// When set, no CPU runs the scheduler (checked by `schedule()`/`try_schedule()`).
/// The selftest suite sets this so idle APs can't consume threads the tests
/// inject into global queues (e.g. `pending_queue`) and schedule dummy threads.
pub static SCHED_QUIESCE: AtomicBool = AtomicBool::new(false);

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
    /// Permanent per-CPU idle thread. The CPU switches here whenever nothing
    /// else is runnable and the current slot is Exited/empty/blocked (see
    /// `prepare_switch`), giving every CPU a stable context from which
    /// `try_schedule` can pick new work and a safe stack on which to reclaim
    /// an Exited thread. Never placed in any ready queue.
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
    /// `stride_heap` so `pick_next` sees them.  O(k log N) where k = newly woken.
    /// Only performs work if dirty flag is set to avoid unnecessary overhead.
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

    /// Drop all runnable state (selftest: isolates scheduler tests from
    /// threads woken by earlier tests in this CPU's queues).
    #[allow(dead_code)]
    pub fn reset_runnable_state(&mut self) {
        self.stride_heap.clear();
        for q in &mut self.ready_queues { q.clear(); }
        self.ready_queues_dirty = false;
    }

    /// Push a thread directly into the stride heap.  O(log N).
    #[allow(dead_code)]
    pub fn push_thread(&mut self, thread: Box<Thread>) {
        self.stride_heap.push(PassOrd(thread));
    }

    /// Generic drain-and-wake: pull threads from `queue`, wake up to
    /// `max_wake` matching ones (set status Ready, move to ready_queues),
    /// and push non-matching ones back. Returns the number woken.
    ///
    /// `matches` — returns true for threads that should be woken.
    /// `clear_block` — clears the per-thread field that caused the block
    ///   (e.g. `futex_wake_addr = None`).
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
    /// Records failures via the verification runner.
    #[cfg(feature = "verification")]
    fn check_pick_invariants(context: &str, selected_pass: u64, passes: &[u64]) {
        if passes.is_empty() { return; }
        // INVARIANT: selected must be min-pass
        if let Some(min_pass) = passes.iter().min() {
            if selected_pass > *min_pass {
                let mut runner = crate::verified::runner::VERIFICATION_RUNNER.lock();
                runner.record_failure(context, &alloc::format!(
                    "SelectedNotMinPass: selected={} min={} among {} ready threads",
                    selected_pass, min_pass, passes.len()
                ));
            }
        }
        // INVARIANT: stretch bound
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
        // INVARIANT: pass sum bounded
        let total_pass: u64 = passes.iter().sum();
        let max_possible = crate::interrupts::get_ticks().saturating_mul(crate::verified::scheduler::STRIDE_MAX);
        if total_pass > max_possible.saturating_mul(2) {
            let mut runner = crate::verified::runner::VERIFICATION_RUNNER.lock();
            runner.record_failure(context, &alloc::format!(
                "PassSumMismatch: sum={} >> expected max={} (ticks={})",
                total_pass, max_possible, crate::interrupts::get_ticks()
            ));
        }
        // Starvation check
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
        // Absorb anything added via ready_queues (wake paths)
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

/// Global queues shared across all CPUs.
/// Each queue has its own Mutex so operations on different queues
/// (e.g. spawn→pending vs tick→sleep) don't contend.
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
    /// Only holds block_queue lock — does not block other queues.
    pub fn wake_blocked_threads(&self, key: u64, max_wake: u32, target_ready: &mut PerCpuScheduler) -> u32 {
        let mut block = self.block_queue.lock();
        target_ready.drain_wake(&mut block, max_wake,
            |t| t.pipe_block_key == Some(key),
            |t| { t.pipe_block_key = None; },
        )
    }

    /// Wake threads waiting on a futex.
    /// Only holds futex_queue lock — does not block other queues.
    pub fn wake_futex(&self, uaddr: u64, max_wake: u32, target_ready: &mut PerCpuScheduler) -> u32 {
        let mut futex = self.futex_queue.lock();
        target_ready.drain_wake(&mut futex, max_wake,
            |t| t.futex_wake_addr == Some(uaddr),
            |t| { t.futex_wake_addr = None; },
        )
    }
}

/// Per-CPU scheduler instances (indexed by CPU ID).
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

/// Global shared queues — each has its own Mutex to minimize contention.
/// Access via `GLOBAL.pending_queue.lock()`, `GLOBAL.sleep_queue.lock()`, etc.
pub static GLOBAL: GlobalScheduler = GlobalScheduler::new();

/// Access a specific CPU's scheduler by index (for monitoring / debugging).
pub fn cpu_sched(cpu_id: usize) -> Option<&'static Mutex<PerCpuScheduler>> {
    PER_CPU.get(cpu_id)
}

/// Get the per-CPU scheduler for the current CPU.
pub fn this_cpu_sched() -> &'static Mutex<PerCpuScheduler> {
    let cpu_id = crate::syscalls::get_per_cpu().cpu_id as usize;
    &PER_CPU[cpu_id]
}

/// Route a thread being switched away to the queue matching its blocking
/// criterion (sleep/futex/pipe), or back to the ready queues for preempted
/// threads. The parent's saved registers live in its own `stack_ptr` once
/// `switch_context` runs, so the saved state travels with the Box regardless
/// of which queue it lands in.
fn route_outgoing(s: &mut PerCpuScheduler, mut switching: Box<Thread>) {
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
        // Exited: we are now on the NEXT thread's kernel stack (the switch
        // already completed), so it is safe to unmap the dying thread's stack
        // and destroy its address space.
        if let Some(ref proc) = switching.process {
            crate::memory::paging::AddressSpace::destroy(&proc.address_space);
        }
        crate::memory::stack::free_stack(&switching.stack);
        // Dropping the Box here reclaims the Thread struct.
    }
}

/// Entry point of each CPU's permanent idle thread: drain anything the
/// switch that brought us here parked (an Exited thread whose stack is
/// reclaimed on this safe stack, or a blocked thread being routed to its
/// wake queue), then hlt until an IRQ wakes us and try to pick work.
extern "C" fn cpu_idle_entry() -> ! {
    loop {
        if let Some(mut sched) = this_cpu_sched().try_lock() {
            route_switching_old(&mut sched);
        }
        x86_64::instructions::interrupts::enable_and_hlt();
        try_schedule();
    }
}

impl PerCpuScheduler {
    /// Caller MUST drop the Mutex guard BEFORE calling switch_context.
    pub fn prepare_switch(&mut self) -> Option<(*mut u64, u64)> {
        // Reclaim anything parked by an earlier switch whose post-switch
        // drain never ran: a fork/clone child iretq's to userspace without
        // running route_switching_old, and a try_schedule drain can lose the
        // try_lock. Route ALL parked threads — Exited ones are freed, the
        // rest return to the ready queue. Never drop a parked thread: its
        // saved context would be lost forever.
        if let Some(parked) = self.switching_old.take() {
            route_outgoing(&mut *self, parked);
        }
        let mut next = match self.pick_next() {
            Some(t) => t,
            None => {
                // Nothing runnable. Switch to this CPU's idle thread when
                // the current slot is Exited (a dying thread whose stack
                // cannot be freed while we are still executing on it),
                // empty (a CPU that booted with nothing to run), or a
                // futex/pipe-blocked thread that wake_*() can only reach
                // through its queue. Running / sleep-blocked sole threads
                // stay current: schedule() resumes them in place.
                let want_idle = match self.current_thread.as_ref().map(|t| t.status) {
                    None | Some(crate::task::thread::ThreadStatus::Exited) => true,
                    Some(crate::task::thread::ThreadStatus::Blocked) => {
                        let cur = self.current_thread.as_ref().unwrap();
                        cur.futex_wake_addr.is_some() || cur.pipe_block_key.is_some()
                    }
                    _ => false,
                };
                if want_idle {
                    match self.idle.take() {
                        Some(idle) => idle,
                        None => return None,
                    }
                } else {
                    return None;
                }
            }
        };
        // A fork/clone child iretq's to user space on its first scheduling
        // and never returns through the post-switch `route_switching_old`,
        // so the parent would sit forever in `switching_old`. Detect that
        // case here and route the parent directly instead.
        let child_first = next.first_switch_pending;

        // Update CURRENT_PROCESS before activating address space.
        if let Some(ref process) = next.process {
            let mut cur_proto = match crate::task::process::CURRENT_PROCESS.try_lock() {
                Some(guard) => guard,
                None => {
                    let p_idx = next.priority as usize;
                    let p_idx = if p_idx > 7 { 7 } else { p_idx };
                    self.ready_queues[p_idx].push_back(next);
                    // The thread was popped from the stride heap, so the
                    // dirty flag was already cleared by flush_ready_queues;
                    // re-mark it or pick_next will never see this thread.
                    self.mark_ready_queues_dirty();
                    return None;
                }
            };
            unsafe {
                process.address_space.activate();
            }
            *cur_proto = Some(process.clone());
        }

        next.status = crate::task::thread::ThreadStatus::Running;
        // The child is about to be switched to for the first time (or put
        // back): clear its one-shot flag so a later schedule treats it as a
        // normal thread.
        next.first_switch_pending = false;
        let new_rsp = next.stack_ptr;
        let stack_top = next.stack_top();

        let old_rsp_ptr = if let Some(mut old) = self.current_thread.take() {
            if old.status == crate::task::thread::ThreadStatus::Exited {
                // Do NOT free the dying stack/AS here: we are still executing
                // on its kernel stack until switch_context switches away, and
                // free_stack unmaps pages that our next push/return needs.
                // Park it and let route_outgoing (post-switch, new stack)
                // reclaim the memory.
                self.switching_old = Some(old);
                &raw mut self.dummy
            } else if self.idle_id == Some(old._id) {
                // The preempted idle thread: save its context and put it
                // back in the idle slot. It must never enter a ready queue
                // (pick_next would treat it as ordinary work).
                let p = &mut old.stack_ptr as *mut u64;
                old.pass = old.pass.wrapping_add(old.stride);
                self.idle = Some(old);
                p
            } else {
                let p = &mut old.stack_ptr as *mut u64;
                // Advance pass by stride (virtual time accounting)
                old.pass = old.pass.wrapping_add(old.stride);
                if child_first {
                    // The new thread never drains `switching_old`, so park
                    // the parent directly. Its regs are saved into
                    // `old.stack_ptr` by switch_context below, exactly as a
                    // preempted thread's would be.
                    route_outgoing(&mut *self, old);
                } else {
                    // Store in switching_old instead of ready_queues — the thread is still
                    // executing on this CPU until switch_context saves its registers.
                    self.switching_old = Some(old);
                }
                p
            }
        } else {
            self.switching_old = None;
            &raw mut self.dummy
        };

        self.current_thread = Some(next);
        crate::syscalls::set_kernel_stack(stack_top);
        crate::gdt::set_privilege_stack(stack_top);

        Some((old_rsp_ptr, new_rsp))
    }

    pub fn prepare_switch_tls(&mut self) -> Option<(*mut u64, u64, u64)> {
        let (old, new) = self.prepare_switch()?;
        let fs_base = self.current_thread.as_ref().map(|t| t.fs_base).unwrap_or(0);
        Some((old, new, fs_base))
    }
}

/// Route the thread that just switched away to the queue matching its
/// blocking criterion (sleep/futex/pipe), or back to the ready queues for
/// preempted threads. The context switch already saved the thread's
/// registers into its own `stack_ptr` (via `prepare_switch`), so the saved
/// state travels with the Box regardless of which queue it lands in.
///
/// Also called from `prepare_switch` for the fork/clone child case: a
/// freshly-created child iretq's to userspace without ever returning from
/// `switch_context`, so the parent parked in `switching_old` must be
/// routed then, before the slot is overwritten.
pub(crate) fn route_switching_old(s: &mut PerCpuScheduler) {
    if let Some(switching) = s.switching_old.take() {
        route_outgoing(s, switching);
    }
}

/// Main scheduler loop for each CPU.
///
/// Returns (instead of idling forever) when the current thread — blocked
/// inside a syscall — is the only runnable work and its wake condition is
/// met (or it was switched back in with nothing else to run). The resume
/// continues after the block-point `switch_thread`, so the syscall postamble
/// runs and the thread returns to userspace. Non-syscall callers (boot
/// handoff, OOM kill) append `loop { enable_and_hlt() }` so a return can
/// never fall through into invalid code.
pub fn schedule() {
    let mut watchdog_counter = 0u64;
    loop {
        if SCHED_QUIESCE.load(Ordering::Relaxed) {
            x86_64::instructions::interrupts::enable_and_hlt();
            continue;
        }
        // The prepare -> switch -> route sequence must be atomic with
        // respect to interrupts, exactly as in try_schedule: syscalls run
        // with IF=1 (SYSCALL does not clear IF), so a timer IRQ landing
        // between prepare_switch and switch_thread would re-enter the
        // scheduler and double-schedule the thread being switched to.
        let saved: u64;
        unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) saved, options(att_syntax)); }

        let (old_ptr, new_sp, new_fs) = {
            let mut s = this_cpu_sched().lock();
            s.prepare_switch_tls()
        }.map_or((core::ptr::null_mut(), 0, 0), |(a, b, c)| (a, b, c));

        if !old_ptr.is_null() {
            crate::task::thread::switch_thread(old_ptr, new_sp, new_fs);

            // Context switch completed — route the thread we switched away
            // from now that its registers are saved in its own `stack_ptr`.
            let mut s = this_cpu_sched().lock();
            route_switching_old(&mut s);
            drop(s);
        } else {
            // Nothing runnable. A syscall thread that can resume may leave
            // the scheduler; otherwise idle-wait for an IRQ to wake someone.
            let mut s = this_cpu_sched().lock();
            if let Some(cur) = s.current_thread.as_mut() {
                if cur.status == crate::task::thread::ThreadStatus::Running {
                    drop(s);
                    if saved & 0x200 != 0 {
                        unsafe { core::arch::asm!("sti"); }
                    }
                    return;
                }
                if cur.status == crate::task::thread::ThreadStatus::Blocked {
                    let time_wake = cur.sleep_until
                        .map_or(false, |t| crate::interrupts::get_ticks() >= t);
                    let sig_wake = cur.sleep_until.is_some()
                        && crate::syscalls::check_signal_interrupt();
                    if time_wake || sig_wake {
                        cur.status = crate::task::thread::ThreadStatus::Running;
                        cur.sleep_until = None;
                        drop(s);
                        if saved & 0x200 != 0 {
                            unsafe { core::arch::asm!("sti"); }
                        }
                        return;
                    }
                }
            }
            drop(s);
        }

        if saved & 0x200 != 0 {
            unsafe { core::arch::asm!("sti"); }
        }

        watchdog_counter = watchdog_counter.wrapping_add(1);
        if watchdog_counter & 0xFF == 0 {
            crate::drivers::watchdog::pet();
            crate::drivers::watchdog::check();
        }

        // Drain COW deferred deallocation queue while idle
        crate::memory::frame_info::drain_deferred();
        // sti before hlt: the idle loop can be reached from a syscall context,
        // which runs with IF=0 (syscall entry never re-enables it). A bare hlt
        // there sleeps forever - no timer, no wakeup.
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

/// Non-blocking version for interrupt handlers.
pub fn try_schedule() {
    if SCHED_QUIESCE.load(Ordering::Relaxed) {
        return;
    }
    // The prepare -> switch -> route sequence must be atomic with respect to
    // interrupts. try_schedule is called from the idle loop with IF=1, so a
    // timer IRQ can land between prepare_switch (which parks the current
    // thread and picks a new one) and switch_thread, re-entering
    // try_schedule and double-scheduling the thread we are about to switch
    // to (its context gets loaded here AND it sits in a queue). Disable
    // interrupts for the whole sequence and restore the caller's IF state.
    let saved: u64;
    unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) saved, options(att_syntax)); }

    let switch = {
        let mut s = this_cpu_sched().try_lock();
        if let Some(ref mut sched) = s {
            // Don't switch until schedule() has set up the initial idle thread.
            // Without this guard, a timer interrupt after `sti` but before
            // `schedule()` would hijack the boot stack into the DUMMY slot.
            if sched.current_thread.is_none() {
                if saved & 0x200 != 0 {
                    unsafe { core::arch::asm!("sti"); }
                }
                return;
            }
            sched.prepare_switch_tls()
        } else {
            None
        }
    };

    if let Some((old_ptr, next_ptr, new_fs)) = switch {
        crate::task::thread::switch_thread(old_ptr, next_ptr, new_fs);

        // Context switch completed — route the preempted thread now that
        // its registers are saved in its own `stack_ptr`.
        if let Some(mut sched) = this_cpu_sched().try_lock() {
            route_switching_old(&mut sched);
        }
    }

    // Restore the caller's interrupt state (may be on a different thread's
    // stack than the one that entered try_schedule — this runs in the
    // context that switch_thread returned to, i.e. the preempted thread).
    if saved & 0x200 != 0 {
        unsafe { core::arch::asm!("sti"); }
    }
}

/// Spawn a new thread, placed in the global pending pool for any CPU to pick up.
pub fn spawn(entry: extern "C" fn() -> !) {
    let thread = Thread::new(entry);
    GLOBAL.pending_queue.lock().push_back(Box::new(thread));
}

/// Add an already-constructed thread to the global pending pool.
pub fn spawn_thread(thread: Thread) {
    GLOBAL.pending_queue.lock().push_back(Box::new(thread));
}

/// Block the current thread on a pipe. Returns when woken; callers must
/// re-check their condition and re-block if it still isn't satisfied.
pub fn block_on_pipe(key: u64) {
    {
        let mut sched = this_cpu_sched().lock();
        if let Some(current) = sched.current_thread.as_mut() {
            current.status = crate::task::thread::ThreadStatus::Blocked;
            current.pipe_block_key = Some(key);
        }
    }
    schedule();
}

/// Wake all threads blocked on a pipe key.
pub fn wake_pipe(key: u64) {
    let mut sched = this_cpu_sched().lock();
    let woken = GLOBAL.wake_blocked_threads(key, u32::MAX, &mut *sched);
    if woken > 0 { broadcast_reschedule_ipi(); }
}

/// IPI handler: trigger `try_schedule()` on the receiving CPU.
/// Broadcast a reschedule IPI to all other CPUs so they pick up
/// newly-ready threads (e.g. after `wake_futex`).
pub fn broadcast_reschedule_ipi() {
    crate::smp::smp_broadcast_func(2, 0); // IpiKind::Reschedule = 2
}

/// Move current thread to sleep queue.
#[allow(dead_code)] // selftest injects threads via GLOBAL.add_sleeping_thread directly
pub fn add_sleeping_thread(thread: Thread) {
    GLOBAL.add_sleeping_thread(thread);
}

/// Add thread to futex wait queue.
#[allow(dead_code)] // only used by selftest (tests/futex_test.rs)
pub fn add_futex_thread(thread: Thread) {
    GLOBAL.add_futex_thread(thread);
}

/// Wake threads from futex wait queue.
pub fn wake_futex(uaddr: u64, max_wake: u32) -> u32 {
    let mut sched = this_cpu_sched().lock();
    let woken = GLOBAL.wake_futex(uaddr, max_wake, &mut *sched);
    if woken > 0 { broadcast_reschedule_ipi(); }
    woken
}

/// Wake all threads in the futex queue whose process ID matches.
pub fn wake_process_futex(pid: u64) -> u32 {
    let mut sched = this_cpu_sched().lock();
    let mut futex = GLOBAL.futex_queue.lock();
    let woken = sched.drain_wake(&mut futex, u32::MAX,
        |t| t.process.as_ref().map(|p| p.id == pid).unwrap_or(false),
        |t| { t.futex_wake_addr = None; },
    );
    if woken > 0 { broadcast_reschedule_ipi(); }
    woken
}

/// Wake all pipe-blocked threads whose process ID matches.
pub fn wake_process_blocked(pid: u64) -> u32 {
    let mut sched = this_cpu_sched().lock();
    let mut block = GLOBAL.block_queue.lock();
    let woken = sched.drain_wake(&mut block, u32::MAX,
        |t| t.process.as_ref().map(|p| p.id == pid).unwrap_or(false),
        |t| { t.pipe_block_key = None; },
    );
    if woken > 0 { broadcast_reschedule_ipi(); }
    woken
}

/// Boost the priority of a thread belonging to a specific process.
/// Returns true if a thread was boosted.
pub fn boost_thread_priority(_pid: u64, _target_priority: u8) -> bool {
    // ponytail: single-thread-per-process model; full priority inheritance
    // requires per-thread priority tracking across process boundaries.
    // If this kernel gains shared-memory threading, implement by scanning
    // the ready queues for the target PID and raising its priority.
    false
}

/// Process timer tick: wake sleeping threads, tick POSIX timers, ITIMER_REAL, accumulate CPU time.
///
/// Runs in IRQ context with IF=0 and can preempt any syscall mid-critical-section.
/// A blocking spin here is a permanent deadlock (the preempted holder cannot
/// run until we iret), so every lock is taken non-blocking and contended work
/// is deferred to the next tick.
pub fn tick(current_ticks: u64) {
    crate::syscalls::posix_timers::check_posix_timers();

    let mut sched = match this_cpu_sched().try_lock() {
        Some(s) => s,
        None => return,
    };

    if let Some(mut sleep) = GLOBAL.sleep_queue.try_lock() {
        // Rotate in place instead of draining into a new VecDeque: queue
        // growth allocates, and we are in IRQ context (IF=0).
        let n = sleep.len();
        let mut woken = 0u32;
        for _ in 0..n {
            let Some(mut thread) = sleep.pop_front() else { break };
            let mut wake = false;
            if let Some(wake_time) = thread.sleep_until {
                if current_ticks >= wake_time { wake = true; }
            }
            if !wake {
                if let Some(ref proc) = thread.process {
                    if let Some(sig) = proc.signals.try_lock() {
                        if sig.has_unmasked_pending(sig.blocked) { wake = true; }
                    }
                }
            }
            if wake {
                thread.status = crate::task::thread::ThreadStatus::Ready;
                thread.sleep_until = None;
                let p = (thread.priority as usize).min(7);
                sched.ready_queues[p].push_back(thread);
                woken += 1;
            } else {
                sleep.push_back(thread);
            }
        }
        if woken > 0 { sched.mark_ready_queues_dirty(); }
    }

    // Accumulate CPU time for current thread's process
    if let Some(ref cur) = sched.current_thread {
        if let Some(ref proc) = cur.process {
            proc.utime.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
    drop(sched);

    // Decrement ITIMER_REAL for every process. Fixed stack array instead of a
    // Vec: IRQ context must not allocate. ponytail: >64 pids defer to next tick.
    let mut itimer_pids: [u64; 64] = [0; 64];
    let mut itimer_count = 0usize;
    {
        let table = match crate::task::process::PROCESS_TABLE.try_lock() {
            Some(t) => t,
            None => return,
        };
        for pid in table.keys() {
            if itimer_count >= itimer_pids.len() { break; }
            itimer_pids[itimer_count] = *pid;
            itimer_count += 1;
        }
    }
    for &pid in &itimer_pids[..itimer_count] {
        // Hold the table lock and touch the process in place: a .cloned()
        // Process is a full allocation and IRQ context must not allocate.
        // All inner locks are try_lock, so this cannot deadlock.
        let table = match crate::task::process::PROCESS_TABLE.try_lock() {
            Some(t) => t,
            None => return,
        };
        let Some(proc) = table.get(&pid) else { continue };
        let mut it = match proc.itimer_real.try_lock() {
            Some(it) => it,
            None => continue,
        };
        if it.it_value.tv_sec > 0 || it.it_value.tv_usec > 0 {
            let tick_usec = 10_000u64; // 10ms per tick
            let remaining_usec = (it.it_value.tv_sec as u64) * 1_000_000 + it.it_value.tv_usec as u64;
            if remaining_usec <= tick_usec {
                // Timer expired
                it.it_value = it.it_interval; // reload
                if let Some(mut sig) = proc.signals.try_lock() {
                    sig.raise(crate::syscalls::signal::Signal::_SIGALRM);
                }
                // Wake process if blocked. Pre-flight the helper locks: we hold
                // IF=0, so once uncontended here the blocking acquires inside
                // the helpers are guaranteed not to deadlock.
                let wakeable = GLOBAL.block_queue.try_lock().is_some()
                    && GLOBAL.futex_queue.try_lock().is_some()
                    && this_cpu_sched().try_lock().is_some();
                if wakeable {
                    crate::syscalls::futex::wake_process_futex_threads(proc.id);
                    crate::syscalls::futex::wake_process_blocked_threads(proc.id);
                }
            } else {
                let new_usec = remaining_usec - tick_usec;
                it.it_value.tv_sec = (new_usec / 1_000_000) as i64;
                it.it_value.tv_usec = (new_usec % 1_000_000) as i64;
            }
        }
    }
}

/// Perform an operation on the current thread without removing it from the
/// scheduler. Interrupts are disabled for the duration so a timer handler
/// never sees `current_thread == None`.
pub fn with_current_thread<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Thread) -> R,
{
    let saved: u64;
    unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) saved, options(att_syntax)) };
    let mut sched = this_cpu_sched().lock();
    let result = sched.current_thread.as_mut().map(|t| f(&mut *t));
    drop(sched);
    if saved & 0x200 != 0 {
        unsafe { core::arch::asm!("sti") };
    }
    result
}

/// Set the current thread on this CPU (for execve/init updates).
#[allow(dead_code)]
pub fn set_current_thread(thread: Box<Thread>) {
    this_cpu_sched().lock().current_thread = Some(thread);
}

pub fn init() {
    crate::println!("Scheduler: Initializing Thread Engine...");
    // Pre-reserve queue capacity so tick()/try_schedule() never allocate:
    // they run in IRQ context, and an allocation there while the preempted
    // thread holds the global ALLOCATOR lock deadlocks the CPU (see
    // interrupts.rs note on the ALLOCATOR spinlock).
    // ponytail: reserve 64; growth past that in IRQ context is pre-existing.
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
            // Permanent per-CPU idle thread: the CPU switches here when
            // nothing else is runnable (see prepare_switch), so every CPU
            // has a stable context to pick work from and a safe stack on
            // which to reclaim Exited threads.
            let idle = Box::new(Thread::new(cpu_idle_entry));
            sched.idle_id = Some(idle._id);
            sched.idle = Some(idle);
        }
    }
}

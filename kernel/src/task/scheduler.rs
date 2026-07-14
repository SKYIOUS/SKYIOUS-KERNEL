//! Preemptive priority-based round-robin scheduler.
//!
//! ## Design
//! Each CPU owns a `PerCpuScheduler` with 8 priority-sorted ready queues (level 7 =
//! highest). The LAPIC timer fires at ~100 Hz; its handler calls `try_schedule()`,
//! which picks the highest-priority ready thread and context-switches to it.
//!
//! Pick order:
//!   1. Local ready queues (highest priority first, round-robin within a level).
//!   2. Global pending queue (newly-spawned threads).
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

use alloc::collections::VecDeque;
use spin::Mutex;
use crate::task::thread::Thread;
use alloc::boxed::Box;

/// Per-CPU scheduler: ready queues + currently running thread.
pub struct PerCpuScheduler {
    /// Ready queues retained for `tick()`/`wake_*()` API compatibility.
    /// Stride scheduling picks the min-pass thread across ALL queues,
    /// ignoring priority order — priority is expressed via tickets.
    ready_queues: [VecDeque<Box<Thread>>; 8],
    pub current_thread: Option<Box<Thread>>,
}

impl PerCpuScheduler {
    const fn new() -> Self {
        PerCpuScheduler {
            ready_queues: [
                VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new(),
                VecDeque::new(), VecDeque::new(), VecDeque::new(), VecDeque::new(),
            ],
            current_thread: None,
        }
    }

    /// Find the thread with the smallest `pass` value across all ready queues
    /// (stride scheduling). Linear scan — O(N) in ready-thread count.
    fn stride_min_pass<'a>(queues: &'a mut [VecDeque<Box<Thread>>; 8]) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize, u64)> = None; // (q_idx, pos, pass)
        for (qi, q) in queues.iter().enumerate() {
            for (pi, t) in q.iter().enumerate() {
                let pass = t.pass;
                match best {
                    Some((_, _, bp)) if pass < bp => best = Some((qi, pi, pass)),
                    None => best = Some((qi, pi, pass)),
                    _ => {}
                }
            }
        }
        best.map(|(q, p, _)| (q, p))
    }

    pub fn pick_next(&mut self) -> Option<Box<Thread>> {
        // 1. Stride: find min-pass thread across all local queues
        if let Some((qidx, pidx)) = Self::stride_min_pass(&mut self.ready_queues) {
            let t = self.ready_queues[qidx].remove(pidx).unwrap();
            return Some(t);
        }

        // 2. Try global pending queue
        if let Some(t) = GLOBAL.pending_queue.lock().pop_front() {
            return Some(t);
        }

        // 3. Work Stealing: try to steal from other CPUs
        let current_cpu = core::cmp::min(crate::smp::get_cpu_id(), MAX_CPUS - 1);
        for i in 0..MAX_CPUS {
            if i == current_cpu { continue; }
            if let Some(mut other_sched) = PER_CPU[i].try_lock() {
                // Steal the min-pass thread from the other CPU
                if let Some((qidx, pidx)) = Self::stride_min_pass(&mut other_sched.ready_queues) {
                    let t = other_sched.ready_queues[qidx].remove(pidx).unwrap();
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
        let mut woken = 0u32;
        let mut still_waiting = VecDeque::new();
        while let Some(mut thread) = block.pop_front() {
            if woken < max_wake && thread.pipe_block_key == Some(key) {
                thread.status = crate::task::thread::ThreadStatus::Ready;
                thread.pipe_block_key = None;
                let p = if thread.priority > 7 { 7 } else { thread.priority };
                target_ready.ready_queues[p as usize].push_back(thread);
                woken += 1;
            } else {
                still_waiting.push_back(thread);
            }
        }
        *block = still_waiting;
        woken
    }

    /// Wake threads waiting on a futex.
    /// Only holds futex_queue lock — does not block other queues.
    pub fn wake_futex(&self, uaddr: u64, max_wake: u32, target_ready: &mut PerCpuScheduler) -> u32 {
        let mut futex = self.futex_queue.lock();
        let mut woken = 0u32;
        let mut still_waiting = VecDeque::new();
        while let Some(mut thread) = futex.pop_front() {
            if woken < max_wake && thread.futex_wake_addr == Some(uaddr) {
                thread.status = crate::task::thread::ThreadStatus::Ready;
                thread.futex_wake_addr = None;
                let p = if thread.priority > 7 { 7 } else { thread.priority };
                target_ready.ready_queues[p as usize].push_back(thread);
                woken += 1;
            } else {
                still_waiting.push_back(thread);
            }
        }
        *futex = still_waiting;
        woken
    }
}

/// Per-CPU scheduler instances (indexed by CPU ID).
const MAX_CPUS: usize = 8;
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

impl PerCpuScheduler {
    /// Caller MUST drop the Mutex guard BEFORE calling switch_context.
    pub fn prepare_switch(&mut self) -> Option<(*mut u64, u64)> {
        let mut next = self.pick_next()?;

        // Update CURRENT_PROCESS before activating address space.
        if let Some(ref process) = next.process {
            let mut cur_proto = match crate::task::process::CURRENT_PROCESS.try_lock() {
                Some(guard) => guard,
                None => {
                    let p_idx = next.priority as usize;
                    let p_idx = if p_idx > 7 { 7 } else { p_idx };
                    self.ready_queues[p_idx].push_back(next);
                    return None;
                }
            };
            unsafe {
                process.address_space.activate();
            }
            *cur_proto = Some(process.clone());
        }

        next.status = crate::task::thread::ThreadStatus::Running;
        let new_rsp = next.stack_ptr;
        let stack_top = next.stack_top();

        let old_rsp_ptr = if let Some(mut old) = self.current_thread.take() {
            if old.status == crate::task::thread::ThreadStatus::Exited {
                if let Some(ref proc) = old.process {
                    crate::memory::paging::AddressSpace::destroy(&proc.address_space);
                }
                crate::memory::stack::free_stack(&old.stack);
                static mut EXIT_DUMMY: u64 = 0;
                &raw mut EXIT_DUMMY
            } else {
                old.status = crate::task::thread::ThreadStatus::Ready;
                let p = &mut old.stack_ptr as *mut u64;
                // Advance pass by stride (virtual time accounting)
                old.pass = old.pass.wrapping_add(old.stride);
                let p_idx = old.priority as usize;
                let p_idx = if p_idx > 7 { 7 } else { p_idx };
                self.ready_queues[p_idx].push_back(old);
                p
            }
        } else {
            static mut DUMMY: u64 = 0;
            &raw mut DUMMY
        };

        self.current_thread = Some(next);
        crate::syscalls::set_kernel_stack(stack_top);

        Some((old_rsp_ptr, new_rsp))
    }

    pub fn prepare_switch_tls(&mut self) -> Option<(*mut u64, u64, u64)> {
        let (old, new) = self.prepare_switch()?;
        let fs_base = self.current_thread.as_ref().map(|t| t.fs_base).unwrap_or(0);
        Some((old, new, fs_base))
    }
}

/// Main scheduler loop for each CPU.
pub fn schedule() -> ! {
    let mut watchdog_counter = 0u64;
    loop {
        let (old_ptr, new_sp, new_fs) = {
            let mut s = this_cpu_sched().lock();
            s.prepare_switch_tls()
        }.map_or((core::ptr::null_mut(), 0, 0), |(a, b, c)| (a, b, c));

        if !old_ptr.is_null() {
            crate::task::thread::switch_thread(old_ptr, new_sp, new_fs);
        }

        watchdog_counter = watchdog_counter.wrapping_add(1);
        if watchdog_counter & 0xFF == 0 {
            crate::drivers::watchdog::pet();
            crate::drivers::watchdog::check();
        }

        // Drain COW deferred deallocation queue while idle
        crate::memory::frame_info::drain_deferred();
        x86_64::instructions::hlt();
    }
}

/// Non-blocking version for interrupt handlers.
pub fn try_schedule() {
    let switch = {
        let mut s = this_cpu_sched().try_lock();
        if let Some(ref mut sched) = s {
            // Don't switch until schedule() has set up the initial idle thread.
            // Without this guard, a timer interrupt after `sti` but before
            // `schedule()` would hijack the boot stack into the DUMMY slot.
            if sched.current_thread.is_none() {
                return;
            }
            sched.prepare_switch_tls()
        } else {
            None
        }
    };

    if let Some((old_ptr, next_ptr, new_fs)) = switch {
        crate::task::thread::switch_thread(old_ptr, next_ptr, new_fs);
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

/// Block the current thread on a pipe.
pub fn block_on_pipe(key: u64) {
    let mut sched = this_cpu_sched().lock();
    if let Some(mut current) = sched.current_thread.take() {
        current.status = crate::task::thread::ThreadStatus::Blocked;
        current.pipe_block_key = Some(key);
        GLOBAL.block_queue.lock().push_back(current);
    }
    drop(sched);
    schedule();
}

/// Wake all threads blocked on a pipe key.
pub fn wake_pipe(key: u64) {
    let mut sched = this_cpu_sched().lock();
    let woken = GLOBAL.wake_blocked_threads(key, u32::MAX, &mut *sched);
    if woken > 0 { broadcast_reschedule_ipi(); }
}

/// IPI handler: trigger `try_schedule()` on the receiving CPU.
/// Called from interrupt context (IpiFunc vector 251).
extern "C" fn ipi_reschedule_handler(_arg: u64) {
    try_schedule();
}

/// Broadcast a reschedule IPI to all other CPUs so they pick up
/// newly-ready threads (e.g. after `wake_futex`).
pub fn broadcast_reschedule_ipi() {
    crate::smp::smp_broadcast_func(ipi_reschedule_handler as extern "C" fn(u64), 0);
}

/// Move current thread to sleep queue.
pub fn add_sleeping_thread(thread: Thread) {
    GLOBAL.add_sleeping_thread(thread);
}

/// Add thread to futex wait queue.
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
    let mut woken = 0u32;
    let mut still_waiting = alloc::collections::VecDeque::new();
    while let Some(mut thread) = futex.pop_front() {
        let matches = thread.process.as_ref().map(|p| p.id == pid).unwrap_or(false);
        if matches {
            thread.status = crate::task::thread::ThreadStatus::Ready;
            thread.futex_wake_addr = None;
            let p = if thread.priority > 7 { 7 } else { thread.priority };
            sched.ready_queues[p as usize].push_back(thread);
            woken += 1;
        } else {
            still_waiting.push_back(thread);
        }
    }
    *futex = still_waiting;
    if woken > 0 { broadcast_reschedule_ipi(); }
    woken
}

/// Wake all pipe-blocked threads whose process ID matches.
pub fn wake_process_blocked(pid: u64) -> u32 {
    let mut sched = this_cpu_sched().lock();
    let mut block = GLOBAL.block_queue.lock();
    let mut woken = 0u32;
    let mut still_waiting = alloc::collections::VecDeque::new();
    while let Some(mut thread) = block.pop_front() {
        let matches = thread.process.as_ref().map(|p| p.id == pid).unwrap_or(false);
        if matches {
            thread.status = crate::task::thread::ThreadStatus::Ready;
            thread.pipe_block_key = None;
            let p = if thread.priority > 7 { 7 } else { thread.priority };
            sched.ready_queues[p as usize].push_back(thread);
            woken += 1;
        } else {
            still_waiting.push_back(thread);
        }
    }
    *block = still_waiting;
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

/// Process timer tick: wake sleeping threads. Non-blocking for interrupt context.
pub fn tick(current_ticks: u64) {
    if let Some(mut sched) = this_cpu_sched().try_lock() {
        if let Some(mut sleep) = GLOBAL.sleep_queue.try_lock() {
            let mut still_sleeping = VecDeque::new();
            while let Some(mut thread) = sleep.pop_front() {
                let mut wake = false;
                if let Some(wake_time) = thread.sleep_until {
                    if current_ticks >= wake_time { wake = true; }
                }
                if !wake {
                    if let Some(ref proc) = thread.process {
                        let sig = proc.signals.lock();
                        if sig.has_unmasked_pending(sig.blocked) { wake = true; }
                    }
                }
                if wake {
                    thread.status = crate::task::thread::ThreadStatus::Ready;
                    thread.sleep_until = None;
                    let p = (thread.priority as usize).min(7);
                    sched.ready_queues[p].push_back(thread);
                } else {
                    still_sleeping.push_back(thread);
                }
            }
            *sleep = still_sleeping;
        }
    }
}

/// Get the current thread on this CPU (for execve/init updates).
pub fn current_thread() -> Option<Box<Thread>> {
    this_cpu_sched().lock().current_thread.take()
}

/// Set the current thread on this CPU (for execve/init updates).
pub fn set_current_thread(thread: Box<Thread>) {
    this_cpu_sched().lock().current_thread = Some(thread);
}

pub fn init() {
    crate::println!("Scheduler: Initializing Thread Engine...");
}

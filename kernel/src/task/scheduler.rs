use alloc::collections::VecDeque;
use spin::Mutex;
use crate::task::thread::Thread;

use alloc::boxed::Box;

/// Per-CPU scheduler: ready queues + currently running thread.
pub struct PerCpuScheduler {
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

    pub fn pick_next(&mut self) -> Option<Box<Thread>> {
        // 1. Try local ready queues (High priority first)
        for i in (0..8).rev() {
            if let Some(t) = self.ready_queues[i].pop_front() {
                return Some(t);
            }
        }

        // 2. Try global pending queue
        if let Some(mut global) = GLOBAL.try_lock() {
            if let Some(t) = global.pending_queue.pop_front() {
                return Some(t);
            }
        }

        // 3. Work Stealing: try to steal from other CPUs
        let current_cpu = crate::smp::get_cpu_id();
        for i in 0..MAX_CPUS {
            if i == current_cpu { continue; }
            if let Some(mut other_sched) = PER_CPU[i].try_lock() {
                // Steal from the highest priority non-empty queue
                for prio in (0..8).rev() {
                    if let Some(t) = other_sched.ready_queues[prio].pop_back() {
                        return Some(t);
                    }
                }
            }
        }
        None
    }
}

/// Global queues shared across all CPUs.
pub struct GlobalScheduler {
    pub pending_queue: VecDeque<Box<Thread>>,
    pub sleep_queue: VecDeque<Box<Thread>>,
    pub block_queue: VecDeque<Box<Thread>>,
    pub futex_queue: VecDeque<Box<Thread>>,
}

impl GlobalScheduler {
    const fn new() -> Self {
        GlobalScheduler {
            pending_queue: VecDeque::new(),
            sleep_queue: VecDeque::new(),
            block_queue: VecDeque::new(),
            futex_queue: VecDeque::new(),
        }
    }

    pub fn add_sleeping_thread(&mut self, thread: Thread) {
        self.sleep_queue.push_back(Box::new(thread));
    }

    pub fn add_futex_thread(&mut self, thread: Thread) {
        self.futex_queue.push_back(Box::new(thread));
    }

    /// Check sleep queue, move woken threads into `target_ready`.
    pub fn tick(&mut self, current_ticks: u64, target_ready: &mut PerCpuScheduler) {
        let mut still_sleeping = VecDeque::new();
        while let Some(mut thread) = self.sleep_queue.pop_front() {
            if let Some(wake_time) = thread.sleep_until {
                if current_ticks >= wake_time {
                    thread.status = crate::task::thread::ThreadStatus::Ready;
                    thread.sleep_until = None;
                    let priority = thread.priority as usize;
                    let p = if priority > 7 { 7 } else { priority };
                    target_ready.ready_queues[p].push_back(thread);
                    continue;
                }
            }
            still_sleeping.push_back(thread);
        }
        self.sleep_queue = still_sleeping;
    }

    /// Wake threads blocked on a pipe key.
    pub fn wake_blocked_threads(&mut self, key: u64, max_wake: u32, target_ready: &mut PerCpuScheduler) -> u32 {
        let mut woken = 0u32;
        let mut still_waiting = VecDeque::new();
        while let Some(mut thread) = self.block_queue.pop_front() {
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
        self.block_queue = still_waiting;
        woken
    }

    /// Wake threads waiting on a futex.
    pub fn wake_futex(&mut self, uaddr: u64, max_wake: u32, target_ready: &mut PerCpuScheduler) -> u32 {
        let mut woken = 0u32;
        let mut still_waiting = VecDeque::new();
        while let Some(mut thread) = self.futex_queue.pop_front() {
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
        self.futex_queue = still_waiting;
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

/// Global shared queues.
pub static GLOBAL: Mutex<GlobalScheduler> = Mutex::new(GlobalScheduler::new());

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

        // Check watchdog every ~256 iterations (~2.5s at 100Hz)
        watchdog_counter = watchdog_counter.wrapping_add(1);
        if watchdog_counter & 0xFF == 0 {
            crate::drivers::watchdog::pet();
            crate::drivers::watchdog::check();
        }

        x86_64::instructions::hlt();
    }
}

/// Non-blocking version for interrupt handlers.
pub fn try_schedule() {
    let switch = {
        let mut s = this_cpu_sched().try_lock();
        if let Some(ref mut sched) = s {
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
    GLOBAL.lock().pending_queue.push_back(Box::new(thread));
}

/// Add an already-constructed thread to the global pending pool.
pub fn spawn_thread(thread: Thread) {
    GLOBAL.lock().pending_queue.push_back(Box::new(thread));
}

/// Block the current thread on a pipe.
pub fn block_on_pipe(key: u64) {
    let mut sched = this_cpu_sched().lock();
    if let Some(mut current) = sched.current_thread.take() {
        current.status = crate::task::thread::ThreadStatus::Blocked;
        current.pipe_block_key = Some(key);
        let mut global = GLOBAL.lock();
        global.block_queue.push_back(current);
        drop(global);
    }
    drop(sched);
    schedule();
}

/// Wake all threads blocked on a pipe key.
pub fn wake_pipe(key: u64) {
    let mut sched = this_cpu_sched().lock();
    let mut global = GLOBAL.lock();
    global.wake_blocked_threads(key, u32::MAX, &mut *sched);
}

/// Move current thread to sleep queue.
pub fn add_sleeping_thread(thread: Thread) {
    GLOBAL.lock().add_sleeping_thread(thread);
}

/// Add thread to futex wait queue.
pub fn add_futex_thread(thread: Thread) {
    GLOBAL.lock().add_futex_thread(thread);
}

/// Wake threads from futex wait queue.
pub fn wake_futex(uaddr: u64, max_wake: u32) -> u32 {
    let mut sched = this_cpu_sched().lock();
    let mut global = GLOBAL.lock();
    global.wake_futex(uaddr, max_wake, &mut *sched)
}

/// Process timer tick: wake sleeping threads. Non-blocking for interrupt context.
pub fn tick(current_ticks: u64) {
    if let Some(mut sched) = this_cpu_sched().try_lock() {
        if let Some(mut global) = GLOBAL.try_lock() {
            global.tick(current_ticks, &mut *sched);
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

//! Thread spawning, blocking, wake helpers, and utility functions.

use super::{this_cpu_sched, GLOBAL, schedule};

/// Spawn a new thread, placed in the global pending pool for any CPU to pick up.
pub fn spawn(entry: extern "C" fn() -> !) {
    let thread = crate::task::thread::Thread::new(entry);
    GLOBAL.pending_queue.lock().push_back(alloc::boxed::Box::new(thread));
}

/// Add an already-constructed thread to the global pending pool.
pub fn spawn_thread(thread: crate::task::thread::Thread) {
    GLOBAL.pending_queue.lock().push_back(alloc::boxed::Box::new(thread));
}

/// Block the current thread on a pipe. Returns when woken.
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

/// Broadcast a reschedule IPI to all other CPUs.
pub fn broadcast_reschedule_ipi() {
    crate::smp::smp_broadcast_func(2, 0); // IpiKind::Reschedule = 2
}

/// Move current thread to sleep queue.
#[allow(dead_code)]
pub fn add_sleeping_thread(thread: crate::task::thread::Thread) {
    GLOBAL.add_sleeping_thread(thread);
}

/// Add thread to futex wait queue.
#[allow(dead_code)]
pub fn add_futex_thread(thread: crate::task::thread::Thread) {
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
#[allow(dead_code)]
pub fn boost_thread_priority(_pid: u64, _target_priority: u8) -> bool {
    false
}

/// Perform an operation on the current thread without removing it from the
/// scheduler. Interrupts are disabled for the duration.
pub fn with_current_thread<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut crate::task::thread::Thread) -> R,
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
pub fn set_current_thread(thread: alloc::boxed::Box<crate::task::thread::Thread>) {
    this_cpu_sched().lock().current_thread = Some(thread);
}

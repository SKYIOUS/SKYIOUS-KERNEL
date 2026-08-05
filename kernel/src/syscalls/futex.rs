use hashbrown::HashMap;
use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;
use crate::task::process::CURRENT_PROCESS;
use crate::task::scheduler;
use crate::syscalls::errno;

lazy_static! {
    /// Tracks PI futex ownership: uaddr -> (owner_pid, original_priority)
    static ref PI_OWNERS: Mutex<HashMap<u64, (u64, u8)>> = Mutex::new(HashMap::new());
}

/// Wake all threads blocked on any futex belonging to a given process.
pub fn wake_process_futex_threads(pid: u64) -> u32 {
    scheduler::wake_process_futex(pid)
}

/// Wake all pipe-blocked threads belonging to a given process.
pub fn wake_process_blocked_threads(pid: u64) -> u32 {
    scheduler::wake_process_blocked(pid)
}

/// Core futex syscall dispatch — signal-interruptible.
pub fn sys_futex(uaddr: *mut u32, op: u32, val: u32, _val2: u32, uaddr2: *mut u32, _val3: u32) -> u64 {
    const FUTEX_WAIT:  u32 = 0;
    const FUTEX_WAKE:  u32 = 1;
    const FUTEX_REQUEUE: u32 = 3;
    const FUTEX_LOCK_PI:   u32 = 11;
    const FUTEX_UNLOCK_PI: u32 = 12;

    match op {
        FUTEX_WAIT  => futex_wait(uaddr, val),
        FUTEX_WAKE  => scheduler::wake_futex(uaddr as u64, val) as u64,
        FUTEX_REQUEUE => {
            let woken = scheduler::wake_futex(uaddr as u64, val);
            if woken > 0 { scheduler::wake_futex(uaddr2 as u64, _val2); }
            woken as u64
        }
        FUTEX_LOCK_PI   => futex_lock_pi(uaddr),
        FUTEX_UNLOCK_PI => futex_unlock_pi(uaddr),
        _ => errno::Errno::ENOSYS as u64,
    }
}

/// Check whether a signal is pending for the current process.
fn signal_pending() -> bool {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map(|p| p.signals.lock().has_pending()).unwrap_or(false)
}

/// FUTEX_WAIT — if a signal is pending, return EINTR.
fn futex_wait(uaddr: *mut u32, expected: u32) -> u64 {
    let current_val = unsafe { core::ptr::read_volatile(uaddr) };
    if current_val != expected {
        return errno::Errno::EAGAIN as u64;
    }

    // Signal check BEFORE we give up the CPU.
    // After blocking, the scheduler reclaims this thread's context;
    // the thread will never execute code after schedule(). Signals
    // that arrive while blocked are handled by wake_process_futex()
    // → the thread resumes from its last try_schedule() preemption
    // point, and the syscall-return signal check (do_syscall exit)
    // will deliver any pending signal.
    if signal_pending() {
        return errno::Errno::EINTR as u64;
    }

    // Mark the current thread Blocked in place. The block-point context is
    // saved into the thread's own `stack_ptr` by `prepare_switch`, so the
    // resume (after `schedule()` returns) continues the syscall postamble.
    {
        let mut sched = scheduler::this_cpu_sched().lock();
        if let Some(current) = sched.current_thread.as_mut() {
            current.status = crate::task::thread::ThreadStatus::Blocked;
            current.futex_wake_addr = Some(uaddr as u64);
        }
    }
    scheduler::schedule();
    0
}

/// FUTEX_LOCK_PI — acquire a PI-aware futex, boosting the owner's priority.
fn futex_lock_pi(uaddr: *mut u32) -> u64 {
    const FUTEX_WAITERS: u32 = 0x8000_0000;
    const PID_MASK:     u32 = 0x7FFF_FFFF;

    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id as u32).unwrap_or(0)
    };
    if pid == 0 { return errno::Errno::ESRCH as u64; }

    // Fast path: cmpxchg 0 → pid
    let prev = unsafe { core::ptr::read_volatile(uaddr) };
    if prev == 0 {
        let mut old = 0u32;
        unsafe {
            core::arch::asm!(
                "lock cmpxchgl {new:e}, ({addr})",
                addr = in(reg) uaddr,
                new  = in(reg) pid,
                inout("eax") old,
                options(att_syntax, nostack),
            );
        }
        if old == 0 {
            let prio = scheduler::this_cpu_sched().lock().current_thread
                .as_ref().map(|t| t.priority).unwrap_or(3);
            PI_OWNERS.lock().insert(uaddr as u64, (pid as u64, prio));
            return 0;
        }
    }

    // Lock held. Set FUTEX_WAITERS bit if missing.
    let current = unsafe { core::ptr::read_volatile(uaddr) };
    if current & FUTEX_WAITERS == 0 {
        unsafe { core::ptr::write_volatile(uaddr, current | FUTEX_WAITERS); }
    }

    // Priority inheritance: if the waiter is higher priority, boost the owner.
    let owner_pid = current & PID_MASK;
    if owner_pid != 0 {
        let our_prio = scheduler::this_cpu_sched().lock().current_thread
            .as_ref().map(|t| t.priority).unwrap_or(3);
        scheduler::boost_thread_priority(owner_pid as u64, our_prio);
    }

    if signal_pending() { return errno::Errno::EINTR as u64; }

    // Mark the current thread Blocked in place; see futex_wait.
    {
        let mut sched = scheduler::this_cpu_sched().lock();
        if let Some(current) = sched.current_thread.as_mut() {
            current.status = crate::task::thread::ThreadStatus::Blocked;
            current.futex_wake_addr = Some(uaddr as u64);
        }
    }
    scheduler::schedule();
    0
}

/// FUTEX_UNLOCK_PI — release a PI futex, waking waiters.
fn futex_unlock_pi(uaddr: *mut u32) -> u64 {
    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id as u32).unwrap_or(0)
    };
    if pid == 0 { return errno::Errno::ESRCH as u64; }

    let prev = unsafe { core::ptr::read_volatile(uaddr) };
    if prev & 0x7FFF_FFFF != pid {
        return errno::Errno::EPERM as u64;
    }

    if prev & 0x8000_0000 != 0 {
        unsafe { core::ptr::write_volatile(uaddr, 0); }
        scheduler::wake_futex(uaddr as u64, 1);
    } else {
        unsafe { core::ptr::write_volatile(uaddr, 0); }
    }

    PI_OWNERS.lock().remove(&(uaddr as u64));
    0
}
use crate::sync::IrqSafeMutex as Mutex;
use crate::syscalls::errno;
use crate::syscalls::user_access;
use crate::task::process::CURRENT_PROCESS;
use crate::task::scheduler;
use hashbrown::HashMap;
use lazy_static::lazy_static;

/// Read the futex word from userspace through the fault-aware copy helper.
/// A kernel or unmapped address is refused before any dereference.
fn read_user_u32(uaddr: *mut u32) -> Result<u32, ()> {
    let mut raw = [0u8; core::mem::size_of::<u32>()];
    // SAFETY: copy_from_user validates the user range and page faults inside
    // the copy are redirected to the exception-table fixup, which reports
    // failure instead of faulting in ring 0.
    unsafe { user_access::copy_from_user(&mut raw, uaddr as *const u8)? };
    Ok(u32::from_ne_bytes(raw))
}

// Atomic compare-and-swap of a user word. SysV argument order:
// rdi = uaddr, esi = expected, edx = new, rcx = prev_out. Returns 0 after
// writing the previous word to `prev_out`; a fault is redirected by the
// page-fault handler to the user-copy fixup (`mov eax, 1; ret`), which
// returns 1 and leaves `prev_out` untouched.
#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    r#"
    .global futex_user_cas
    futex_user_cas:
        mov eax, esi
        lock cmpxchg dword ptr [rdi], edx
        mov dword ptr [rcx], eax
        xor eax, eax
        ret
    "#
);

#[cfg(target_arch = "x86_64")]
extern "C" {
    fn futex_user_cas(uaddr: *mut u32, expected: u32, new: u32, prev_out: *mut u32) -> u32;
}

/// True when CR4.SMAP is active. `stac`/`clac` are illegal without it.
#[cfg(target_arch = "x86_64")]
fn smap_enabled() -> bool {
    use x86_64::registers::control::{Cr4, Cr4Flags};
    Cr4::read().contains(Cr4Flags::SUPERVISOR_MODE_ACCESS_PREVENTION)
}

/// Atomically compare-and-swap the futex word at `uaddr`.
///
/// Returns the word's previous value in both the swapped and not-swapped
/// case, or `Err(())` if the address is not user memory or the access faulted.
#[cfg(target_arch = "x86_64")]
fn user_cas_u32(uaddr: *mut u32, expected: u32, new: u32) -> Result<u32, ()> {
    use core::sync::atomic::Ordering;

    if !user_access::validate_ptr(uaddr as *const u8, core::mem::size_of::<u32>()) {
        return Err(());
    }
    // Probe the mapping through the fault-aware copy path first: an unmapped
    // page is refused without entering the atomic. The page can still be
    // unmapped after this point; the nest count below makes that fault abort
    // instead of panicking.
    read_user_u32(uaddr)?;

    let mut prev = 0u32;
    let smap = smap_enabled();
    let per_cpu = crate::syscalls::get_per_cpu();
    if smap {
        // SAFETY: stac clears SMAP's AC flag for this CPU only; clac runs on
        // both the normal return and the page-fault fixup return below.
        unsafe { core::arch::asm!("stac", options(nomem, nostack, preserves_flags)) };
    }
    per_cpu.user_copy_nest.fetch_add(1, Ordering::Relaxed);
    // SAFETY: `uaddr` is validated user memory, the SysV argument order
    // matches the leaf, and the leaf touches exactly four bytes at `uaddr`.
    // SMAP is off and `user_copy_nest` is raised, so a fault there is
    // redirected to the user-copy fixup (returns 1) instead of panicking.
    let failed = unsafe { futex_user_cas(uaddr, expected, new, &mut prev) };
    per_cpu.user_copy_nest.fetch_sub(1, Ordering::Relaxed);
    if smap {
        // SAFETY: restores the AC flag that stac cleared above.
        unsafe { core::arch::asm!("clac", options(nomem, nostack, preserves_flags)) };
    }
    if failed != 0 {
        return Err(());
    }
    Ok(prev)
}

/// Non-x86 fallback: no SMAP and no fault fixup in the atomic path, so the
/// CAS runs directly through `AtomicU32` after a user-range and alignment
/// check.
#[cfg(not(target_arch = "x86_64"))]
fn user_cas_u32(uaddr: *mut u32, expected: u32, new: u32) -> Result<u32, ()> {
    use core::sync::atomic::{AtomicU32, Ordering};

    if !user_access::validate_ptr(uaddr as *const u8, core::mem::size_of::<u32>())
        || !(uaddr as usize).is_multiple_of(core::mem::align_of::<u32>())
    {
        return Err(());
    }
    // SAFETY: the range is user memory and the pointer is 4-byte aligned, so
    // it satisfies AtomicU32's validity contract.
    let word = unsafe { &*(uaddr as *const AtomicU32) };
    Ok(word
        .compare_exchange(expected, new, Ordering::AcqRel, Ordering::Acquire)
        .unwrap_or_else(|actual| actual))
}

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
pub fn sys_futex(
    uaddr: *mut u32,
    op: u32,
    val: u32,
    _val2: u32,
    uaddr2: *mut u32,
    _val3: u32,
) -> u64 {
    const FUTEX_WAIT: u32 = 0;
    const FUTEX_WAKE: u32 = 1;
    const FUTEX_REQUEUE: u32 = 3;
    const FUTEX_LOCK_PI: u32 = 11;
    const FUTEX_UNLOCK_PI: u32 = 12;

    match op {
        FUTEX_WAIT => futex_wait(uaddr, val),
        FUTEX_WAKE => scheduler::wake_futex(uaddr as u64, val) as u64,
        FUTEX_REQUEUE => {
            let woken = scheduler::wake_futex(uaddr as u64, val);
            if woken > 0 {
                scheduler::wake_futex(uaddr2 as u64, _val2);
            }
            woken as u64
        }
        FUTEX_LOCK_PI => futex_lock_pi(uaddr),
        FUTEX_UNLOCK_PI => futex_unlock_pi(uaddr),
        _ => errno::Errno::ENOSYS as u64,
    }
}

/// Check whether a signal is pending for the current process.
fn signal_pending() -> bool {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref()
        .map(|p| p.signals.lock().has_pending())
        .unwrap_or(false)
}

/// FUTEX_WAIT — if a signal is pending, return EINTR.
fn futex_wait(uaddr: *mut u32, expected: u32) -> u64 {
    let current_val = match read_user_u32(uaddr) {
        Ok(v) => v,
        Err(()) => return errno::Errno::EFAULT as u64,
    };
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
    const PID_MASK: u32 = 0x7FFF_FFFF;

    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id as u32).unwrap_or(0)
    };
    if pid == 0 {
        return errno::Errno::ESRCH as u64;
    }

    // Fast path: claim an unlocked word with a single atomic CAS.
    let mut current = match user_cas_u32(uaddr, 0, pid) {
        Ok(prev) => prev,
        Err(()) => return errno::Errno::EFAULT as u64,
    };
    if current == 0 {
        let prio = scheduler::this_cpu_sched()
            .lock()
            .current_thread
            .as_ref()
            .map(|t| t.priority)
            .unwrap_or(3);
        PI_OWNERS.lock().insert(uaddr as u64, (pid as u64, prio));
        return 0;
    }

    // Lock held: set FUTEX_WAITERS atomically. Retry on a concurrent update;
    // if the owner released in the meantime, try to acquire the word instead.
    loop {
        if current & PID_MASK == 0 {
            match user_cas_u32(uaddr, 0, pid) {
                Ok(0) => {
                    let prio = scheduler::this_cpu_sched()
                        .lock()
                        .current_thread
                        .as_ref()
                        .map(|t| t.priority)
                        .unwrap_or(3);
                    PI_OWNERS.lock().insert(uaddr as u64, (pid as u64, prio));
                    return 0;
                }
                Ok(prev) => {
                    current = prev;
                    if current & PID_MASK == 0 {
                        // Stale FUTEX_WAITERS with no owner: retrying would
                        // spin, and no owner exists to boost. Fall through to
                        // the wait path (the pre-CAS behavior for this word).
                        break;
                    }
                    continue;
                }
                Err(()) => return errno::Errno::EFAULT as u64,
            }
        }
        if current & FUTEX_WAITERS != 0 {
            break;
        }
        match user_cas_u32(uaddr, current, current | FUTEX_WAITERS) {
            Ok(prev) => {
                if prev == current {
                    break;
                }
                current = prev;
            }
            Err(()) => return errno::Errno::EFAULT as u64,
        }
    }

    // Priority inheritance: if the waiter is higher priority, boost the owner.
    let owner_pid = current & PID_MASK;
    if owner_pid != 0 {
        let our_prio = scheduler::this_cpu_sched()
            .lock()
            .current_thread
            .as_ref()
            .map(|t| t.priority)
            .unwrap_or(3);
        scheduler::boost_thread_priority(owner_pid as u64, our_prio);
    }

    if signal_pending() {
        return errno::Errno::EINTR as u64;
    }

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
    if pid == 0 {
        return errno::Errno::ESRCH as u64;
    }

    let mut prev = match read_user_u32(uaddr) {
        Ok(v) => v,
        Err(()) => return errno::Errno::EFAULT as u64,
    };
    loop {
        if prev & 0x7FFF_FFFF != pid {
            return errno::Errno::EPERM as u64;
        }
        // Clear the word only if it still holds the value we inspected: a
        // waiter may have set FUTEX_WAITERS in the meantime.
        match user_cas_u32(uaddr, prev, 0) {
            Ok(previous) if previous == prev => {
                if prev & 0x8000_0000 != 0 {
                    scheduler::wake_futex(uaddr as u64, 1);
                }
                PI_OWNERS.lock().remove(&(uaddr as u64));
                return 0;
            }
            Ok(actual) => prev = actual,
            Err(()) => return errno::Errno::EFAULT as u64,
        }
    }
}

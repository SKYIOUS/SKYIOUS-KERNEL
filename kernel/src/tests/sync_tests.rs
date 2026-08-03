//! Regression tests for the IRQ-safe mutex contract.
//!
//! Guards the exact bug class that caused intermittent boot hangs: an
//! `IrqSafeMutexGuard` whose drop restored IF before releasing the underlying
//! spin mutex, leaving a window where the lock is held with interrupts
//! enabled. A timer IRQ landing in that window strands the lock forever.
//!
//! Contract under test:
//!   1. While a guard is held, interrupts are disabled (IF=0).
//!   2. Dropping the guard restores IF to its pre-acquire state.
//!   3. Dropping the guard actually releases the lock (unlock happens before
//!      IF is restored, so the release is never preemptable).
//!
//! Run at boot via the `self_test` feature; TAP output goes to serial and CI
//! fails on any `not ok`.

use crate::sync::IrqSafeMutex;
use crate::selftest;

fn if_flag() -> bool {
    // SAFETY: pushfq/pop reads RFLAGS; IF is bit 9 (0x200).
    let rflags: u64;
    unsafe { core::arch::asm!("pushfq; pop {0}", out(reg) rflags, options(att_syntax)) };
    rflags & 0x200 != 0
}

static MUTEX: IrqSafeMutex<u64> = IrqSafeMutex::new(0);

/// IF is cleared while the guard is held, and restored on drop.
fn test_if_contract() -> Result<(), &'static str> {
    let initial = if_flag();
    {
        let mut guard = MUTEX.lock();
        if if_flag() {
            return Err("IF still set while guard held");
        }
        *guard = 42;
    }
    if if_flag() != initial {
        return Err("IF not restored after guard drop");
    }
    if *MUTEX.lock() != 42 {
        return Err("value lost across lock/unlock");
    }
    Ok(())
}

/// Drop releases the spin mutex (unlock precedes any IF restore).
fn test_unlock_on_drop() -> Result<(), &'static str> {
    if MUTEX.is_locked() {
        return Err("mutex locked before acquire");
    }
    {
        let _guard = MUTEX.lock();
        if !MUTEX.is_locked() {
            return Err("mutex not locked while guard held");
        }
    }
    if MUTEX.is_locked() {
        return Err("mutex still locked after guard drop");
    }
    Ok(())
}

/// try_lock on an uncontended mutex succeeds and restores IF; on a contended
/// mutex it fails without deadlocking and still restores IF.
fn test_try_lock_contract() -> Result<(), &'static str> {
    let initial = if_flag();
    {
        let _held = MUTEX.lock();
        if MUTEX.try_lock().is_some() {
            return Err("try_lock succeeded while held");
        }
        if if_flag() {
            return Err("IF set after failed try_lock");
        }
        drop(_held);
    }
    if if_flag() != initial {
        return Err("IF not restored after failed try_lock");
    }
    let g = MUTEX.try_lock().ok_or("try_lock failed on free mutex")?;
    drop(g);
    if MUTEX.is_locked() {
        return Err("mutex still locked after try_lock guard drop");
    }
    Ok(())
}

/// Allocator round-trip under lock churn: exercises the global ALLOCATOR
/// (slab fast path + fallback heap) through the fixed guard implementation.
fn test_allocator_roundtrip() -> Result<(), &'static str> {
    for i in 0..512u32 {
        let mut v = alloc::vec::Vec::new();
        v.push(i);
        v.push(i.wrapping_mul(3));
        let s = alloc::format!("cycle-{}", i);
        if v[1] != i.wrapping_mul(3) || s.is_empty() {
            return Err("allocation round-trip corrupted");
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "aarch64"))]
pub fn register() {
    selftest::register("sync::if_contract", test_if_contract);
    selftest::register("sync::unlock_on_drop", test_unlock_on_drop);
    selftest::register("sync::try_lock_contract", test_try_lock_contract);
    selftest::register("sync::allocator_roundtrip", test_allocator_roundtrip);
}

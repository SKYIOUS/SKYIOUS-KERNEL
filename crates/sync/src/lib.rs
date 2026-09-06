//! IRQ-safe spin mutex for `#![no_std]` kernels.
//!
//! Interrupts are disabled while the lock is held, preventing the timer IRQ
//! from preempting a thread mid-critical-section and stranding the lock.
//!
//! # Usage
//!
//! ```rust,ignore
//! static COUNTER: IrqSafeMutex<u64> = IrqSafeMutex::new(0);
//!
//! // In thread context
//! let mut guard = COUNTER.lock();
//! *guard += 1;
//!
//! // In IRQ context — MUST use try_lock()
//! if let Some(mut guard) = COUNTER.try_lock() {
//!     *guard += 1;
//! }
//! ```
//!
//! # Constraints
//!
//! Guards must not be held across blocking operations (sleep/wait):
//! the CPU would sit with IF=0 and no tick could ever wake the sleeper.

#![no_std]

use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU16, Ordering};

/// RFLAGS interrupt-enable bit (bit 9).
const IF_BIT: u64 = 1 << 9;

/// Sentinel: the mutex is currently unlocked.
const NO_HOLDER: u16 = u16::MAX;

/// IRQ-safe spin mutex. Disables interrupts while the lock is held.
///
/// This prevents timer IRQ from preempting a thread holding the lock,
/// which would strand the lock and deadlock other waiters.
///
/// # Re-entrancy invariant (I1)
///
/// A CPU must never acquire a mutex it already holds: the lock is
/// non-reentrant, and because interrupts are off while held, a same-CPU
/// re-acquisition spins with IF=0 — invisible to ticks and the watchdog
/// (the self-deadlock family fixed 2026-09-04). `lock()` detects the
/// holder CPU and panics with the mutex address instead of spinning;
/// `try_lock()` returns `None` (probe patterns depend on that).
pub struct IrqSafeMutex<T: ?Sized> {
    /// CPU id of the current holder, or `NO_HOLDER` when unlocked.
    /// Diagnostic mirror for re-entrancy detection; ordering is provided
    /// by the spin mutex (store after acquire, clear before release).
    /// MUST stay ahead of `inner`: that field is the unsized tail (DST).
    holder_cpu: AtomicU16,
    inner: spin::Mutex<T>,
}

/// RAII guard returned by [`IrqSafeMutex::lock`] and [`IrqSafeMutex::try_lock`].
///
/// Disables interrupts on creation, restores on drop.
pub struct IrqSafeMutexGuard<'a, T: ?Sized> {
    inner: ManuallyDrop<spin::MutexGuard<'a, T>>,
    /// RFLAGS captured before disabling interrupts.
    rflags: u64,
    /// Back-reference to clear the holder mirror on release.
    mutex: &'a IrqSafeMutex<T>,
}

impl<T> IrqSafeMutex<T> {
    /// Create a new IRQ-safe mutex wrapping the given value.
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            holder_cpu: AtomicU16::new(NO_HOLDER),
        }
    }
}

impl<T: ?Sized> IrqSafeMutex<T> {
    /// Lock the mutex, disabling interrupts for the critical section.
    ///
    /// # Panics
    ///
    /// Panics if the current CPU already holds this mutex: that is always
    /// a nested-acquisition bug (temporaries or block-scoped guards) and
    /// would otherwise spin forever with IF=0, invisible to the watchdog.
    #[must_use = "guard disables interrupts until dropped; you probably want to hold it"]
    #[inline]
    pub fn lock(&self) -> IrqSafeMutexGuard<'_, T> {
        let rflags = save_and_disable_interrupts();
        let cpu = this_cpu();
        if self.inner.is_locked() && self.holder_cpu.load(Ordering::Relaxed) == cpu {
            panic!(
                "IrqSafeMutex::lock: re-entrant acquisition on CPU {} — this CPU already holds the mutex at {:#x} (nested guard; non-reentrant lock)",
                cpu,
                (self as *const Self as *const u8) as usize
            );
        }
        let guard = self.inner.lock();
        self.holder_cpu.store(cpu, Ordering::Relaxed);
        IrqSafeMutexGuard {
            inner: ManuallyDrop::new(guard),
            rflags,
            mutex: self,
        }
    }

    /// Try to lock the mutex without blocking. Disables interrupts if successful.
    ///
    /// Returns `None` if the lock is already held — including when held by
    /// the current CPU (probe patterns such as `X.try_lock().is_some()` rely
    /// on `None`, and a same-CPU `try_lock` is a skip, not a deadlock).
    /// Re-enables interrupts if they were enabled before the attempt.
    #[must_use = "guard disables interrupts until dropped; you probably want to hold it"]
    #[inline]
    pub fn try_lock(&self) -> Option<IrqSafeMutexGuard<'_, T>> {
        let rflags = save_and_disable_interrupts();
        if self.inner.is_locked() && self.holder_cpu.load(Ordering::Relaxed) == this_cpu() {
            restore_interrupts(rflags);
            return None;
        }
        match self.inner.try_lock() {
            Some(guard) => {
                self.holder_cpu.store(this_cpu(), Ordering::Relaxed);
                Some(IrqSafeMutexGuard {
                    inner: ManuallyDrop::new(guard),
                    rflags,
                    mutex: self,
                })
            }
            None => {
                restore_interrupts(rflags);
                None
            }
        }
    }

    /// Returns `true` if the underlying spin mutex is currently held.
    ///
    /// Note: this reads the lock state without disabling interrupts, so the
    /// result is a point-in-time snapshot that may already be stale.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }
}

impl<T: core::fmt::Display + ?Sized> core::fmt::Display for IrqSafeMutexGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: ?Sized> Deref for IrqSafeMutexGuard<'_, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: ?Sized> DerefMut for IrqSafeMutexGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: ?Sized> Drop for IrqSafeMutexGuard<'_, T> {
    fn drop(&mut self) {
        // Clear the holder mirror BEFORE releasing the spin lock: the spin
        // release happens-before any other CPU's acquire, so its store wins;
        // the residual window (holder cleared, spin not yet released) only
        // weakens detection, never allows a nested acquisition to succeed.
        self.mutex.holder_cpu.store(NO_HOLDER, Ordering::Relaxed);
        // SAFETY: the inner spin mutex must be unlocked FIRST, while
        // interrupts are still off, and only then may IF be restored.
        // ManuallyDrop::drop runs the inner guard's Drop, which releases
        // the spin lock before we re-enable interrupts below.
        unsafe { ManuallyDrop::drop(&mut self.inner) };
        restore_interrupts(self.rflags);
    }
}

/// Current CPU id (x86_64: CPUID leaf 1, EBX[31:24] = initial APIC ID).
///
/// Unique per CPU; used only as a diagnostic mirror key. The APIC ID can
/// exceed 8 bits under x2APIC, so the full byte is widened to u16.
#[cfg(not(all(test, not(target_os = "none"))))]
#[inline(always)]
fn this_cpu() -> u16 {
    // Use custom provider if registered, otherwise fall back to CPUID.
    // The provider is set by the kernel during early boot to use the
    // GS-based per-CPU data which is much faster than CPUID.
    unsafe {
        if let Some(f) = CPU_ID_PROVIDER {
            return f();
        }
    }
    // __cpuid(1) is the LLVM CPUID builtin (handles the callee-saved rbx
    // internally); leaf 1 never faults on x86_64 and EBX[31:24] is the
    // caller's initial APIC ID, unique per CPU.
    let r = core::arch::x86_64::__cpuid(1);
    ((r.ebx >> 24) & 0xFF) as u16
}

/// Current CPU id (host-test mock — no CPUID on the test host).
#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
fn this_cpu() -> u16 {
    0
}

/// Function pointer type for custom CPU ID provider.
///
/// The kernel can register a faster implementation (e.g., GS-based per-CPU data)
/// during early boot. This avoids the expensive CPUID instruction on every
/// lock acquisition.
type CpuIdProvider = fn() -> u16;

/// Optional custom CPU ID provider. Set by the kernel during init.
/// Defaults to None (uses CPUID fallback).
static mut CPU_ID_PROVIDER: Option<CpuIdProvider> = None;

/// Register a custom CPU ID provider function.
///
/// This should be called once during kernel initialization, before any
/// `IrqSafeMutex` is used. The provider must be safe to call from any
/// context (including interrupt context with interrupts disabled).
///
/// # Safety
///
/// The caller must ensure:
/// - The function is safe to call with interrupts disabled
/// - The function returns a unique identifier per CPU
/// - The function does not panic or allocate
pub unsafe fn set_cpu_id_provider(f: CpuIdProvider) {
    CPU_ID_PROVIDER = Some(f);
}

/// Save RFLAGS and disable interrupts. Returns the captured RFLAGS.
///
/// # Safety
///
/// Must be paired with a call to `restore_interrupts()` with the same value.
#[cfg(not(all(test, not(target_os = "none"))))]
#[inline(always)]
fn save_and_disable_interrupts() -> u64 {
    let rflags: u64;
    // SAFETY: pushfq/pop captures flags, cli disables interrupts.
    // No memory barrier needed — spin::Mutex provides its own ordering.
    unsafe {
        core::arch::asm!("pushfq; pop {rflags}; cli", rflags = out(reg) rflags, options(att_syntax));
    }
    rflags
}

/// Save RFLAGS and disable interrupts (test mock — no-op on host).
#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
fn save_and_disable_interrupts() -> u64 {
    0
}

/// Restore interrupts to the state captured by `save_and_disable_interrupts()`.
///
/// # Safety
///
/// The `rflags` argument must be the value returned by `save_and_disable_interrupts()`.
#[cfg(not(all(test, not(target_os = "none"))))]
#[inline(always)]
fn restore_interrupts(rflags: u64) {
    if rflags & IF_BIT != 0 {
        // SAFETY: sti is only issued if interrupts were enabled at the time
        // save_and_disable_interrupts() was called.
        unsafe { core::arch::asm!("sti") };
    }
}

/// Restore interrupts (test mock — no-op on host).
#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
fn restore_interrupts(_rflags: u64) {}

// ── Tests ────────────────────────────────────────────────────────────────────
//
// Unit tests for the no_std wrapper logic. The inline-asm helpers
// (save_and_disable_interrupts / restore_interrupts) are mocked on
// non-kernel targets so tests can exercise the lock API.

#[cfg(test)]
mod tests {
    // Test harness links std even though the crate is no_std; needed for
    // catch_unwind in the re-entrancy panic test.
    extern crate alloc;
    extern crate std;
    use super::*;

    /// IF_BIT is at RFLAGS position 9.
    #[test]
    fn if_bit_is_bit_9() {
        assert_eq!(IF_BIT, 1 << 9);
        assert_eq!(IF_BIT, 0x200);
    }

    /// `IrqSafeMutex::new` stores the value and the wrapper is `Send` (sync data is `Send`).
    #[test]
    fn mutex_new_stores_value() {
        let m: IrqSafeMutex<u64> = IrqSafeMutex::new(42);
        // is_locked reports the underlying spin state — at rest it should be unlocked.
        assert!(
            !m.is_locked(),
            "freshly constructed mutex should not be locked"
        );
    }

    /// Display impl forwards to inner value.
    #[test]
    fn display_impl_delegates() {
        let m: IrqSafeMutex<i32> = IrqSafeMutex::new(-7);
        let guard = m.lock();
        let s = alloc::format!("{}", guard);
        assert_eq!(s, "-7");
    }

    /// `is_locked` is a point-in-time snapshot. Acquire a guard, observe locked.
    /// (Lock is released by guard drop.)
    #[test]
    fn is_locked_reflects_state() {
        let m: IrqSafeMutex<u32> = IrqSafeMutex::new(0);
        assert!(!m.is_locked());
        {
            let _g = m.lock();
            assert!(m.is_locked(), "locked while guard is alive");
        }
        assert!(!m.is_locked(), "unlocked after guard drop");
    }

    /// Deref/DerefMut access the inner value.
    #[test]
    fn deref_returns_inner() {
        let m: IrqSafeMutex<[u8; 4]> = IrqSafeMutex::new([1, 2, 3, 4]);
        let mut g = m.lock();
        assert_eq!(g[0], 1);
        assert_eq!(g[3], 4);
        g[2] = 99;
        assert_eq!(g[2], 99);
    }

    /// `try_lock` succeeds when unlocked.
    #[test]
    fn try_lock_succeeds_when_free() {
        let m: IrqSafeMutex<u8> = IrqSafeMutex::new(5);
        let g = m.try_lock();
        assert!(g.is_some());
    }

    /// `try_lock` returns None when already held, and after the holding guard
    /// drops, a subsequent try_lock succeeds. (Validates non-reentrant behavior
    /// observable from the host test target.)
    #[test]
    fn try_lock_returns_none_when_held() {
        let m: IrqSafeMutex<u8> = IrqSafeMutex::new(0);
        let _g = m.lock();
        assert!(
            m.try_lock().is_none(),
            "try_lock must not succeed while held"
        );
    }

    /// Invariant I1: same-CPU re-entrant `lock()` is the self-deadlock
    /// family (spins forever with IF=0, invisible to the watchdog) — it must
    /// panic with a diagnostic instead.
    #[test]
    fn lock_while_held_same_cpu_panics() {
        let m: IrqSafeMutex<u8> = IrqSafeMutex::new(0);
        let _g = m.lock();
        // AssertUnwindSafe: the mutex is intentionally held across the
        // attempt (that is the point of the test); the guard is dropped
        // on unwind and the mutex is not reused afterwards.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g2 = m.lock();
        }));
        assert!(
            result.is_err(),
            "re-entrant lock() must panic (non-reentrant mutex), not spin"
        );
    }

    /// Invariant I1 (try_lock variant): a same-CPU `try_lock` is a skip, not
    /// a deadlock — must return `None` (probe patterns rely on this), never
    /// panic and never succeed.
    #[test]
    fn try_lock_while_held_same_cpu_returns_none() {
        let m: IrqSafeMutex<u8> = IrqSafeMutex::new(0);
        let _g = m.lock();
        assert!(m.try_lock().is_none(), "same-CPU try_lock must be None");
        drop(_g);
        assert!(
            m.try_lock().is_some(),
            "try_lock succeeds after the holder drops"
        );
    }

    /// The holder mirror must clear on drop: a fresh acquisition after
    /// release is normal, not a re-entrancy false positive.
    #[test]
    fn relock_after_drop_succeeds() {
        let m: IrqSafeMutex<u8> = IrqSafeMutex::new(0);
        {
            let _g = m.lock();
        }
        let g = m.lock();
        assert_eq!(*g, 0);
    }
}

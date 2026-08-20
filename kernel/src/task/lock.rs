//! Sleep-wake lock: a Mutex that blocks (yields the scheduler) on contention
//! instead of spinning. Uses the scheduler's pipe-blocking infrastructure.
//!
//! ## Trade-offs
//! - Contested acquires are ~µs (context switch) vs ns (spin), but the other
//!   thread makes progress instead of burning a CPU.
//! - Must NOT be used from interrupt context (block_on_pipe requires a thread).
//! - Fairness ≈ FIFO (wake_pipe wakes in insertion order).

use core::sync::atomic::{AtomicU64, Ordering::*};
use core::cell::UnsafeCell;

/// A mutex that blocks rather than spins when contended.
/// Used by the VFS global lock (vfs/mod.rs). Deploy on BUDDY_ALLOCATOR or the
/// compositor when contention is measured via profiling (add a counter to
/// SchedLock::lock slow path).
#[allow(dead_code)]
pub struct SchedLock<T> {
    held: AtomicU64,       // 0 = free, 1 = held
    key: AtomicU64,        // unique pipe-block key
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for SchedLock<T> {}
unsafe impl<T: Send> Sync for SchedLock<T> {}

#[allow(dead_code)]
static NEXT_LOCK_KEY: AtomicU64 = AtomicU64::new(0x1000_0000_0000);

#[allow(dead_code)]
impl<T> SchedLock<T> {
    pub const fn new(val: T) -> Self {
        SchedLock {
            held: AtomicU64::new(0),
            key: AtomicU64::new(0),
            data: UnsafeCell::new(val),
        }
    }

    pub fn new_named(val: T) -> Self {
        SchedLock {
            held: AtomicU64::new(0),
            key: AtomicU64::new(NEXT_LOCK_KEY.fetch_add(1, Relaxed)),
            data: UnsafeCell::new(val),
        }
    }

    fn key(&self) -> u64 {
        let k = self.key.load(Relaxed);
        if k == 0 {
            let new = NEXT_LOCK_KEY.fetch_add(1, Relaxed);
            match self.key.compare_exchange(0, new, Relaxed, Relaxed) {
                Ok(_) => new,
                Err(actual) => actual,
            }
        } else {
            k
        }
    }

    pub fn lock(&self) -> SchedLockGuard<'_, T> {
        // Fast path: try once
        if self.held.swap(1, Acquire) == 0 {
            return SchedLockGuard { lock: self, data: unsafe { &mut *self.data.get() } };
        }
        // Slow path: block until we acquire
        loop {
            crate::task::scheduler::block_on_pipe(self.key());
            if self.held.swap(1, Acquire) == 0 {
                return SchedLockGuard { lock: self, data: unsafe { &mut *self.data.get() } };
            }
        }
    }

    /// Non-blocking try_lock
    pub fn try_lock(&self) -> Option<SchedLockGuard<'_, T>> {
        if self.held.swap(1, Acquire) == 0 {
            Some(SchedLockGuard { lock: self, data: unsafe { &mut *self.data.get() } })
        } else {
            None
        }
    }
}

pub struct SchedLockGuard<'a, T> {
    lock: &'a SchedLock<T>,
    data: &'a mut T,
}

impl<T> core::ops::Deref for SchedLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.data }
}

impl<T> core::ops::DerefMut for SchedLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T { self.data }
}

impl<T> Drop for SchedLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.held.store(0, Release);
        // Wake one waiter (if any) — use wake_pipe with max_wake=1
        crate::task::scheduler::wake_pipe(self.lock.key());
    }
}

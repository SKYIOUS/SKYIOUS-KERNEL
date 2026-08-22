//! RCU (Read-Copy-Update) synchronization primitive.
//!
//! RCU allows multiple readers to access shared data concurrently without locking.
//! Writers create a copy, modify it, then atomically swap the pointer. Readers
//! that started before the swap continue reading the old data until they call
//! `rcu_read_unlock()`, at which point the old data can be safely freed.
//!
//! This implementation is designed for a uniprocessor or lightly SMP kernel:
//! - `rcu_read_lock()` disables preemption (no actual lock)
//! - `synchronize_rcu()` waits for a grace period (all CPUs passed through quiescent state)
//! - `call_rcu()` registers a callback to run after the grace period

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use crate::sync::IrqSafeMutex;

/// Global RCU state
static RCU_STATE: RcuState = RcuState::new();

/// Per-CPU quiescent state counter
static mut CPU_QUIESCENT: [u64; 64] = [0; 64];

/// RCU state tracking
pub struct RcuState {
    /// Global GP (grace period) counter — incremented each completed grace period
    pub gp_counter: AtomicUsize,
    /// Number of CPUs currently in RCU read-side critical section
    pub reader_count: AtomicUsize,
    /// Pending callbacks to run after grace period
    pub callbacks: IrqSafeMutex<Vec<RcuCallback>>,
    /// Grace period in progress flag
    pub gp_in_progress: AtomicBool,
}

/// A callback to execute after RCU grace period
pub struct RcuCallback {
    /// The callback function pointer
    pub func: unsafe extern "C" fn(*mut u8),
    /// Argument to pass to the callback
    pub arg: *mut u8,
}

// SAFETY: RcuCallback is only accessed through the global RCU_STATE mutex
unsafe impl Send for RcuCallback {}
unsafe impl Sync for RcuCallback {}

impl RcuState {
    const fn new() -> Self {
        Self {
            gp_counter: AtomicUsize::new(0),
            reader_count: AtomicUsize::new(0),
            callbacks: IrqSafeMutex::new(Vec::new()),
            gp_in_progress: AtomicBool::new(false),
        }
    }
}

/// Enter an RCU read-side critical section.
///
/// Disables preemption to ensure the current thread cannot be preempted
/// during the critical section. This is the "read lock" — it has near-zero
/// overhead since it just prevents context switches.
pub fn rcu_read_lock() {
    // Disable preemption by incrementing the reader count.
    // In a uniprocessor kernel, this prevents context switches during RCU reads.
    // In SMP, we'd need to also track per-CPU nesting.
    RCU_STATE.reader_count.fetch_add(1, Ordering::SeqCst);
    
    // Disable interrupts to prevent preemption (matching IrqSafeMutex pattern)
    x86_64::instructions::interrupts::disable();
}

/// Exit an RCU read-side critical section.
pub fn rcu_read_unlock() {
    RCU_STATE.reader_count.fetch_sub(1, Ordering::SeqCst);
    
    // Re-enable interrupts
    x86_64::instructions::interrupts::enable();
}

/// Wait for a grace period to elapse.
///
/// A grace period ensures that all CPUs have passed through a quiescent state
/// (no RCU readers active). After this returns, it's safe to free old data.
pub fn synchronize_rcu() {
    // Wait for all current readers to finish
    while RCU_STATE.reader_count.load(Ordering::SeqCst) > 0 {
        core::hint::spin_loop();
    }
    
    // Additional barrier: ensure we see the latest state
    core::sync::atomic::fence(Ordering::SeqCst);
    
    // Increment the GP counter to signal completion
    RCU_STATE.gp_counter.fetch_add(1, Ordering::SeqCst);
}

/// Register a callback to be invoked after the current grace period.
///
/// The callback will be called with the given argument once all CPUs have
/// passed through a quiescent state.
pub fn call_rcu(callback: unsafe extern "C" fn(*mut u8), arg: *mut u8) {
    // Register with CFI so the callback can be validated when executed
    crate::sync::cfi::cfi_register_target(callback as usize);
    let cb = RcuCallback { func: callback, arg };
    RCU_STATE.callbacks.lock().push(cb);
}

/// Process pending RCU callbacks.
///
/// Should be called periodically (e.g., from the timer tick) to execute
/// callbacks whose grace period has elapsed.
pub fn rcu_process_callbacks() {
    let cbs = {
        let mut callbacks = RCU_STATE.callbacks.lock();
        if callbacks.is_empty() {
            return;
        }
        // Take all pending callbacks
        let taken: Vec<RcuCallback> = callbacks.drain(..).collect();
        taken
    };
    
    // Wait for grace period
    synchronize_rcu();
    
    // Execute all callbacks (CFI validated)
    for cb in cbs {
        if crate::sync::cfi::cfi_check(cb.func as usize) {
            unsafe { (cb.func)(cb.arg) };
        } else {
            crate::serial_write("[CFI] Blocked invalid RCU callback\n");
        }
    }
}

/// RCU-protected pointer wrapper.
///
/// Provides safe access to data that may be updated via RCU.
/// Writers create a new copy, modify it, then swap the pointer.
/// Readers call `read()` to get a reference that's valid until
/// `rcu_read_unlock()`.
pub struct RcuPtr<T: 'static> {
    ptr: core::sync::atomic::AtomicPtr<T>,
}

impl<T> RcuPtr<T> {
    /// Create a new RCU pointer with an initial value.
    pub fn new(val: T) -> Self {
        Self {
            ptr: core::sync::atomic::AtomicPtr::new(Box::into_raw(Box::new(val))),
        }
    }
    
    /// Read the current pointer.
    ///
    /// The returned pointer is valid as long as the caller is inside an
    /// RCU read-side critical section (between `rcu_read_lock()` and
    /// `rcu_read_unlock()`).
    pub fn read(&self) -> *const T {
        self.ptr.load(Ordering::Acquire)
    }
    
    /// Update the pointer with a new value.
    ///
    /// The old value will be freed after a grace period. This is the
    /// "update" part of Read-Copy-Update.
    pub fn update(&self, new_val: T) {
        let new_ptr = Box::into_raw(Box::new(new_val));
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        
        // Schedule freeing the old data after a grace period
        if !old_ptr.is_null() {
            unsafe extern "C" fn free_ptr<T>(ptr: *mut u8) {
                unsafe {
                    let _ = Box::from_raw(ptr as *mut T);
                }
            }
            call_rcu(free_ptr::<T> as unsafe extern "C" fn(*mut u8), old_ptr as *mut u8);
        }
    }
}

impl<T> Drop for RcuPtr<T> {
    fn drop(&mut self) {
        let ptr = self.ptr.load(Ordering::Acquire);
        if !ptr.is_null() {
            unsafe { let _ = Box::from_raw(ptr); }
        }
    }
}

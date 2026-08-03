use core::sync::atomic::{AtomicBool, Ordering};

pub struct RawIrqMutex(AtomicBool);

impl RawIrqMutex {
    pub const fn new() -> Self {
        RawIrqMutex(AtomicBool::new(false))
    }
}

// SAFETY: x86_64 interrupt disable achieves mutual exclusion on single-core;
// on SMP the atomic CAS enforces mutual exclusion across cores. Disabling
// interrupts prevents deadlock with interrupt handlers.
unsafe impl lock_api::RawMutex for RawIrqMutex {
    type GuardMarker = lock_api::GuardSend;

    const INIT: RawIrqMutex = RawIrqMutex(AtomicBool::new(false));

    fn lock(&self) {
        // ponytail: always-enable assumption; a full save/restore RFLAGS.IF
        // variant can be added if a code path holds a lock with IRQs already off
        x86_64::instructions::interrupts::disable();
        while self.0.swap(true, Ordering::Acquire) {
            while self.0.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }

    fn try_lock(&self) -> bool {
        x86_64::instructions::interrupts::disable();
        if !self.0.swap(true, Ordering::Acquire) {
            true
        } else {
            x86_64::instructions::interrupts::enable();
            false
        }
    }

    // SAFETY: caller must hold the lock; re-enabling interrupts is safe
    // after releasing the atomic flag.
    unsafe fn unlock(&self) {
        self.0.store(false, Ordering::Release);
        // ponytail: always re-enable; see lock() comment
        x86_64::instructions::interrupts::enable();
    }
}

pub type IrqMutex<T> = lock_api::Mutex<RawIrqMutex, T>;

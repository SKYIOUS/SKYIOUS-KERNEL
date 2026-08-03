//! IRQ-safe spin mutex: interrupts are disabled while the lock is held.
//!
//! The timer IRQ (and `try_schedule` inside it) can context-switch threads at
//! any instruction. With a plain spinlock, a thread preempted mid-critical-
//! section strands the lock: the next thread to acquire it spins forever — and
//! if that spin happens with IF=0 (e.g. the boot trampoline entering userspace
//! cli's before allocating), the CPU is dead: no iret means the holder never
//! resumes. Holding the lock with IF=0 makes the holder uninterruptible, so an
//! IRQ-handler switch can never strand a lock.
//!
//! Guards must not be held across a blocking operation (sleep/wait): the CPU
//! would sit with IF=0 and no tick could ever wake the sleeper.

use core::ops::{Deref, DerefMut};
use core::mem::ManuallyDrop;

pub struct IrqSafeMutex<T: ?Sized> {
    inner: spin::Mutex<T>,
}

pub struct IrqSafeMutexGuard<'a, T: ?Sized> {
    inner: ManuallyDrop<spin::MutexGuard<'a, T>>,
    rflags: u64,
}

impl<T> IrqSafeMutex<T> {
    pub const fn new(value: T) -> Self {
        Self { inner: spin::Mutex::new(value) }
    }
}

impl<T: ?Sized> IrqSafeMutex<T> {
    pub fn lock(&self) -> IrqSafeMutexGuard<'_, T> {
        // SAFETY: `pushfq; pop` captures the current flags, `cli` disables
        // interrupts; the captured state is restored when the guard drops.
        let rflags: u64;
        unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) rflags, options(att_syntax)) };
        IrqSafeMutexGuard { inner: ManuallyDrop::new(self.inner.lock()), rflags }
    }

    pub fn try_lock(&self) -> Option<IrqSafeMutexGuard<'_, T>> {
        let rflags: u64;
        unsafe { core::arch::asm!("pushfq; pop {0}; cli", out(reg) rflags, options(att_syntax)) };
        match self.inner.try_lock() {
            Some(g) => Some(IrqSafeMutexGuard { inner: ManuallyDrop::new(g), rflags }),
            None => {
                if rflags & 0x200 != 0 {
                    // SAFETY: re-enable interrupts only if they were enabled
                    // before we disabled them.
                    unsafe { core::arch::asm!("sti") };
                }
                None
            }
        }
    }

    /// True if the underlying spin mutex is currently held. Used by the
    /// self-test suite to verify guards release the lock on drop.
    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }
}

impl<T: core::fmt::Display + ?Sized> core::fmt::Display for IrqSafeMutexGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<'a, T: ?Sized> Deref for IrqSafeMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner.deref()
    }
}

impl<'a, T: ?Sized> DerefMut for IrqSafeMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.deref_mut()
    }
}

impl<'a, T: ?Sized> Drop for IrqSafeMutexGuard<'a, T> {
    fn drop(&mut self) {
        // SAFETY: the inner spin mutex must be unlocked FIRST, while
        // interrupts are still off, and only then may IF be restored. If the
        // unlock ran with IF=1 (field drops happen after this body), a timer
        // IRQ could preempt the thread in the window where the lock is still
        // held; the next allocator user would then spin at IF=0 forever while
        // the holder can never resume.
        unsafe { core::ptr::drop_in_place(&mut *self.inner) };
        // SAFETY: sti is only issued if the flag captured at lock time says
        // interrupts were enabled then.
        if self.rflags & 0x200 != 0 {
            unsafe { core::arch::asm!("sti") };
        }
    }
}

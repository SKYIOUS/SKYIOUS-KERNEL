use core::sync::atomic::{AtomicU32, Ordering};

// Futex-based mutex
pub struct Mutex {
    inner: AtomicU32, // 0 = unlocked, 1 = locked (no waiters), 2 = locked (with waiters)
}

impl Mutex {
    pub const fn new() -> Self {
        Mutex { inner: AtomicU32::new(0) }
    }

    pub fn lock(&self) {
        // Fast path: try to acquire
        if self.inner.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            return;
        }
        // Slow path: mark as having waiters and wait
        loop {
            if self.inner.swap(2, Ordering::Acquire) == 0 {
                return;
            }
            // FUTEX_WAIT: wake when *uaddr != val (i.e., when inner != 2)
            crate::syscall::futex(
                &self.inner as *const AtomicU32 as *mut u32,
                0, // FUTEX_WAIT
                2, // wait while value == 2
            );
        }
    }

    pub fn unlock(&self) {
        if self.inner.swap(0, Ordering::Release) == 2 {
            // FUTEX_WAKE: wake one waiter
            crate::syscall::futex(
                &self.inner as *const AtomicU32 as *mut u32,
                1, // FUTEX_WAKE
                1, // wake 1
            );
        }
    }
}

// TLS (Thread-Local Storage) key management
// Simple implementation: allocate TLS block and set FS base via arch_prctl

pub struct TlsKey {
    pub offset: usize, // offset within the TLS block
}

impl TlsKey {
    pub const fn new(offset: usize) -> Self {
        TlsKey { offset }
    }

    pub fn get(&self) -> u64 {
        let mut base = 0u64;
        crate::syscall::arch_prctl(0x1003, &mut base as *mut u64 as u64); // ARCH_GET_FS
        if base == 0 { return 0; }
        unsafe { *((base + self.offset as u64) as *const u64) }
    }

    pub fn set(&self, val: u64) {
        let mut base = 0u64;
        crate::syscall::arch_prctl(0x1003, &mut base as *mut u64 as u64); // ARCH_GET_FS
        if base == 0 { return; }
        unsafe { *((base + self.offset as u64) as *mut u64) = val; }
    }
}

/// Initialize TLS for the current thread by allocating a page and setting FS base.
pub fn init_tls() -> u64 {
    // Allocate a 4KB page for TLS
    let addr = crate::syscall::syscall2(crate::SYS_MMAP, 0, 4096);
    if (addr as i64) < 0 || addr >= 0xFFFF_FFFF_FFFF_FF00 {
        return 0;
    }

    // Set FS base to the allocated TLS block
    let ret = crate::syscall::arch_prctl(0x1002, addr); // ARCH_SET_FS
    if ret != 0 {
        return 0;
    }

    // Initialize errno at offset 0 to 0
    unsafe { *(addr as *mut i32) = 0; }

    addr
}

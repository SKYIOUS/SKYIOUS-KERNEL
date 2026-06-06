use core::sync::atomic::{AtomicI32, Ordering};

// Per-thread errno stored at offset 0 of the TLS area (FS segment base).
// The TLS area is allocated and set via arch_prctl(ARCH_SET_FS, addr).
// Offset 0 holds the errno value.

#[repr(C)]
struct TlsBlock {
    errno: AtomicI32,
}

// Default TLS block for the main thread (stack-allocated).
static MAIN_TLS: TlsBlock = TlsBlock { errno: AtomicI32::new(0) };

/// Get the current thread's TLS block.
fn tls_block() -> &'static TlsBlock {
    // Read FS base via arch_prctl (or use the main TLS if not initialized)
    let mut base = 0u64;
    let ret = crate::syscall::arch_prctl(0x1003, &mut base as *mut u64 as u64); // ARCH_GET_FS
    if ret != 0 || base == 0 {
        // Fallback to main TLS
        return &MAIN_TLS;
    }
    unsafe { &*(base as *const TlsBlock) }
}

pub fn get_errno() -> i32 {
    tls_block().errno.load(Ordering::Relaxed)
}

pub fn set_errno(e: i32) {
    tls_block().errno.store(e, Ordering::Relaxed);
}

#[no_mangle]
pub extern "C" fn __errno_location() -> *mut i32 {
    let tb = tls_block();
    &tb.errno as *const AtomicI32 as *mut i32
}

/// Check a syscall return value and set errno if it's negative.
/// Returns -1 on error, or the original return value on success.
pub fn check(ret: u64) -> u64 {
    if (ret as i64) < 0 {
        let err = (-(ret as i64)) as i32;
        set_errno(err);
        u64::MAX
    } else {
        ret
    }
}

/// Initialize TLS for the current thread. Called from pthread_create / at startup.
pub fn init_tls(addr: u64) {
    if addr != 0 {
        unsafe {
            let tls = &mut *(addr as *mut TlsBlock);
            tls.errno.store(0, Ordering::SeqCst);
        }
    }
}

pub fn allocate_tls() -> u64 {
    // Allocate a page for TLS via mmap (or use brk/heap)
    let addr = crate::syscall::syscall2(
        crate::SYS_MMAP,
        0,
        core::mem::size_of::<TlsBlock>() as u64,
    );
    if (addr as i64) < 0 || addr >= 0xFFFF_FFFF_FFFF_FF00 {
        return 0;
    }
    init_tls(addr);
    addr
}

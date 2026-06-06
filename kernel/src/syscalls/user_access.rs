//! # User Access Helpers
//!
//! Provides safe ways to access userspace memory from the kernel.
//! These functions handle SMAP (Supervisor Mode Access Prevention) by
//! using `stac` and `clac` instructions where appropriate.

use core::slice;
use core::sync::atomic::{AtomicBool, Ordering};

/// Whether the CPU supports SMAP. Initialized once at boot via CPUID.
static HAS_SMAP: AtomicBool = AtomicBool::new(false);

/// Call once at boot to detect SMAP support and set CR4.SMAP if available.
pub fn init_smap() {
    let has_smap = smap_supported();
    HAS_SMAP.store(has_smap, Ordering::Relaxed);
    if has_smap {
        unsafe {
            use x86_64::registers::control::Cr4;
            use x86_64::registers::control::Cr4Flags;
            Cr4::update(|flags| {
                flags.insert(Cr4Flags::SUPERVISOR_MODE_ACCESS_PREVENTION);
            });
        }
    }
}

/// Detect SMAP support via CPUID leaf 7 (EBX bit 20).
fn smap_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let rbx_val: u64;
        core::arch::asm!(
            "push rbx",
            "mov eax, 7",
            "xor ecx, ecx",
            "cpuid",
            "mov {0}, rbx",
            "pop rbx",
            out(reg) rbx_val,
            out("eax") _, out("ecx") _, out("edx") _,
            options(nomem)
        );
        (rbx_val & (1 << 20)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    false
}

#[inline(always)]
fn do_stac() {
    if HAS_SMAP.load(Ordering::Relaxed) {
        unsafe { core::arch::asm!("stac", options(nomem, nostack, preserves_flags)); }
    }
}

#[inline(always)]
fn do_clac() {
    if HAS_SMAP.load(Ordering::Relaxed) {
        unsafe { core::arch::asm!("clac", options(nomem, nostack, preserves_flags)); }
    }
}

/// Validates that a pointer range is within userspace limits.
/// On Vahi, userspace is currently below 0x0000_8000_0000_0000.
pub fn validate_ptr(ptr: *const u8, len: usize) -> bool {
    let start = ptr as u64;
    let end = match start.checked_add(len as u64) {
        Some(e) => e,
        None => return false,
    };
    
    let user_limit = 0x0000_8000_0000_0000;
    end <= user_limit
}

/// Safely copies data from userspace to a kernel buffer.
/// Returns Ok(()) if the address was valid and copy succeeded.
pub unsafe fn copy_from_user(dst: &mut [u8], src_ptr: *const u8) -> Result<(), ()> {
    if !validate_ptr(src_ptr, dst.len()) {
        return Err(());
    }

    do_stac();
    core::ptr::copy_nonoverlapping(src_ptr, dst.as_mut_ptr(), dst.len());
    do_clac();
    
    Ok(())
}

/// Safely copies data from a kernel buffer to userspace.
pub unsafe fn copy_to_user(dst_ptr: *mut u8, src: &[u8]) -> Result<(), ()> {
    if !validate_ptr(dst_ptr, src.len()) {
        return Err(());
    }

    do_stac();
    core::ptr::copy_nonoverlapping(src.as_ptr(), dst_ptr, src.len());
    do_clac();
    
    Ok(())
}

/// A wrapper for reading a string from userspace.
pub unsafe fn read_user_string(ptr: *const u8, max_len: usize) -> Result<alloc::string::String, ()> {
    let mut len = 0;
    
    do_stac();
    
    while len < max_len {
        if !validate_ptr(ptr.add(len), 1) {
            do_clac();
            return Err(());
        }
        if *ptr.add(len) == 0 {
            break;
        }
        len += 1;
    }
    
    let s = slice::from_raw_parts(ptr, len);
    let result = alloc::string::String::from_utf8(s.to_vec()).map_err(|_| ());
    
    do_clac();
    
    result
}

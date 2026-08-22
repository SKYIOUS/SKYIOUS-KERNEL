//! # User Access Helpers
//!
//! Provides safe ways to access userspace memory from the kernel.
//! These functions handle SMAP (Supervisor Mode Access Prevention) by
//! using `stac` and `clac` instructions where appropriate.

use core::sync::atomic::{AtomicBool, Ordering};

/// Whether the CPU supports SMAP. Initialized once at boot via CPUID.
static HAS_SMAP: AtomicBool = AtomicBool::new(false);

// x86-64 copy with exception-style fixup. `rep movsb` faults if the user
// range contains an unmapped page; the page-fault handler redirects RIP to
// `user_copy_fault_return`, which returns 1 (failure). Otherwise 0.
core::arch::global_asm!(
    r#"
    .global user_copy_bytes
    user_copy_bytes:
        mov rax, rdx
        mov rcx, rdx
        rep movsb
        xor eax, eax
        ret
    .global user_copy_fault_return
    user_copy_fault_return:
        mov eax, 1
        ret
    "#
);
extern "C" {
    fn user_copy_bytes(dst: *mut u8, src: *const u8, len: usize) -> usize;
}
// Address of the asm fixup label — the page-fault handler iret's here to
// abort an active copy.
extern "C" {
    static user_copy_fault_return: u8;
}

/// True while the kernel is copying from/to user memory on the current CPU.
/// The page-fault handler checks this: a fault while true means "bad user
/// pointer" → abort the copy instead of panicking the kernel.
pub fn user_copy_active() -> bool {
    crate::syscalls::get_per_cpu().user_copy_nest.load(Ordering::Relaxed) > 0
}

/// Address the page-fault handler must return to to abort the active copy.
pub fn user_copy_fixup_addr() -> u64 {
    core::ptr::addr_of!(user_copy_fault_return) as u64
}

/// Abort the in-flight `user_copy_bytes` and return "failed" to its caller,
/// without ever returning here. Called by the page-fault handler when a
/// user-range fault inside a copy cannot be resolved (COW/swap/demand).
///
/// The trampoline stored the CPU entry RSP (pointing at the error code) in
/// `PerCpuData::pf_entry_rsp`. We overwrite the saved RIP slot with the fixup
/// and iretq into it; the fixup (`mov eax, 1; ret`) returns to the copy
/// routine's caller where the nest count is decremented and `clac` runs.
pub fn abort_user_copy() -> ! {
    let entry_rsp = crate::syscalls::get_per_cpu().pf_entry_rsp;
    let fixup = user_copy_fixup_addr();
    unsafe {
        core::arch::asm!(
            "mov qword ptr [{e} + 8], {f}",  // saved RIP slot
            "mov rsp, {e}",
            "add rsp, 8",
            "iretq",
            e = in(reg) entry_rsp,
            f = in(reg) fixup,
            options(noreturn),
        );
    }
}

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

/// Core user-copy helper: validates pointers are in userspace, then performs
/// the copy with SMAP and nest tracking. Returns the failure code from
/// `user_copy_bytes` (0 = success, non-zero = fault).
unsafe fn do_user_copy(dst: *mut u8, src: *const u8, len: usize) -> usize {
    do_stac();
    crate::syscalls::get_per_cpu().user_copy_nest.fetch_add(1, Ordering::Relaxed);
    let failed = user_copy_bytes(dst, src, len);
    crate::syscalls::get_per_cpu().user_copy_nest.fetch_sub(1, Ordering::Relaxed);
    do_clac();
    failed
}

/// Safely copies data from userspace to a kernel buffer.
/// Returns Ok(()) if the address was valid and copy succeeded.
pub unsafe fn copy_from_user(dst: &mut [u8], src_ptr: *const u8) -> Result<(), ()> {
    if !validate_ptr(src_ptr, dst.len()) {
        return Err(());
    }

    if do_user_copy(dst.as_mut_ptr(), src_ptr, dst.len()) != 0 {
        return Err(());
    }
    Ok(())
}

/// Safely copies data from a kernel buffer to userspace.
pub unsafe fn copy_to_user(dst_ptr: *mut u8, src: &[u8]) -> Result<(), ()> {
    if !validate_ptr(dst_ptr, src.len()) {
        return Err(());
    }

    if do_user_copy(dst_ptr, src.as_ptr(), src.len()) != 0 {
        return Err(());
    }
    Ok(())
}

/// A wrapper for reading a string from userspace.
pub unsafe fn read_user_string(ptr: *const u8, max_len: usize) -> Result<alloc::string::String, ()> {
    if ptr.is_null() || max_len == 0 {
        return Err(());
    }

    if !validate_ptr(ptr, max_len) {
        return Err(());
    }

    let mut buf = alloc::vec![0u8; max_len];
    if do_user_copy(buf.as_mut_ptr(), ptr, max_len) != 0 {
        return Err(());
    }

    let actual_len = buf.iter().position(|&b| b == 0).unwrap_or(max_len);
    buf.truncate(actual_len);
    alloc::string::String::from_utf8(buf).map_err(|_| ())
}

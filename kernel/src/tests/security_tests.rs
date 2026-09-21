// Security regression tests — sys_chdir userspace pointer validation.
//
// These tests verify that sys_chdir (and the user_access::read_user_string
// helper it uses) correctly rejects invalid pointers without dereferencing
// them. The security invariant is:
//
//   sys_chdir must never dereference an unchecked userspace pointer and must
//   reject invalid pointers with EFAULT.
//
// In kernel-mode selftest context, we cannot easily create valid userspace
// pointers (validate_ptr rejects kernel addresses). We test the negative
// cases: kernel pointers, null pointers, and unmapped addresses must all
// be rejected with EFAULT.

use crate::syscalls::errno;
use crate::syscalls::user_access;

const EFAULT: u64 = errno::Errno::EFAULT as u64;

// ─── read_user_string validation tests ────────────────────────────

fn test_read_user_string_kernel_ptr() -> Result<(), &'static str> {
    // A kernel address (any address above 0x0000_8000_0000_0000) must be
    // rejected by validate_ptr. We use a known kernel address.
    let kernel_ptr: *const u8 = 0xFFFF_8000_0000_0000 as *const u8;
    match unsafe { user_access::read_user_string(kernel_ptr, 256) } {
        Ok(_) => Err("read_user_string accepted kernel pointer"),
        Err(_) => Ok(()),
    }
}

fn test_read_user_string_null_ptr() -> Result<(), &'static str> {
    // Null pointer must be rejected.
    let null_ptr: *const u8 = core::ptr::null();
    match unsafe { user_access::read_user_string(null_ptr, 256) } {
        Ok(_) => Err("read_user_string accepted null pointer"),
        Err(_) => Ok(()),
    }
}

fn test_read_user_string_unmapped_ptr() -> Result<(), &'static str> {
    // An unmapped userspace address (below kernel range but not mapped).
    // 0x0000_0000_0000_1000 is typically unmapped in user space.
    let unmapped_ptr: *const u8 = 0x0000_0000_0000_1000 as *const u8;
    match unsafe { user_access::read_user_string(unmapped_ptr, 256) } {
        Ok(_) => Err("read_user_string accepted unmapped pointer"),
        Err(_) => Ok(()),
    }
}

fn test_read_user_string_max_len() -> Result<(), &'static str> {
    // A kernel pointer with max_len=0 should still be rejected (validate_ptr
    // runs before the length check).
    let kernel_ptr: *const u8 = 0xFFFF_8000_0000_0000 as *const u8;
    match unsafe { user_access::read_user_string(kernel_ptr, 0) } {
        Ok(_) => Err("read_user_string accepted kernel ptr with max_len=0"),
        Err(_) => Ok(()),
    }
}

// ─── sys_chdir validation tests ────────────────────────────────────

fn test_sys_chdir_kernel_ptr() -> Result<(), &'static str> {
    // sys_chdir with a kernel pointer must return EFAULT.
    let kernel_ptr: *const u8 = 0xFFFF_8000_0000_0000 as *const u8;
    let result = crate::syscalls::fs_open::sys_chdir(kernel_ptr);
    if result == EFAULT {
        Ok(())
    } else {
        Err("sys_chdir did not return EFAULT for kernel pointer")
    }
}

fn test_sys_chdir_null_ptr() -> Result<(), &'static str> {
    // sys_chdir with null pointer must return EFAULT.
    let null_ptr: *const u8 = core::ptr::null();
    let result = crate::syscalls::fs_open::sys_chdir(null_ptr);
    if result == EFAULT {
        Ok(())
    } else {
        Err("sys_chdir did not return EFAULT for null pointer")
    }
}

fn test_sys_chdir_unmapped_ptr() -> Result<(), &'static str> {
    // sys_chdir with an unmapped userspace address must return EFAULT.
    let unmapped_ptr: *const u8 = 0x0000_0000_0000_1000 as *const u8;
    let result = crate::syscalls::fs_open::sys_chdir(unmapped_ptr);
    if result == EFAULT {
        Ok(())
    } else {
        Err("sys_chdir did not return EFAULT for unmapped pointer")
    }
}

// ─── Registration ──────────────────────────────────────────────────

pub fn register() {
    crate::selftest::register(
        "security::read_user_string_kernel_ptr",
        test_read_user_string_kernel_ptr,
    );
    crate::selftest::register(
        "security::read_user_string_null_ptr",
        test_read_user_string_null_ptr,
    );
    crate::selftest::register(
        "security::read_user_string_unmapped_ptr",
        test_read_user_string_unmapped_ptr,
    );
    crate::selftest::register(
        "security::read_user_string_max_len",
        test_read_user_string_max_len,
    );
    crate::selftest::register("security::sys_chdir_kernel_ptr", test_sys_chdir_kernel_ptr);
    crate::selftest::register("security::sys_chdir_null_ptr", test_sys_chdir_null_ptr);
    crate::selftest::register(
        "security::sys_chdir_unmapped_ptr",
        test_sys_chdir_unmapped_ptr,
    );
}

//! User memory access helpers (copy_from_user / copy_to_user).
//!
//! These are stubbed for crate extraction; the real implementations
//! use SMAP-aware assembly and live in `kernel/src/syscalls/`.

extern crate alloc;

/// Copy data from user space to kernel space.
///
/// # Safety
///
/// `src_ptr` must be a valid user-space pointer for `dst.len()` bytes.
#[allow(clippy::result_unit_err)]
pub unsafe fn copy_from_user(_dst: &mut [u8], _src_ptr: *const u8) -> Result<(), ()> {
    Ok(())
}

/// Copy data from kernel space to user space.
///
/// # Safety
///
/// `dst_ptr` must be a valid user-space pointer for `src.len()` bytes.
#[allow(clippy::result_unit_err)]
pub unsafe fn copy_to_user(_dst_ptr: *mut u8, _src: &[u8]) -> Result<(), ()> {
    Ok(())
}

/// Stub: check if we're in a user memory copy context.
#[allow(dead_code)]
pub fn user_copy_active() -> bool {
    false
}

/// Read a null-terminated string from user space.
///
/// # Safety
/// `ptr` must be a valid user-space pointer.
#[allow(clippy::result_unit_err)]
pub unsafe fn read_user_string(
    _ptr: *const u8,
    _max_len: usize,
) -> Result<alloc::string::String, ()> {
    Ok(alloc::string::String::new())
}

/// Stub: abort an in-progress user memory copy.
#[allow(dead_code)]
pub fn abort_user_copy() {}

//! User memory access helpers (copy_from_user / copy_to_user).
//!
//! The real implementations use SMAP-aware assembly and live in
//! `kernel/src/syscalls/user_access.rs`. This module exists so `vahi-net` and
//! `vahi-vfs` can name the API across the crate boundary.
//!
//! These bodies MUST NOT report success while copying nothing. An earlier
//! version returned `Ok(())` unconditionally, so every caller in the net and
//! vfs crates silently received success with an untouched buffer:
//!   - `sendto_unix` transmitted `len` zero bytes and returned `len`,
//!   - `recvfrom_unix` reported `copy_len` written while the user buffer kept
//!     stale contents,
//!   - `socketpair` returned `Ok(0)` without writing the two descriptors, so
//!     the caller kept uninitialised stack garbage.
//!
//! Returning an error converts that from silent data corruption into a clean
//! refusal at every call site, with no change to any caller. Wiring the real
//! SMAP-aware copies in needs `#[no_mangle]` exports across the crate boundary
//! (the same shape as `vahi_kernel_get_ticks` in `crates/net/src/lib.rs`); that
//! is tracked in `docs/architecture/issues.md` rather than done here.

extern crate alloc;

/// Copy data from user space to kernel space.
///
/// # Safety
///
/// `src_ptr` must be a valid user-space pointer for `dst.len()` bytes.
#[allow(clippy::result_unit_err)]
pub unsafe fn copy_from_user(_dst: &mut [u8], _src_ptr: *const u8) -> Result<(), ()> {
    // No copy was performed, so reporting Ok(()) would make the caller read
    // whatever was already in the buffer as if it were user data.
    Err(())
}

/// Copy data from kernel space to user space.
///
/// # Safety
///
/// `dst_ptr` must be a valid user-space pointer for `src.len()` bytes.
#[allow(clippy::result_unit_err)]
pub unsafe fn copy_to_user(_dst_ptr: *mut u8, _src: &[u8]) -> Result<(), ()> {
    // Same reasoning as copy_from_user: nothing was written to the user buffer.
    Err(())
}

/// Stub: check if we're in a user memory copy context.
    #[allow(dead_code)]
    // ponytail: always false (no copy-active tracking); upgrade path: track nest depth vs do_user_addr_fault unwinding.
    pub fn user_copy_active() -> bool {
    false
}

/// Read a null-terminated string from user space.
///
/// # Safety
/// `ptr` must be a valid user-space pointer.
#[allow(clippy::result_unit_err)]
pub unsafe fn read_user_string(_ptr: *const u8, _max_len: usize) -> Result<alloc::string::String, ()> {
    // Returning an empty string reads as a successful empty argument, so
    // callers would operate on "" instead of failing. Refuse instead.
    Err(())
}

/// Stub: abort an in-progress user memory copy.
#[allow(dead_code)]
// ponytail: no-op (no unwind/restore); upgrade path: restore saved user regs + drop copy-active flag.
pub fn abort_user_copy() {}

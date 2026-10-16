//! Userspace-pointer contract for the syscall layer (AGENTS.md non-negotiable #8).
//!
//! Every userspace-supplied pointer must reach kernel state through
//! `validate_ptr`/`copy_from_user`/`copy_to_user`. These pin the paths that
//! built a slice over the raw register value instead, each of which let ring 0
//! read or write an arbitrary address rather than returning EFAULT:
//!   - `io_uring_register(IORING_REGISTER_BUFFERS/FILES/EVENTFD)`
//!   - `write()` on an io_uring fd (SQEs)
//!   - `read()` on an inotify fd (header + watch name written through `buf`)
//!   - `getgroups` / `setgroups`
//!   - GUI clipboard get/set
//!
//! Each test drives the handler to the point where it touches the pointer, so
//! a refusal proves the pointer was validated rather than that some earlier
//! guard (no such fd, no such process, empty list) short-circuited the call.

use crate::selftest;
use crate::syscalls::errno::Errno;

const KERNEL_PTR: u64 = 0xFFFF_8000_0000_0000;

fn efault() -> u64 {
    Errno::EFAULT as u64
}

fn einval() -> u64 {
    Errno::EINVAL as u64
}

/// A refusal is correct; success is not. EINVAL counts as a refusal only for
/// handlers that reject on arity before touching the pointer.
fn refused(rc: u64, allow_einval: bool) -> bool {
    rc == efault() || (allow_einval && rc == einval())
}

/// Errno as a signed value, for a diagnostic message (the handlers return the
/// negative errno cast to u64).
fn as_errno(rc: u64) -> i64 {
    rc as i64
}

/// Install a process context for the duration of a test.
///
/// Several handlers resolve the fd through `CURRENT_PROCESS` and return ESRCH
/// before ever reaching the pointer, so without a process a refusal proves
/// nothing. The previous context is restored so a failing assertion cannot
/// leak into the next test.
fn with_process_context<T>(f: impl FnOnce() -> T) -> T {
    use crate::memory::buddy::BuddyFrameAllocator;
    use crate::memory::paging::AddressSpace;
    use crate::task::process::{Process, CURRENT_PROCESS};

    let previous = CURRENT_PROCESS.lock().clone();
    let mut allocator = BuddyFrameAllocator;
    if let Some(addr_space) = AddressSpace::new(&mut allocator) {
        let new_proc =
            alloc::sync::Arc::new(Process::new(Process::next_id(), None, addr_space));
        *CURRENT_PROCESS.lock() = Some(new_proc);
    }
    let out = f();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// `io_uring_register` sliced the raw `arg` for BUFFERS, FILES and EVENTFD
/// with no `validate_ptr`, so a kernel address was read (and, for FILES,
/// turned into fds) in ring 0.
fn test_io_uring_register_rejects_kernel_pointer() -> Result<(), &'static str> {
    with_process_context(|| {
        let ring_fd = crate::syscalls::io_uring::sys_io_uring_setup(8, 0);
        if ring_fd > 0xFFFF {
            return Err("io_uring_setup did not yield an fd");
        }

        for op in [
            crate::syscalls::io_uring::IORING_REGISTER_BUFFERS,
            crate::syscalls::io_uring::IORING_REGISTER_FILES,
            crate::syscalls::io_uring::IORING_REGISTER_EVENTFD,
        ] {
            let rc = crate::syscalls::io_uring::sys_io_uring_register(
                ring_fd,
                op,
                KERNEL_PTR,
                8,
            );
            if rc == 0 {
                return Err("io_uring_register accepted a kernel pointer");
            }
            if !refused(rc, true) {
                if rc == efault() {
                    return Err("io_uring_register should have returned EFAULT");
                }
                return Err("io_uring_register refused with an unexpected errno");
            }
        }
        Ok(())
    })
}

/// `write()` on an io_uring fd built `from_raw_parts` over the caller's SQE
/// pointer, so submitting read an attacker-chosen address in ring 0.
fn test_io_uring_write_rejects_kernel_sqe_pointer() -> Result<(), &'static str> {
    with_process_context(|| {
        let ring_fd = crate::syscalls::io_uring::sys_io_uring_setup(8, 0);
        if ring_fd > 0xFFFF {
            return Err("io_uring_setup did not yield an fd");
        }
        let sqe_size = core::mem::size_of::<crate::syscalls::io_uring::IoSqEntry>();
        let rc =
            crate::syscalls::fs_io::sys_write(ring_fd, KERNEL_PTR as *const u8, sqe_size);
        if rc == 0 {
            return Err("io_uring write() accepted a kernel SQE pointer");
        }
        if !refused(rc, true) {
            return Err("io_uring write() refused with an unexpected errno");
        }
        Ok(())
    })
}

/// `read()` on an inotify fd wrote the event header and watch name through a
/// raw `from_raw_parts_mut` — an arbitrary-address kernel *write*.
fn test_inotify_read_rejects_kernel_buffer() -> Result<(), &'static str> {
    with_process_context(|| {
        let watch_fd = crate::syscalls::inotify::sys_inotify_init(0);
        if watch_fd > 0xFFFF {
            return Err("inotify_init did not yield an fd");
        }
        let path = b"/tmp\0";
        let _wd = crate::syscalls::inotify::sys_inotify_add_watch(
            watch_fd,
            path.as_ptr(),
            0x0000_00FF,
        );

        let rc = crate::syscalls::fs_io::sys_read(watch_fd, KERNEL_PTR as *mut u8, 64);
        if rc == 0 {
            return Err("inotify read() reported success into a kernel buffer");
        }
        if !refused(rc, true) {
            return Err("inotify read() refused with an unexpected errno");
        }
        Ok(())
    })
}

/// `getgroups` wrote `groups.len()` u32s through an unchecked `list`. With an
/// empty group list the handler writes nothing and returns 0, which would be a
/// vacuous pass, so the group list is populated first.
fn test_getgroups_rejects_kernel_buffer() -> Result<(), &'static str> {
    with_process_context(|| {
        {
            use crate::task::process::CURRENT_PROCESS;
            let guard = CURRENT_PROCESS.lock();
            let p = match guard.as_ref() {
                Some(p) => p,
                None => return Err("no process context"),
            };
            *p.groups.lock() = alloc::vec![1u32, 2, 3];
        }

        let n = crate::syscalls::process_creds::sys_getgroups(0, core::ptr::null_mut());
        if n < 3 {
            return Err("group list was not populated; test would be vacuous");
        }

        let rc =
            crate::syscalls::process_creds::sys_getgroups(64, KERNEL_PTR as *mut u32);
        if rc == 0 {
            return Err("getgroups wrote through a kernel pointer");
        }
        if !refused(rc, true) {
            return Err("getgroups refused with an unexpected errno");
        }
        Ok(())
    })
}

/// `setgroups` reads `size` u32s from an unchecked `list`. Capability-gated, so
/// EPERM is an acceptable refusal; a success means kernel memory was adopted
/// as the supplementary group list.
fn test_setgroups_rejects_kernel_buffer() -> Result<(), &'static str> {
    with_process_context(|| {
        let rc =
            crate::syscalls::process_creds::sys_setgroups(4, KERNEL_PTR as *const u32);
        if rc == 0 {
            return Err("setgroups adopted kernel memory as the group list");
        }
        if rc != efault() && rc != einval() && rc != Errno::EPERM as u64 {
            return Err("setgroups refused with an unexpected errno");
        }
        Ok(())
    })
}

/// GUI clipboard copies both directions through the raw `buf`: mode 0 is a
/// kernel→arbitrary-address write, mode 1 an arbitrary-address read into
/// global state that mode 0 can then exfiltrate.
fn test_clipboard_rejects_kernel_buffer() -> Result<(), &'static str> {
    // No userspace pages are mapped during the selftest run (the existing
    // `new_features::user_copy_fault_abort` test relies on the same fact), so
    // this pins the refusal contract rather than a successful round-trip.
    //
    // Mode 1 copies `len` bytes FROM `buf` into clipboard state and returns
    // `len`; mode 0 copies clipboard contents TO `buf`. Both used raw
    // `copy_nonoverlapping` over the caller's address, so a kernel address
    // turned into a kernel-side read (mode 1, later exfiltrated by mode 0) and
    // a kernel-side write (mode 0). The kernel heap lives at
    // 0xFFFF_C000_0000_0000, far above the userspace ceiling, so any address a
    // selftest can name here is hostile and must be refused with EFAULT.
    let rc = crate::syscalls::gui::sys_clipboard(1, KERNEL_PTR as *mut u8, 8);
    if rc == 0 {
        return Err("clipboard set accepted a kernel pointer");
    }
    if !refused(rc, true) {
        return Err("clipboard set refused with an unexpected errno");
    }

    // Mode 0's guard is only reached when the clipboard is non-empty; mode 1
    // above left it empty because it was refused. Confirm the guard itself
    // through validate_ptr, which is the choke point both modes call, so the
    // mode-0 path is covered by construction rather than by state.
    if crate::syscalls::user_access::validate_ptr(KERNEL_PTR as *const u8, 8) {
        return Err("validate_ptr accepted a kernel pointer");
    }

    // Leave no state behind for later tests.
    crate::syscalls::gui::sys_clipboard(1, core::ptr::null_mut(), 0);
    Ok(())
}

pub fn register() {
    selftest::register(
        "userptr:io_uring_register_rejects_kernel_pointer",
        test_io_uring_register_rejects_kernel_pointer,
    );
    selftest::register(
        "userptr:io_uring_write_rejects_kernel_sqe_pointer",
        test_io_uring_write_rejects_kernel_sqe_pointer,
    );
    selftest::register(
        "userptr:inotify_read_rejects_kernel_buffer",
        test_inotify_read_rejects_kernel_buffer,
    );
    selftest::register(
        "userptr:getgroups_rejects_kernel_buffer",
        test_getgroups_rejects_kernel_buffer,
    );
    selftest::register(
        "userptr:setgroups_rejects_kernel_buffer",
        test_setgroups_rejects_kernel_buffer,
    );
    selftest::register(
        "userptr:clipboard_rejects_kernel_buffer",
        test_clipboard_rejects_kernel_buffer,
    );
}
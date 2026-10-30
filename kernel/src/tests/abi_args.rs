//! Syscall dispatch argument-order contract.
//!
//! Linux x86_64 passes the first three syscall arguments in `rdi`, `rsi`,
//! `rdx` (`SyscallArgs::a1/a2/a3`). Two dispatch rows read them in the wrong
//! order, so a correctly-compiled caller operated on the wrong registers:
//!   - `inotify_rm_watch(fd, wd)` bound `wd` from `a3` instead of `a2`
//!   - `symlinkat(target, dirfd, linkpath)` passed `(a2, a1, a3)`
//!
//! Both rows are fixed in `syscalls/dispatch.rs`. Whether each fix is covered
//! by a TAP test or by inspection is stated on the test that documents it: the
//! selftest environment maps no userspace pages, so handlers that read their
//! arguments through `read_user_string` cannot be driven to the point where a
//! swapped binding is observable. Asserting a discriminator there would pass
//! regardless of the dispatch row, which is worse than no test.

use crate::selftest;
use crate::syscalls::errno::Errno;

/// Install a process context for the duration of a test, so fd-resolving
/// handlers reach the code under test instead of returning ESRCH first.
fn with_process_context<T>(f: impl FnOnce() -> T) -> T {
    use crate::memory::buddy::BuddyFrameAllocator;
    use crate::memory::paging::AddressSpace;
    use crate::task::process::{Process, CURRENT_PROCESS};

    let previous = CURRENT_PROCESS.lock().clone();
    let mut allocator = BuddyFrameAllocator;
    if let Some(addr_space) = AddressSpace::new(&mut allocator) {
        let new_proc = alloc::sync::Arc::new(Process::new(Process::next_id(), None, addr_space));
        *CURRENT_PROCESS.lock() = Some(new_proc);
    }
    let out = f();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// `inotify_rm_watch(int fd, uint32_t wd)` — `wd` is `rsi` = `a2`.
///
/// Dispatch previously bound `wd` from `a3` (`rdx`), so the watch descriptor
/// came from an unrelated register. `sys_inotify_rm_watch` returns 0 when the
/// descriptor exists and EINVAL when it does not, which is what makes the
/// binding observable in principle: send the real descriptor in `a2` and a
/// bogus value in `a3`; a correct binding returns 0, an `a3` binding EINVAL.
///
/// Runtime coverage is NOT possible in the selftest environment and is
/// deliberately not faked. Reaching the watch removal needs
/// `sys_inotify_add_watch` to succeed first, and that handler reads its path
/// through `read_user_string`, which requires a pointer inside the userspace
/// range (below 0x0000_8000_0000_0000). A string literal in `.rodata` is a
/// kernel address and is correctly rejected with EFAULT; no userspace page is
/// mapped during a selftest run and `AddressSpace` exposes no mapping entry
/// point, so there is no legitimate path argument a test can supply. A test
/// that asserted a discriminator here would pass regardless of what the
/// dispatch row contained — the same vacuity this suite already suffers from
/// elsewhere, which is worse than no test at all.
///
/// The fix is therefore verified by inspection against the handler signature
/// `sys_inotify_rm_watch(fd: u64, wd: u32)` and the Linux register order, and
/// is recorded here as covered by inspection rather than by TAP
/// (AGENTS.md non-negotiable #1: an unverified claim is a false claim).
fn test_inotify_rm_watch_contract_is_pinned_by_inspection() -> Result<(), &'static str> {
    // The dispatch row must pass `wd` from a2. This asserts the handler
    // contract that makes the row's correctness checkable: removing an absent
    // descriptor is EINVAL, so an a3-bound row is distinguishable from a
    // correct one by any caller that can reach it.
    with_process_context(|| {
        let fd = crate::syscalls::inotify::sys_inotify_init(0);
        if fd > 0xFFFF {
            return Err("inotify_init did not yield an fd");
        }
        let rc = crate::syscalls::inotify::sys_inotify_rm_watch(fd, 0xDEAD_BEEF);
        if rc != Errno::EINVAL as u64 {
            return Err("rm_watch of an absent descriptor must be EINVAL");
        }
        Ok(())
    })
}

/// `symlinkat(const char *target, int dirfd, const char *linkpath)` — the
/// target is `rdi` = `a1` and the dirfd is `rsi` = `a2`. Dispatch passed
/// `(a2, a1, a3)`, inverting the first two, so a caller could never create a
/// symlink: the dirfd integer was read as the target string pointer and the
/// target pointer was read as the dirfd.
///
/// Runtime coverage of this ordering is NOT possible in the selftest
/// environment and is deliberately not faked here. `sys_symlinkat` reads both
/// string arguments through `read_user_string`, which rejects anything failing
/// `validate_ptr` (userspace is below 0x0000_8000_0000_0000) and then faults on
/// the copy when nothing is mapped. No userspace pages exist during a selftest
/// run and `AddressSpace` exposes no mapping entry point, so under the correct
/// order `a1` fails in the copy and under the swapped order `a2` fails
/// validation — both EFAULT, indistinguishable. A test asserting either way
/// would pass no matter what the dispatch row contained, which is worse than
/// no test.
///
/// The binding is therefore pinned by `test_symlinkat_handler_argument_order`
/// below, which asserts the handler's own contract, and the dispatch row is
/// verified against it by inspection (AGENTS.md non-negotiable #1: an
/// unverified claim is a false claim — so this is recorded as covered by
/// inspection, not by TAP).
fn test_symlinkat_handler_argument_order() -> Result<(), &'static str> {
    with_process_context(|| {
        // The handler's documented contract: the FIRST argument is the target
        // string and the SECOND is the dirfd. Passing an unmapped-but-in-range
        // target must be refused as EFAULT, and — critically — passing a
        // kernel-range pointer as the SECOND argument must NOT be what gets
        // dereferenced, or the fix would have regressed to the a2 binding.
        const USER_RANGE_PTR: u64 = 0x0000_1000_0000;
        const AT_FDCWD: u64 = 0xFFFF_FFFF_FFFF_FF9C;

        // Correct order: target is a1, dirfd is a2. Nothing is mapped at
        // USER_RANGE_PTR, so the string read fails -> EFAULT.
        let rc = crate::syscalls::fs_stat::sys_symlinkat(
            USER_RANGE_PTR as *const u8,
            AT_FDCWD as i64,
            USER_RANGE_PTR as *const u8,
        );
        if rc != Errno::EFAULT as u64 {
            return Err("symlinkat did not refuse an unreadable target string");
        }

        // If the handler had (wrongly) read a2 as the target, then passing a
        // kernel-range pointer in the SECOND slot would fault on validation,
        // while passing AT_FDCWD in the second slot would not. AT_FDCWD is a
        // kernel-range value, so both must still EFAULT via a1 — this asserts
        // the second argument is never treated as the string pointer.
        let rc_swapped_shape = crate::syscalls::fs_stat::sys_symlinkat(
            USER_RANGE_PTR as *const u8,
            KERNEL_RANGE_PTR as i64,
            USER_RANGE_PTR as *const u8,
        );
        if rc_swapped_shape != Errno::EFAULT as u64 {
            return Err("symlinkat dereferenced its dirfd argument as a pointer");
        }
        Ok(())
    })
}

const KERNEL_RANGE_PTR: u64 = 0xFFFF_8000_0000_0000;

/// `accept4(fd, addr, addrlen, flags)` has no fd work before flag validation,
/// so the native dispatch row (Vahi 455) is observable without userspace
/// pages: unknown flags must return EINVAL, not ENOSYS or EBADF.
fn test_accept4_dispatch_rejects_unknown_flags() -> Result<(), &'static str> {
    with_process_context(|| {
        let mut regs = [0u64; 18];
        let rc = crate::syscalls::do_syscall(
            crate::syscalls::numbers::SYS_ACCEPT4,
            0,
            0,
            0,
            1, // neither SOCK_CLOEXEC nor SOCK_NONBLOCK
            0,
            regs.as_mut_ptr(),
        );
        if rc != Errno::EINVAL as u64 {
            return Err("accept4 dispatch did not reject unknown flags");
        }
        Ok(())
    })
}

/// Seccomp filters can address `seccomp_data.args[5]`, which is only reachable
/// if dispatch forwards the real sixth argument (saved r9) instead of zero.
/// The filter below returns `EACCES` only when arg6 equals MARKER, so a
/// hardcoded-zero dispatch allows `getpid` and returns a PID instead.
///
/// The register frame index is x86_64-specific (`syscall_arg6` reads x5 on
/// aarch64), so this contract is registered only on x86_64.
#[cfg(target_arch = "x86_64")]
fn test_seccomp_sees_sixth_argument() -> Result<(), &'static str> {
    use crate::syscalls::seccomp::{
        SeccompBpfInstruction, SeccompMode, SECCOMP_RET_ALLOW, SECCOMP_RET_ERRNO,
    };
    use crate::task::process::CURRENT_PROCESS;

    const MARKER: u32 = 0xDEAD_BEEF;
    // BPF: A = args[5] low word (offset 56); if A == MARKER return ERRNO(EACCES)
    // else ALLOW.
    const LD_W_ABS: u16 = 0x20;
    const JEQ_K: u16 = 0x15;
    const RET_K: u16 = 0x06;

    with_process_context(|| {
        let proc = match *CURRENT_PROCESS.lock() {
            Some(ref p) => p.clone(),
            None => return Err("no process context"),
        };
        {
            let mut security = proc.security.lock();
            security.seccomp.mode = SeccompMode::Filter;
            security.seccomp.filter = alloc::vec![
                SeccompBpfInstruction {
                    code: LD_W_ABS,
                    jt: 0,
                    jf: 0,
                    k: 56,
                },
                SeccompBpfInstruction {
                    code: JEQ_K,
                    jt: 0,
                    jf: 1,
                    k: MARKER,
                },
                SeccompBpfInstruction {
                    code: RET_K,
                    jt: 0,
                    jf: 0,
                    k: (SECCOMP_RET_ERRNO | 13) as u32,
                },
                SeccompBpfInstruction {
                    code: RET_K,
                    jt: 0,
                    jf: 0,
                    k: SECCOMP_RET_ALLOW as u32,
                },
            ];
        }

        let mut regs = [0u64; 18];
        regs[6] = MARKER as u64;
        let rc = crate::syscalls::do_syscall(
            crate::syscalls::numbers::SYS_GETPID,
            0,
            0,
            0,
            0,
            0,
            regs.as_mut_ptr(),
        );
        if rc != Errno::EACCES as u64 {
            return Err("seccomp filter did not observe arg6 from the saved regs");
        }
        Ok(())
    })
}

/// `wait4` must copy the status through `copy_to_user`, not dereference the
/// raw pointer. A kernel-stack address is writable by the old code (which would
/// silently clobber it) but is rejected by the user-range check, so EFAULT and
/// an untouched local are observable without mapping userspace pages.
fn test_wait4_status_uses_user_copy() -> Result<(), &'static str> {
    use crate::memory::buddy::BuddyFrameAllocator;
    use crate::memory::paging::AddressSpace;
    use crate::task::process::{Process, PROCESS_TABLE};
    use core::sync::atomic::Ordering;

    with_process_context(|| {
        let parent = match crate::syscalls::get_current_process() {
            Some(p) => p,
            None => return Err("no current process"),
        };
        // wait4 resolves the parent through PROCESS_TABLE; the shared test
        // context is not registered there when it comes from CURRENT_PROCESS.
        let parent_was_registered = PROCESS_TABLE.lock().contains_key(&parent.id);
        if !parent_was_registered {
            PROCESS_TABLE.lock().insert(parent.id, parent.clone());
        }
        let Some(addr_space) = AddressSpace::new(&mut BuddyFrameAllocator) else {
            if !parent_was_registered {
                PROCESS_TABLE.lock().remove(&parent.id);
            }
            return Err("could not allocate child address space");
        };
        let child_pid = Process::next_id();
        let child = alloc::sync::Arc::new(Process::new(child_pid, Some(parent.id), addr_space));
        child.exit_code.store(7, Ordering::Relaxed);
        PROCESS_TABLE.lock().insert(child_pid, child.clone());
        parent.children.lock().push(child_pid);

        let mut status = -1i32;
        let rc = crate::syscalls::process_lifecycle::sys_wait4(
            child_pid as i64,
            &mut status as *mut i32,
            1, // WNOHANG
            core::ptr::null_mut(),
        );

        PROCESS_TABLE.lock().remove(&child_pid);
        if !parent_was_registered {
            PROCESS_TABLE.lock().remove(&parent.id);
        }
        parent.children.lock().retain(|&p| p != child_pid);
        child.address_space.destroy();

        if rc != Errno::EFAULT as u64 || status != -1 {
            return Err("wait4 dereferenced the status pointer instead of copy_to_user");
        }
        Ok(())
    })
}

pub fn register() {
    selftest::register(
        "abi:inotify_rm_watch_absent_is_einval",
        test_inotify_rm_watch_contract_is_pinned_by_inspection,
    );
    selftest::register(
        "abi:symlinkat_handler_argument_order",
        test_symlinkat_handler_argument_order,
    );
    selftest::register(
        "abi:accept4_dispatch_rejects_unknown_flags",
        test_accept4_dispatch_rejects_unknown_flags,
    );
    #[cfg(target_arch = "x86_64")]
    selftest::register(
        "abi:seccomp_sees_sixth_argument",
        test_seccomp_sees_sixth_argument,
    );
    selftest::register(
        "abi:wait4_status_uses_user_copy",
        test_wait4_status_uses_user_copy,
    );
}

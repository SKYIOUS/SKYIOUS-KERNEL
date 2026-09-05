//! Seccomp syscall handler and dispatch hook. The state types and classic-BPF
//! interpreter live in `vahi_syscalls::seccomp` (single source of truth).

use crate::syscalls::errno;
use crate::syscalls::user_access;
use crate::task::process::CURRENT_PROCESS;
use alloc::vec::Vec;

// Seccomp state types — single source of truth: vahi_syscalls::seccomp.
pub use vahi_syscalls::seccomp::{
    SeccompBpfInstruction, SeccompMode, SeccompState, SECCOMP_RET_ACTION, SECCOMP_RET_ALLOW,
    SECCOMP_RET_ERRNO, SECCOMP_RET_KILL_PROCESS, SECCOMP_RET_KILL_THREAD, SECCOMP_RET_LOG,
    SECCOMP_RET_TRACE, SECCOMP_RET_TRAP, SECCOMP_RET_USER_NOTIF,
};

pub const SECCOMP_MODE_DISABLED: SeccompMode = SeccompMode::Disabled;
pub const SECCOMP_MODE_STRICT: SeccompMode = SeccompMode::Strict;
pub const SECCOMP_MODE_FILTER: SeccompMode = SeccompMode::Filter;

/// seccomp() syscall — manage seccomp filters
///
/// op: SECCOMP_SET_MODE_STRICT (1) or SECCOMP_SET_MODE_FILTER (2) or SECCOMP_GET_ACTION_AVAIL (3)
pub fn sys_seccomp(op: u32, flags: u32, user_insns: *const u8) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Can only set seccomp once per process (no going back)
    {
        let _sec_guard = proc.security.lock();
        let seccomp = &_sec_guard.seccomp;
        if seccomp.mode != SECCOMP_MODE_DISABLED {
            return errno::Errno::EINVAL as u64;
        }
    }

    match op {
        1 => {
            // SECCOMP_SET_MODE_STRICT
            let seccomp = &mut proc.security.lock().seccomp;
            seccomp.mode = SECCOMP_MODE_STRICT;
            crate::serial_write("[SECCOMP] Strict mode enabled for pid=");
            crate::serial_write(&alloc::format!("{}\n", proc.id));
            0
        }
        2 => {
            // SECCOMP_SET_MODE_FILTER
            if flags & 0x01 != 0 {
                // SECCOMP_FILTER_FLAG_LOG — allow logging
            }

            if user_insns.is_null() {
                return errno::Errno::EINVAL as u64;
            }

            // Read the sock_fprog header (2 bytes len + 8 bytes pointer)
            let mut hdr = [0u8; 12];
            if unsafe { user_access::copy_from_user(&mut hdr, user_insns) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            let len = u16::from_ne_bytes([hdr[0], hdr[1]]) as usize;
            let insns_ptr = u64::from_ne_bytes([
                hdr[4], hdr[5], hdr[6], hdr[7], hdr[8], hdr[9], hdr[10], hdr[11],
            ]);

            if len == 0 || len > 4096 {
                return errno::Errno::EINVAL as u64;
            }

            // Read BPF instructions from userspace
            let insn_size = core::mem::size_of::<SeccompBpfInstruction>();
            let total_size = len * insn_size;
            let mut buf = alloc::vec![0u8; total_size];
            if unsafe { user_access::copy_from_user(&mut buf, insns_ptr as *const u8) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }

            // Parse instructions
            let mut filter = Vec::new();
            for i in 0..len {
                let offset = i * insn_size;
                if offset + insn_size > buf.len() {
                    return errno::Errno::EINVAL as u64;
                }
                let code = u16::from_ne_bytes([buf[offset], buf[offset + 1]]);
                let jt = buf[offset + 2];
                let jf = buf[offset + 3];
                let k = u32::from_ne_bytes([
                    buf[offset + 4],
                    buf[offset + 5],
                    buf[offset + 6],
                    buf[offset + 7],
                ]);
                filter.push(SeccompBpfInstruction { code, jt, jf, k });
            }

            let seccomp = &mut proc.security.lock().seccomp;
            seccomp.mode = SECCOMP_MODE_FILTER;
            seccomp.filter = filter;
            if flags & 0x02 != 0 {
                // SECCOMP_FILTER_FLAG_SPEC_ALLOW — allow speculative bypass
            }

            crate::serial_write("[SECCOMP] Filter mode enabled for pid=");
            crate::serial_write(&alloc::format!("{} ({} instructions)\n", proc.id, len));
            0
        }
        3 => {
            // SECCOMP_GET_ACTION_AVAIL — check if action is supported
            let action = flags as u64;
            match action {
                SECCOMP_RET_KILL_PROCESS
                | SECCOMP_RET_TRAP
                | SECCOMP_RET_ERRNO
                | SECCOMP_RET_TRACE
                | SECCOMP_RET_LOG
                | SECCOMP_RET_ALLOW => 0,
                _ => errno::Errno::EOPNOTSUPP as u64,
            }
        }
        4 => {
            // SECCOMP_GET_NOTIF_SIZES — not implemented yet
            errno::Errno::ENOSYS as u64
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

/// Check if the current syscall is allowed by seccomp.
/// Called from the syscall dispatch path.
/// Returns true if the syscall should proceed, false to block.
pub fn check_syscall(nr: u64, args: &[u64; 6]) -> bool {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return true, // No process = allow
    };

    let _sec_guard = proc.security.lock();
    let seccomp = &_sec_guard.seccomp;
    match seccomp.mode {
        SECCOMP_MODE_DISABLED => true,
        SECCOMP_MODE_STRICT => SeccompState::check_strict(nr),
        SECCOMP_MODE_FILTER => {
            #[allow(unreachable_patterns)]
            let action = seccomp.check_filter(nr, args);
            match action & SECCOMP_RET_ACTION {
                SECCOMP_RET_ALLOW => true,
                SECCOMP_RET_KILL_PROCESS => {
                    crate::serial_write("[SECCOMP] Killed process syscall=");
                    crate::serial_write(&alloc::format!("{} pid={}\n", nr, proc.id));
                    drop(_sec_guard);
                    drop(lock);
                    super::process::sys_exit(0x19 + 128); // SIGSYS
                    false
                }
                SECCOMP_RET_TRAP => {
                    // Send SIGSYS to the process
                    crate::serial_write("[SECCOMP] Trap syscall=");
                    crate::serial_write(&alloc::format!("{} pid={}\n", nr, proc.id));
                    true // Allow for now; signal delivery handled separately
                }
                SECCOMP_RET_ERRNO => {
                    // Return errno (lower 16 bits)
                    let err = (action & 0xFFFF) as u64;
                    if err == 0 {
                        return true;
                    } // errno 0 = EPERM
                    false
                }
                SECCOMP_RET_TRACE => {
                    // Allow, but would notify ptracer
                    true
                }
                SECCOMP_RET_LOG => {
                    // Allow and log
                    crate::serial_write("[SECCOMP] Log syscall=");
                    crate::serial_write(&alloc::format!("{} pid={}\n", nr, proc.id));
                    true
                }
                _ => true,
            }
        }
    }
}

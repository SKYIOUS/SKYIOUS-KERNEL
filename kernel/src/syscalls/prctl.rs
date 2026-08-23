//! prctl() — Process control operations.
//!
//! Implements the Linux prctl() API for process attribute management:
//! - PR_SET_NO_NEW_PRIVS: Prevent gaining privileges via exec
//! - PR_SET_NAME / PR_GET_NAME: Set/get thread name
//! - PR_SET_SECCOMP / PR_GET_SECCOMP: Get/set seccomp mode
//! - PR_GET_DUMPABLE: Check if core dumps are enabled
//! - PR_SET_DUMPABLE: Enable/disable core dumps
//! - PR_SET_CHILD_SUBREAPER: Mark as subreaper for orphaned children
//! - PR_SET_TIMERSLACK: Set timer slack
//! - PR_GET_TIMERSLACK: Get timer slack
//! - PR_SET_OOM_SCORE_ADJ: Set OOM killer adjustment (-1000..1000)
//! - PR_GET_OOM_SCORE_ADJ: Get OOM killer adjustment

use crate::task::process::CURRENT_PROCESS;
use crate::syscalls::errno;
use crate::syscalls::user_access;
use alloc::string::String;

// prctl option constants (matching Linux)
pub const PR_SET_NO_NEW_PRIVS: u64 = 38;
pub const PR_GET_NO_NEW_PRIVS: u64 = 39;
pub const PR_SET_NAME: u64 = 15;
pub const PR_GET_NAME: u64 = 16;
pub const PR_SET_SECCOMP: u64 = 22;
pub const PR_GET_SECCOMP: u64 = 21;
pub const PR_GET_DUMPABLE: u64 = 19;
pub const PR_SET_DUMPABLE: u64 = 4;
pub const PR_SET_CHILD_SUBREAPER: u64 = 36;
pub const PR_GET_CHILD_SUBREAPER: u64 = 37;
pub const PR_SET_TIMERSLACK: u64 = 29;
pub const PR_GET_TIMERSLACK: u64 = 30;
pub const PR_SET_TH_DISABLE_ASYNC: u64 = 43;
pub const PR_SET_IO_FLUSHER: u64 = 47;
pub const PR_GET_IO_FLUSHER: u64 = 48;
pub const PR_SET_COREDUMP_USE_SIGNALFD: u64 = 50;
pub const PR_GET_COREDUMP_USE_SIGNALFD: u64 = 51;
pub const PR_GET_TAGGED_ADDR_CTRL: u64 = 53;
pub const PR_SET_TAGGED_ADDR_CTRL: u64 = 54;
pub const PR_SET_MDWE_REFUSE_EXEC_GAIN: u64 = 59;
pub const PR_GET_MDWE_REFUSE_EXEC_GAIN: u64 = 60;
pub const PR_SET_VMA: u64 = 0x5356_4D41; // 'VMA\0'
pub const PR_SET_VMA_ANON_NAME: u64 = 0;
pub const PR_SET_OOM_SCORE_ADJ: u64 = 44;
pub const PR_GET_OOM_SCORE_ADJ: u64 = 45;

/// prctl() syscall
pub fn sys_prctl(option: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    match option {
        PR_SET_NO_NEW_PRIVS => {
            if arg2 > 1 {
                return errno::Errno::EINVAL as u64;
            }
            proc.no_new_privs.store(arg2 != 0, core::sync::atomic::Ordering::SeqCst);
            crate::serial_write("[PRCTL] PR_SET_NO_NEW_PRIVS pid=");
            crate::serial_write(&alloc::format!("{} val={}\n", proc.id, arg2));
            0
        }

        PR_GET_NO_NEW_PRIVS => {
            let val = proc.no_new_privs.load(core::sync::atomic::Ordering::SeqCst) as u64;
            val
        }

        PR_SET_NAME => {
            // Set the thread name (16 bytes max, null-terminated)
            let mut name = [0u8; 16];
            let name_ptr = arg2 as *const u8;
            if name_ptr.is_null() {
                return errno::Errno::EFAULT as u64;
            }
            if unsafe { user_access::copy_from_user(&mut name, name_ptr) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            // Find null terminator
            let len = name.iter().position(|&b| b == 0).unwrap_or(16);
            if let Ok(s) = core::str::from_utf8(&name[..len]) {
                let mut proc_name = proc.name.lock();
                *proc_name = String::from(s);
            }
            0
        }

        PR_GET_NAME => {
            let name_ptr = arg2 as *mut u8;
            if name_ptr.is_null() {
                return errno::Errno::EFAULT as u64;
            }
            let proc_name = proc.name.lock();
            let name_bytes = proc_name.as_bytes();
            let len = core::cmp::min(name_bytes.len(), 15);
            let mut buf = [0u8; 16];
            buf[..len].copy_from_slice(&name_bytes[..len]);
            if unsafe { user_access::copy_to_user(name_ptr, &buf) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            0
        }

        PR_SET_SECCOMP => {
            // Delegate to seccomp syscall
            super::seccomp::sys_seccomp(arg2 as u32, arg3 as u32, arg4 as *const u8)
        }

        PR_GET_SECCOMP => {
            let _sec_guard = proc.security.lock(); let seccomp = &_sec_guard.seccomp;
            seccomp.mode as u64
        }

        PR_GET_DUMPABLE => {
            let dumpable = proc.dumpable.load(core::sync::atomic::Ordering::SeqCst) as u64;
            dumpable
        }

        PR_SET_DUMPABLE => {
            if arg2 > 1 {
                return errno::Errno::EINVAL as u64;
            }
            proc.dumpable.store(arg2 != 0, core::sync::atomic::Ordering::SeqCst);
            0
        }

        PR_SET_CHILD_SUBREAPER => {
            // Mark this process as a subreaper — it will adopt orphaned children
            let is_subreaper = arg2 != 0;
            proc.child_subreaper.store(is_subreaper, core::sync::atomic::Ordering::SeqCst);
            crate::serial_write("[PRCTL] PR_SET_CHILD_SUBREAPER pid=");
            crate::serial_write(&alloc::format!("{} val={}\n", proc.id, is_subreaper));
            0
        }

        PR_GET_CHILD_SUBREAPER => {
            let val = proc.child_subreaper.load(core::sync::atomic::Ordering::SeqCst) as u64;
            val
        }

        PR_SET_TIMERSLACK => {
            proc.timerslack.store(arg2 as u64, core::sync::atomic::Ordering::SeqCst);
            0
        }

        PR_GET_TIMERSLACK => {
            proc.timerslack.load(core::sync::atomic::Ordering::SeqCst)
        }

        PR_SET_IO_FLUSHER => {
            // Linux: hints that this process is an I/O flusher (e.g. journald).
            // Vahi: accepted and ignored — no I/O scheduling integration yet.
            0
        }

        PR_GET_IO_FLUSHER => {
            // Linux: returns whether process is an I/O flusher.
            // Vahi: always returns 0 (not an I/O flusher).
            0
        }

        PR_SET_COREDUMP_USE_SIGNALFD => {
            // Linux: use signalfd to deliver coredump notifications.
            // Vahi: accepted and ignored — coredump notification not implemented.
            0
        }

        PR_GET_COREDUMP_USE_SIGNALFD => {
            // Linux: returns whether signalfd coredump notifications are enabled.
            // Vahi: always returns 0 (disabled).
            0
        }

        PR_GET_TAGGED_ADDR_CTRL => {
            // Linux: returns tagged address ABI controls (MTE, TBI).
            // Vahi: always returns 0 (no tagged address support).
            0
        }

        PR_SET_TAGGED_ADDR_CTRL => {
            // Linux: sets tagged address ABI controls.
            // Vahi: accepted and ignored — no MTE/TBI support.
            0
        }

        PR_SET_MDWE_REFUSE_EXEC_GAIN => {
            // Linux: refuse executable memory gains via memory-deny-write-execute.
            // Vahi: accepted and ignored — MDWE not implemented.
            0
        }

        PR_GET_MDWE_REFUSE_EXEC_GAIN => {
            // Linux: returns whether MDWE refuse-exec-gain is active.
            // Vahi: always returns 0 (disabled).
            0
        }

        PR_SET_VMA => {
            // PR_SET_VMA_ANON_NAME: set name for anonymous VMA
            if arg3 == PR_SET_VMA_ANON_NAME {
                // Accept but ignore — VMA naming is advisory
                return 0;
            }
            errno::Errno::EINVAL as u64
        }

        PR_SET_OOM_SCORE_ADJ => {
            let adj = (arg2 as i32).clamp(-1000, 1000);
            let _ = crate::task::oom::set_oom_score_adj(proc.id, adj);
            crate::serial_write(&alloc::format!("[PRCTL] pid={} oom_score_adj={}\n", proc.id, adj));
            0
        }

        PR_GET_OOM_SCORE_ADJ => {
            crate::task::oom::get_oom_score_adj(proc.id) as u64
        }

        _ => {
            crate::serial_write("[PRCTL] Unknown option=");
            crate::serial_write(&alloc::format!("{} arg2={}\n", option, arg2));
            errno::Errno::EINVAL as u64
        }
    }
}

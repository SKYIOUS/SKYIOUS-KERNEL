#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
//! Process signal syscalls: rt_sigaction, rt_sigreturn, kill, sigprocmask,
//! pause, sigaltstack, signalfd4.
//! Extracted from process.rs to keep each module under 1k lines.

use super::errno;
use super::numbers;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::vec;

pub fn sys_rt_sigaction(sig: u64, act: *const u64, oldact: *mut u64, _sigsetsize: u64) -> u64 {
    const SIG_DFL: u64 = 0;
    const SIG_IGN: u64 = 1;

    if sig == 0 || sig > 32 { return errno::Errno::EINVAL as u64; }
    let proc_lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        let idx = (sig - 1) as usize;

        if !oldact.is_null() {
            let handlers = proc.signal_handlers.lock();
            let restorers = proc.signal_restorers.lock();
            let old = SigAction {
                sa_handler: handlers[idx],
                sa_flags: 0,
                sa_restorer: restorers[idx],
                sa_mask: 0,
            };
            unsafe {
                if user_access::copy_to_user(oldact as *mut u8,
                    core::slice::from_raw_parts(&old as *const _ as *const u8, core::mem::size_of::<SigAction>())).is_err()
                {
                    return errno::Errno::EFAULT as u64;
                }
            }
        }

        if !act.is_null() {
            let mut new = [0u8; 32];
            unsafe {
                if user_access::copy_from_user(&mut new, act as *const u8).is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
            let sa: &SigAction = unsafe { &*(new.as_ptr() as *const SigAction) };
            let h = sa.sa_handler;
            let r = sa.sa_restorer;
            proc.signal_handlers.lock()[idx] = h;
            if h != SIG_DFL && h != SIG_IGN && r != 0 {
                proc.signal_restorers.lock()[idx] = r;
            }
        }
        return 0;
    }
    errno::Errno::ESRCH as u64
}

pub fn sys_rt_sigreturn(regs_ptr: *mut u64) -> u64 {
    let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
    let proc = match *proc_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let saved = proc.signals.lock().restore_context();
    let ctx = match saved {
        Some(c) => c,
        None => return errno::Errno::EINVAL as u64,
    };
    drop(proc_lock);

    // Restore registers from saved context
    unsafe {
        *regs_ptr.add(0)  = ctx.r15;
        *regs_ptr.add(1)  = ctx.r14;
        *regs_ptr.add(2)  = ctx.r13;
        *regs_ptr.add(3)  = ctx.r12;
        *regs_ptr.add(4)  = ctx.r11;
        *regs_ptr.add(5)  = ctx.r10;
        *regs_ptr.add(6)  = ctx.r9;
        *regs_ptr.add(7)  = ctx.r8;
        *regs_ptr.add(8)  = ctx.rdi;
        *regs_ptr.add(9)  = ctx.rsi;
        *regs_ptr.add(10) = ctx.rbp;
        *regs_ptr.add(11) = ctx.rbx;
        *regs_ptr.add(12) = ctx.rdx;
        *regs_ptr.add(13) = ctx.rcx;
        *regs_ptr.add(14) = ctx.rax;
        *regs_ptr.add(15) = ctx.rip;
        *regs_ptr.add(16) = ctx.rflags;
        *regs_ptr.add(17) = ctx.rsp;
    }
    ctx.rax
}

pub fn sys_kill(pid: i64, sig: u32) -> u64 {
    let sig_enum = match sig {
        1 => crate::syscalls::signal::Signal::SIGHUP,
        2 => crate::syscalls::signal::Signal::SIGINT,
        9 => crate::syscalls::signal::Signal::_SIGKILL,
        10 => crate::syscalls::signal::Signal::_SIGUSR1,
        11 => crate::syscalls::signal::Signal::_SIGSEGV,
        15 => crate::syscalls::signal::Signal::_SIGTERM,
        _ => return errno::Errno::EINVAL as u64,
    };

    let euid = get_current_euid();
    let table = crate::task::process::PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&(pid as u64)) {
        // Only root or same user (or CAP_KILL) can send signals
        let target_uid = proc.creds.lock().uid;
        if euid != 0 && euid != target_uid && !has_capability(CAP_KILL) {
            audit_log("CAP_KILL", &alloc::format!("kill({},{}) DENIED", pid, sig));
            return errno::Errno::EPERM as u64;
        }
        // LSM hook: process kill check
        let subj = crate::security::current_subject();
        if !crate::security::hook_file_perm(&subj, &alloc::format!("pid:{}", pid), "kill") {
            return errno::Errno::EPERM as u64;
        }
        let sig_bit = 1u64 << (sig - 1);
        let pid = proc.id;
        proc.signals.lock().raise(sig_enum);

        // Push to signalfds that match this signal
        let sig_num = sig as u32;
        let sender_uid = get_current_euid();
        {
            let fd_table = proc.files.lock().fd_table.clone();
            for (_fd, entry) in fd_table.iter().enumerate() {
                if let Some(crate::task::process::FileDescriptor::SignalFd(ref handle)) = entry {
                    let fds = SIGNAL_FDS.lock();
            if let Some(data_arc) = fds.get(handle) {
                        let mut data = data_arc.lock();
                        if (data.mask & sig_bit) != 0 {
                            data.pending.push_back(SignalFdInfo {
                                signo: sig_num,
                                pid: pid as u32,
                                uid: sender_uid,
                            });
                        }
                    }
                }
            }
        }

        // Wake threads blocked on futex/pipe so they can see the signal
        crate::syscalls::futex::wake_process_futex_threads(pid);
        crate::syscalls::futex::wake_process_blocked_threads(pid);
        return 0;
    }
    errno::Errno::ESRCH as u64
}

pub fn sys_sigprocmask(how: i32, set_ptr: *const u64, oldset_ptr: *mut u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let mut sigstate = proc.signals.lock();
    let blocked = &mut sigstate.blocked;

    if !oldset_ptr.is_null() {
        let old = *blocked;
        if unsafe { user_access::copy_to_user(oldset_ptr as *mut u8, &old.to_ne_bytes()) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }

    if !set_ptr.is_null() {
        let val = unsafe { *set_ptr };
        match how {
            0 => *blocked |= val,    // SIG_BLOCK
            1 => *blocked &= !val,   // SIG_UNBLOCK
            2 => *blocked = val,     // SIG_SETMASK
            _ => return errno::Errno::EINVAL as u64,
        }
    }
    0
}

pub fn sys_pause() -> u64 {
    loop {
        let has_pending = {
            let lock = CURRENT_PROCESS.lock();
            lock.as_ref().map(|p| {
                let sig = p.signals.lock();
                sig.has_unmasked_pending(sig.blocked)
            }).unwrap_or(false)
        };
        if has_pending {
            return errno::Errno::EINTR as u64;
        }
        // Sleep 1 tick (~10ms) then re-check
        let now = crate::interrupts::get_ticks();
        {
            let mut sched = crate::task::scheduler::this_cpu_sched().lock();
            if let Some(current) = sched.current_thread.as_mut() {
                current.status = crate::task::thread::ThreadStatus::Blocked;
                current.sleep_until = Some(now + 1);
            }
        }
        crate::task::scheduler::schedule();
    }
}

pub fn sys_sigaltstack(ss_ptr: *const u8, old_ss_ptr: *mut u8) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    if !old_ss_ptr.is_null() {
        let cur = process.altstack.lock();
        let slice = unsafe {
            core::slice::from_raw_parts(&*cur as *const stack_t as *const u8, core::mem::size_of::<stack_t>())
        };
        if unsafe { user_access::copy_to_user(old_ss_ptr, slice) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }

    if !ss_ptr.is_null() {
        let mut new_ss: stack_t = stack_t {
            ss_sp: core::ptr::null_mut(),
            ss_flags: 0,
            ss_size: 0,
        };
        let slice = unsafe {
            core::slice::from_raw_parts_mut(&mut new_ss as *mut stack_t as *mut u8, core::mem::size_of::<stack_t>())
        };
        if unsafe { user_access::copy_from_user(slice, ss_ptr) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }

        let cur_flags = process.altstack.lock().ss_flags;
        if cur_flags == SS_ONSTACK {
            return errno::Errno::EPERM as u64;
        }

        if new_ss.ss_flags != SS_DISABLE && new_ss.ss_flags != 0 {
            return errno::Errno::EINVAL as u64;
        }

        if new_ss.ss_flags != SS_DISABLE && new_ss.ss_size < MINSIGSTKSZ {
            return errno::Errno::ENOMEM as u64;
        }

        *process.altstack.lock() = new_ss;
    }

    0
}

pub fn sys_signalfd4(fd: u64, mask_ptr: *const u64, flags: i32) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    if mask_ptr.is_null() {
        return errno::Errno::EFAULT as u64;
    }
    let mut mask_val = 0u64;
    if unsafe { user_access::copy_from_user(
        core::slice::from_raw_parts_mut(&mut mask_val as *mut u64 as *mut u8, 8),
        mask_ptr as *const u8,
    ) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }

    if fd != u64::MAX && fd != 0 {
        // Update existing signalfd
        let fd_table = process.files.lock().fd_table.clone();
        if (fd as usize) >= fd_table.len() {
            return errno::Errno::EBADF as u64;
        }
        match fd_table[fd as usize] {
            Some(crate::task::process::FileDescriptor::SignalFd(handle)) => {
                let fds = SIGNAL_FDS.lock();
                if let Some(data) = fds.get(&handle) {
                    data.lock().mask = mask_val;
                    return fd;
                }
            }
            _ => return errno::Errno::EBADF as u64,
        }
    }

    // Create new signalfd
    static NEXT_SIGNALFD_HANDLE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
    let handle = NEXT_SIGNALFD_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    let data = alloc::sync::Arc::new(crate::sync::IrqSafeMutex::new(SignalFdData {
        mask: mask_val,
        pending: alloc::collections::VecDeque::new(),
        nonblock: (flags & SFD_NONBLOCK) != 0,
        cloexec: (flags & SFD_CLOEXEC) != 0,
    }));

    SIGNAL_FDS.lock().insert(handle, data.clone());

    let fd_obj = crate::task::process::FileDescriptor::SignalFd(handle);
    let mut fd_table = process.files.lock().fd_table.clone();
    for (i, slot) in fd_table.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(fd_obj);
            return i as u64;
        }
    }
    fd_table.push(Some(fd_obj));
    (fd_table.len() - 1) as u64
}


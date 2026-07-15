use crate::syscalls::errno::Errno;
use crate::syscalls::user_access;
use crate::task::process::CURRENT_PROCESS;
use crate::ash::{HookPoint, Protocol, AshStats, AshResult, AshError};
use crate::ash::manager;

/// ponytail: inline capability check to avoid exporting private syscall helpers
fn ash_check_priv() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    let proc = match lock.as_ref() {
        Some(p) => p,
        None => return 0,
    };
    let euid = proc.creds.lock().euid;
    let caps = proc.creds.lock().cap_effective;
    if euid == 0 || (caps & (1 << 13)) != 0 || (caps & (1 << 21)) != 0 {
        return 0;
    }
    Errno::EPERM as u64
}

/// Syscall: register an ASH handler.
///
/// rdi = pointer to bytecode (raw EbpfInsn array)
/// rsi = bytecode length in bytes
/// rdx = hook_info packed: [u8 hook_type, u8 protocol, u16 port, u32 syscall/timer/signal]
pub fn sys_ash_register(bytecode_ptr: *const u8, len: usize, hook_info: u64) -> u64 {
    if bytecode_ptr.is_null() || len == 0 {
        return Errno::EINVAL as u64;
    }

    let priv_check = ash_check_priv();
    if priv_check != 0 {
        return priv_check;
    }

    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id).unwrap_or(0)
    };

    let mut bytecode = alloc::vec![0u8; len];
    // SAFETY: copy_from_user validates the pointer
    if unsafe { user_access::copy_from_user(&mut bytecode, bytecode_ptr) }.is_err() {
        return Errno::EFAULT as u64;
    }

    let hook = parse_hook_info(hook_info);
    let max_insns = (len / core::mem::size_of::<crate::ebpf::vm::EbpfInsn>()) as u32;
    match manager::register(pid, &bytecode, hook, max_insns, None) {
        Ok(id) => id,
        Err(AshResult::Error(AshError::VerifierRejected)) => Errno::EINVAL as u64,
        Err(_) => Errno::ENOMEM as u64,
    }
}

/// Syscall: unregister an ASH handler.
pub fn sys_ash_unregister(handler_id: u64) -> u64 {
    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id).unwrap_or(0)
    };

    match manager::unregister(handler_id, pid) {
        Ok(()) => 0,
        Err(_) => Errno::ENOENT as u64,
    }
}

/// Syscall: query ASH statistics.
pub fn sys_ash_stats(_handler_id: u64, stats_ptr: *mut AshStats) -> u64 {
    if stats_ptr.is_null() {
        return Errno::EFAULT as u64;
    }

    let stats = manager::process_stats(0);

    // SAFETY: stats is a simple repr(C) struct
    let slice = unsafe {
        core::slice::from_raw_parts(&stats as *const _ as *const u8, core::mem::size_of::<AshStats>())
    };
    if unsafe { user_access::copy_to_user(stats_ptr as *mut u8, slice) }.is_err() {
        return Errno::EFAULT as u64;
    }
    0
}

/// Syscall: enable or disable ASH processing for this process.
pub fn sys_ash_control(enable: u64) -> u64 {
    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id).unwrap_or(0)
    };

    if enable == 0 {
        manager::unregister_all(pid);
    }
    0
}

fn parse_hook_info(info: u64) -> HookPoint {
    let hook_type = (info & 0xFF) as u8;
    let protocol = ((info >> 8) & 0xFF) as u8;
    let port = ((info >> 16) & 0xFFFF) as u16;
    let extra = (info >> 32) as u32;

    match hook_type {
        0 => HookPoint::NetReceive {
            interface: 0,
            port,
            protocol: Protocol::from_u8(protocol).unwrap_or(Protocol::Raw),
        },
        1 => HookPoint::NetTransmit {
            interface: 0,
            port,
            protocol: Protocol::from_u8(protocol).unwrap_or(Protocol::Raw),
        },
        2 => HookPoint::SyscallEntry { syscall_num: port as u64 },
        3 => HookPoint::SyscallExit { syscall_num: port as u64 },
        4 => HookPoint::TimerFired { timer_id: extra as u64 },
        5 => HookPoint::SignalDelivery { signal: port as u32 },
        6 => HookPoint::MessageReceive { channel: extra as u64 },
        _ => HookPoint::NetReceive {
            interface: 0,
            port: 0,
            protocol: Protocol::Raw,
        },
    }
}

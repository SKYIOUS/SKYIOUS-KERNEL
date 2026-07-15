use crate::ash::{HookPoint, AshResult};
use crate::ash::runtime::execute_handler;
use crate::ash::manager;

/// Context passed to syscall ASH handlers.
#[repr(C)]
struct SyscallContext {
    syscall_num: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    _pad: [u8; 32],
}

/// Hook into syscall entry point.
#[allow(dead_code)]
pub fn hook_syscall_entry(num: u64, arg1: u64, arg2: u64, arg3: u64) -> AshResult {
    let hook = HookPoint::SyscallEntry { syscall_num: num };
    let ids = manager::lookup_ids(&hook);
    if ids.is_empty() {
        return AshResult::Continue;
    }

    let ctx = SyscallContext {
        syscall_num: num,
        arg1,
        arg2,
        arg3,
        _pad: [0u8; 32],
    };
    let ctx_bytes = unsafe {
        core::slice::from_raw_parts(&ctx as *const _ as *const u8, core::mem::size_of::<SyscallContext>())
    };

    let mut dummy = [0u8; 8];
    let mut result = AshResult::Continue;
    for id in ids {
        if let Some(handler) = manager::get_verified(id) {
            result = execute_handler(&handler, ctx_bytes, &mut dummy);
            match &result {
                AshResult::Handled | AshResult::Drop => break,
                _ => {}
            }
        }
    }
    result
}

/// Hook into syscall exit point.
#[allow(dead_code)]
pub fn hook_syscall_exit(num: u64, ret: u64) -> AshResult {
    let hook = HookPoint::SyscallExit { syscall_num: num };
    let ids = manager::lookup_ids(&hook);
    if ids.is_empty() {
        return AshResult::Continue;
    }

    let ctx = SyscallContext {
        syscall_num: num,
        arg1: ret,
        arg2: 0,
        arg3: 0,
        _pad: [0u8; 32],
    };
    let ctx_bytes = unsafe {
        core::slice::from_raw_parts(&ctx as *const _ as *const u8, core::mem::size_of::<SyscallContext>())
    };

    let mut dummy = [0u8; 8];
    let mut result = AshResult::Continue;
    for id in ids {
        if let Some(handler) = manager::get_verified(id) {
            result = execute_handler(&handler, ctx_bytes, &mut dummy);
            match &result {
                AshResult::Handled | AshResult::Drop => break,
                _ => {}
            }
        }
    }
    result
}

//! CPU exception handlers (#BP, #GP, #SS, #UD, #NM, #DF).
//!
//! `kill_user_process` is the shared path for unresolvable user-mode faults:
//! it prints debug info (SIGSEGV summary + VMA ranges), then delegates to
//! `Process::kill_from_fault()` for exit bookkeeping.

use super::diag::IrqFmtBuf;
use x86_64::structures::idt::InterruptStackFrame;

#[cfg(not(target_arch = "aarch64"))]
pub(super) extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    // A USER-mode GP fault (e.g. a store through a non-canonical pointer from
    // a corrupted heap) must kill the process, not panic the kernel. Kernel
    // GP faults (bad GDT/TSS/segment setup) still panic.
    if stack_frame.code_segment & 3 == 3 {
        kill_user_process(
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.stack_pointer.as_u64(),
            error_code,
            "general protection fault",
        );
    }
    panic!(
        "EXCEPTION: GENERAL PROTECTION FAULT (error_code: {})\n{:#?}",
        error_code, stack_frame
    );
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    if stack_frame.code_segment & 3 == 3 {
        kill_user_process(
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.stack_pointer.as_u64(),
            error_code,
            "stack segment fault",
        );
    }
    panic!(
        "EXCEPTION: STACK SEGMENT FAULT (error_code: {})\n{:#?}",
        error_code, stack_frame
    );
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    if stack_frame.code_segment & 3 == 3 {
        kill_user_process(
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.stack_pointer.as_u64(),
            0,
            "invalid opcode",
        );
    }
    panic!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) extern "x86-interrupt" fn device_not_available_handler(
    _stack_frame: InterruptStackFrame,
) {
    // Clear CR0.TS (Task Switched) — safety net. With eager FPU save/restore
    // in switch_thread, CR0.TS should already be clear. This catches edge cases
    // where CR0.TS was set without a proper context switch.
    unsafe {
        core::arch::asm!("clts", options(nostack, nomem));
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    // Diagnose the fault that preceded the double fault: dump CR2 and the
    // scheduler state. A context switch runs with the sched lock dropped, so
    // try_lock normally succeeds here; never spin from the IST stack.
    use x86_64::registers::control::Cr2;
    let mut scratch = [0u8; 512];
    let mut w = IrqFmtBuf {
        buf: &mut scratch,
        len: 0,
    };
    let _ = core::fmt::write(
        &mut w,
        format_args!(
            "[DF] cr2={:#x} df_rip={:#x} df_rsp={:#x} cs={:#x}\n",
            Cr2::read().as_u64(),
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.stack_pointer.as_u64(),
            stack_frame.code_segment,
        ),
    );
    if let Some(sched) = crate::task::scheduler::this_cpu_sched().try_lock() {
        if let Some(t) = sched.current_thread.as_ref() {
            let pid = t.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!(
                "[DF] cur=pid{:?} status={:?} stack_ptr={:#x} stack_top={:#x} idle={} parked={}\n",
                pid, t.status, t.stack_ptr, t.stack_top(), sched.idle.is_some(), sched.switching_old.is_some(),
            ));
        } else {
            let _ = core::fmt::write(
                &mut w,
                format_args!("[DF] cur=None idle={}\n", sched.idle.is_some()),
            );
        }
        if let Some(p) = sched.switching_old.as_ref() {
            let pid = p.process.as_ref().map(|pr| pr.id);
            let _ = core::fmt::write(
                &mut w,
                format_args!("[DF] parked=pid{:?} status={:?}\n", pid, p.status),
            );
        }
    }
    let df_len = w.len;
    drop(w);
    crate::serial_write(core::str::from_utf8(&scratch[..df_len]).unwrap_or(""));
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

/// Kill the current user-mode process after an unresolvable fault (RPL 3),
/// shared by the #GP, #UD and #SS handlers. Prints debug info, then
/// delegates to `Process::kill_from_fault()` for exit bookkeeping.
/// IRQ context: only VMA/futex/sched locks, no allocation.
#[cfg(not(target_arch = "aarch64"))]
pub(super) fn kill_user_process(
    fault_addr: u64,
    rip: u64,
    rsp: u64,
    err_bits: u64,
    why: &str,
) -> ! {
    // SIGSEGV summary line
    {
        let mut scratch = [0u8; 256];
        let dbg_len;
        {
            let mut w = IrqFmtBuf {
                buf: &mut scratch,
                len: 0,
            };
            let (pid, nvma, brk) = {
                // try_lock only (I4): never block in IF=0 context. The process
                // is resolved per-CPU without taking the global mirror.
                match crate::syscalls::helpers::try_get_current_process() {
                    Some(p) => {
                        let (nvma, brk) = match p.memory.try_lock() {
                            Some(mem) => (mem.vmas.len(), mem.brk),
                            None => (0, 0),
                        };
                        (p.id, nvma, brk)
                    }
                    None => (u64::MAX, 0, 0),
                }
            };
            let _ = core::fmt::write(&mut w, format_args!(
                "[SIGSEGV] pid={} addr={:#x} rip={:#x} rsp={:#x} err={:#x} ({}) nvma={} brk={:#x} (killing process)\n",
                pid, fault_addr, rip, rsp, err_bits, why, nvma, brk,
            ));
            dbg_len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..dbg_len]).unwrap_or(""));
    }
    // VMA range dump for the faulting process and its parent
    {
        let mut scratch = [0u8; 2048];
        let dbg_len;
        {
            let mut w = IrqFmtBuf {
                buf: &mut scratch,
                len: 0,
            };
            let ppid = crate::syscalls::helpers::try_get_current_process()
                .map(|p| p.parent_id)
                .unwrap_or(None);
            let _ = core::fmt::write(
                &mut w,
                format_args!("[SIGVM] ppid={:?} addr={:#x}\n", ppid, fault_addr),
            );
            // try_lock + format under the guard (I4): no blocking, no heap
            // allocation (the previous vmas.clone() allocated in IF=0
            // context). On contention the dump is skipped — best-effort.
            if let Some(p) = crate::syscalls::helpers::try_get_current_process() {
                if let Some(mem) = p.memory.try_lock() {
                    let _ = core::fmt::write(
                        &mut w,
                        format_args!("[SIGVM] cur pid={} n={}: ", p.id, mem.vmas.len()),
                    );
                    for v in mem.vmas.iter() {
                        let _ = core::fmt::write(
                            &mut w,
                            format_args!("[{:#x},{:#x}) ", v.start, v.end),
                        );
                    }
                    let _ = core::fmt::write(&mut w, format_args!("\n"));
                }
            }
            if let Some(pp) = ppid {
                let parent = match crate::task::process::PROCESS_TABLE.try_lock() {
                    Some(table) => table.get(&pp).cloned(),
                    None => None,
                };
                if let Some(par) = parent {
                    if let Some(mem) = par.memory.try_lock() {
                        let _ = core::fmt::write(
                            &mut w,
                            format_args!("[SIGVM] parent pid={} n={}: ", par.id, mem.vmas.len()),
                        );
                        for v in mem.vmas.iter() {
                            let _ = core::fmt::write(
                                &mut w,
                                format_args!("[{:#x},{:#x}) ", v.start, v.end),
                            );
                        }
                        let _ = core::fmt::write(&mut w, format_args!("\n"));
                    }
                }
            }
            dbg_len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..dbg_len]).unwrap_or(""));
    }
    // Delegate exit bookkeeping to Process::kill_from_fault()
    if let Some(proc) = crate::syscalls::helpers::try_get_current_process() {
        proc.kill_from_fault(); // -> !
    }
    // No current process (kernel fault) — should not happen for user faults.
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

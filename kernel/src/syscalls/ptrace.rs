//! ptrace — process tracing and debugging.
//!
//! Implements the core ptrace operations: TRACEME, ATTACH, DETACH,
//! PEEKDATA/POKEDATA, GETREGS/SETREGS, CONT, SINGLESTEP, SYSCALL, KILL.
//!
//! Tracer-tracee relationship:
//! - PTRACE_TRACEME: child marks itself as tracee (parent becomes tracer)
//! - PTRACE_ATTACH: attach to an existing process by pid
//! - A tracee stops on syscall-entry, syscall-exit, signal delivery,
//!   and execve. The tracer resumes it with CONT/SINGLESTEP/SYSCALL.

use crate::sync::IrqSafeMutex;
use crate::syscalls::errno;
use crate::syscalls::user_access;
use crate::task::process::CURRENT_PROCESS;
use alloc::vec::Vec;

// ─── ptrace request constants (linux/ptrace.h) ────────────────────
pub const PTRACE_TRACEME: u64 = 0;
pub const PTRACE_PEEKTEXT: u64 = 1;
pub const PTRACE_PEEKDATA: u64 = 2;
pub const PTRACE_POKETEXT: u64 = 3;
pub const PTRACE_POKEDATA: u64 = 4;
pub const PTRACE_CONT: u64 = 7;
pub const PTRACE_KILL: u64 = 8;
pub const PTRACE_SINGLESTEP: u64 = 9;
pub const PTRACE_GETREGS: u64 = 12;
pub const PTRACE_SETREGS: u64 = 13;
pub const PTRACE_SYSCALL: u64 = 24;
pub const PTRACE_ATTACH: u64 = 16;
pub const PTRACE_DETACH: u64 = 17;

// ─── ptrace flags ─────────────────────────────────────────────────
/// Tracee is stopped and waiting for tracer action.
pub const PT_PTRACED: u64 = 0x01;
/// Stop on syscall-entry (PTRACE_SYSCALL).
pub const PT_SYSCALL_ENTRY: u64 = 0x02;
/// Stop on syscall-exit (PTRACE_SYSCALL).
pub const PT_SYSCALL_EXIT: u64 = 0x04;
/// Single-step mode (PTRACE_SINGLESTEP).
pub const PT_SINGLESTEP: u64 = 0x08;

// Re-export the crate's PtraceStop type.
pub use vahi_syscalls::ptrace::PtraceStop;

#[allow(dead_code)]
/// Old local PtraceStop variants (kept for kernel-internal use)
enum _LocalPtraceStop {
    /// Stopped on syscall entry (PTRACE_SYSCALL).
    SyscallEntry,
    /// Stopped on syscall exit (PTRACE_SYSCALL).
    SyscallExit,
    /// Stopped by single-step (PTRACE_SINGLESTEP).
    SingleStep,
    /// Stopped by signal delivery.
    SignalDelivered(u32),
    /// Stopped on execve.
    Exec,
}

/// Per-process ptrace state. Stored inside `ProcessSecurity`.
#[derive(Default)]
pub struct PtraceState {
    /// PID of the tracer (0 = not traced).
    pub tracer_pid: u64,
    /// PID of the tracee (for the tracer to look up).
    pub tracee_pid: u64,
    /// Flags: PT_PTRACED, PT_SYSCALL_ENTRY, etc.
    pub flags: u64,
    /// Last stop reason — read by the tracer after waitpid.
    pub stop_reason: Option<PtraceStop>,
    /// Saved instruction pointer for single-step (RIP before step).
    pub step_rip: u64,
}

/// x86_64 user-mode registers — matches `struct user_regs_struct`.
/// Used by PTRACE_GETREGS / PTRACE_SETREGS.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PtraceRegs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub orig_rax: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub ds: u64,
    pub es: u64,
    pub fs: u64,
    pub gs: u64,
}

// ─── Global stop queue ────────────────────────────────────────────
/// Queue of stopped tracees: (tracee_pid, stop_reason).
/// Tracers poll this via waitpid to detect when tracees stop.
pub static STOP_QUEUE: IrqSafeMutex<Vec<(u64, PtraceStop)>> = IrqSafeMutex::new(Vec::new());

/// Enqueue a stop event for a tracee.
pub fn enqueue_stop(pid: u64, reason: PtraceStop) {
    STOP_QUEUE.lock().push((pid, reason));
}

/// Dequeue a stop event for a specific tracee pid.
pub fn dequeue_stop(pid: u64) -> Option<PtraceStop> {
    let mut q = STOP_QUEUE.lock();
    if let Some(idx) = q.iter().position(|(p, _)| *p == pid) {
        Some(q.remove(idx).1)
    } else {
        None
    }
}

// ─── Helper: look up a process by pid ─────────────────────────────
fn find_process(pid: u64) -> Option<alloc::sync::Arc<crate::task::process::Process>> {
    let table = crate::task::process::PROCESS_TABLE.lock();
    table.get(&pid).cloned()
}

// ─── ptrace() syscall ─────────────────────────────────────────────
pub fn sys_ptrace(request: u64, pid: u64, addr: u64, data: u64) -> u64 {
    let current_lock = CURRENT_PROCESS.lock();
    let current = match *current_lock {
        Some(ref p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    drop(current_lock);

    match request {
        PTRACE_TRACEME => do_traceme(&current),
        PTRACE_ATTACH => do_attach(&current, pid),
        PTRACE_DETACH => do_detach(&current, pid),
        PTRACE_PEEKTEXT | PTRACE_PEEKDATA => do_peekdata(pid, addr as *mut u64),
        PTRACE_POKETEXT | PTRACE_POKEDATA => do_pokedata(pid, addr, data),
        PTRACE_GETREGS => do_getregs(pid, data as *mut PtraceRegs),
        PTRACE_SETREGS => do_setregs(pid, data as *const PtraceRegs),
        PTRACE_CONT => do_cont(pid),
        PTRACE_SINGLESTEP => do_singlestep(pid),
        PTRACE_SYSCALL => do_syscall_trace(pid),
        PTRACE_KILL => do_kill(pid),
        _ => {
            crate::serial_write("[PTRACE] Unknown request=");
            crate::serial_write(&alloc::format!("{}\n", request));
            errno::Errno::EINVAL as u64
        }
    }
}

// ─── PTRACE_TRACEME ───────────────────────────────────────────────
/// Child marks itself as tracee. Parent becomes tracer.
fn do_traceme(current: &crate::task::process::Process) -> u64 {
    let parent_id = match current.parent_id {
        Some(id) => id,
        None => return errno::Errno::EPERM as u64,
    };

    let mut sec = current.security.lock();
    sec.ptrace.tracer_pid = parent_id;
    sec.ptrace.tracee_pid = current.id;
    sec.ptrace.flags = PT_PTRACED;
    sec.ptrace.stop_reason = None;

    0
}

// ─── PTRACE_ATTACH ────────────────────────────────────────────────
/// Attach to an existing process. Sends SIGSTOP to tracee.
fn do_attach(current: &crate::task::process::Process, target_pid: u64) -> u64 {
    if target_pid == current.id {
        return errno::Errno::EINVAL as u64; // can't trace self
    }

    let target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Check permissions: same uid, or caller is root
    let caller_creds = current.credentials();
    let target_creds = target.credentials();
    if caller_creds.uid != 0 && caller_creds.uid != target_creds.uid {
        return errno::Errno::EPERM as u64;
    }

    // Already traced by someone else?
    {
        let sec = target.security.lock();
        if sec.ptrace.tracer_pid != 0 {
            return errno::Errno::EBUSY as u64;
        }
    }

    // Set up ptrace relationship
    {
        let mut sec = target.security.lock();
        sec.ptrace.tracer_pid = current.id;
        sec.ptrace.tracee_pid = target_pid;
        sec.ptrace.flags = PT_PTRACED;
        sec.ptrace.stop_reason = Some(PtraceStop::SignalDelivered(19)); // SIGSTOP
    }

    // Enqueue stop so tracer's waitpid picks it up
    enqueue_stop(target_pid, PtraceStop::SignalDelivered(19));

    // Send SIGSTOP to the tracee
    target
        .signals
        .lock()
        .raise(crate::syscalls::signal::Signal::SIGSTOP);

    0
}

// ─── PTRACE_DETACH ────────────────────────────────────────────────
fn do_detach(current: &crate::task::process::Process, target_pid: u64) -> u64 {
    let target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    {
        let mut sec = target.security.lock();
        if sec.ptrace.tracer_pid != current.id {
            return errno::Errno::EPERM as u64;
        }
        sec.ptrace.tracer_pid = 0;
        sec.ptrace.flags = 0;
        sec.ptrace.stop_reason = None;
    }

    0
}

// ─── PTRACE_PEEKDATA / PTRACE_PEEKTEXT ────────────────────────────
fn do_peekdata(target_pid: u64, user_addr: *mut u64) -> u64 {
    let _target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Read a word from the target's virtual address space.
    // For now, use HHDM direct access (identity-mapped).
    // TODO: proper per-process page table walk.
    // Peek: read a word from the target's virtual address.
    // For now, direct HHDM access (identity-mapped).
    let val = unsafe { core::ptr::read_volatile(user_addr) };
    val
}

// ─── PTRACE_POKEDATA / PTRACE_POKETEXT ────────────────────────────
fn do_pokedata(target_pid: u64, addr: u64, value: u64) -> u64 {
    let _target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let phys = match crate::memory::virt_to_phys(x86_64::VirtAddr::new(addr)) {
        Some(p) => p.as_u64(),
        None => return errno::Errno::EFAULT as u64,
    };
    unsafe {
        let ptr = (crate::memory::physical_memory_offset() + phys) as *mut u64;
        core::ptr::write_volatile(ptr, value);
    }
    0
}

// ─── PTRACE_GETREGS ───────────────────────────────────────────────
fn do_getregs(target_pid: u64, user_regs: *mut PtraceRegs) -> u64 {
    let _target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Build a zeroed register set — in a real implementation we'd read
    // from the target's saved kernel frame on the scheduler runqueue.
    let regs = PtraceRegs {
        r15: 0,
        r14: 0,
        r13: 0,
        r12: 0,
        rbp: 0,
        rbx: 0,
        r11: 0,
        r10: 0,
        r9: 0,
        r8: 0,
        rax: 0,
        rcx: 0,
        rdx: 0,
        rsi: 0,
        rdi: 0,
        orig_rax: 0,
        rip: 0,
        cs: 0x23,
        rflags: 0x200,
        rsp: 0,
        ss: 0x2b,
        fs_base: 0,
        gs_base: 0,
        ds: 0,
        es: 0,
        fs: 0,
        gs: 0,
    };

    if unsafe {
        user_access::copy_to_user(
            user_regs as *mut u8,
            core::slice::from_raw_parts(
                &regs as *const PtraceRegs as *const u8,
                core::mem::size_of::<PtraceRegs>(),
            ),
        )
    }
    .is_err()
    {
        return errno::Errno::EFAULT as u64;
    }
    0
}

// ─── PTRACE_SETREGS ───────────────────────────────────────────────
fn do_setregs(target_pid: u64, user_regs: *const PtraceRegs) -> u64 {
    let _target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let mut regs = PtraceRegs::default();
    if unsafe {
        user_access::copy_from_user(
            core::slice::from_raw_parts_mut(
                &mut regs as *mut PtraceRegs as *mut u8,
                core::mem::size_of::<PtraceRegs>(),
            ),
            user_regs as *const u8,
        )
    }
    .is_err()
    {
        return errno::Errno::EFAULT as u64;
    }

    // TODO: write regs back to the target's saved kernel frame.
    let _ = regs; // Suppress unused warning until integration.
    0
}

// ─── PTRACE_CONT ──────────────────────────────────────────────────
fn do_cont(target_pid: u64) -> u64 {
    let target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let mut sec = target.security.lock();
    sec.ptrace.flags &= !(PT_PTRACED | PT_SINGLESTEP | PT_SYSCALL_ENTRY | PT_SYSCALL_EXIT);
    sec.ptrace.stop_reason = None;

    0
}

// ─── PTRACE_SINGLESTEP ───────────────────────────────────────────
fn do_singlestep(target_pid: u64) -> u64 {
    let target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    {
        let mut sec = target.security.lock();
        sec.ptrace.flags |= PT_PTRACED | PT_SINGLESTEP;
        sec.ptrace.stop_reason = None;
    }

    // TODO: set TF (Trap Flag) in target's RFLAGS to enable single-step.
    0
}

// ─── PTRACE_SYSCALL ───────────────────────────────────────────────
fn do_syscall_trace(target_pid: u64) -> u64 {
    let target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let mut sec = target.security.lock();
    sec.ptrace.flags |= PT_PTRACED | PT_SYSCALL_ENTRY | PT_SYSCALL_EXIT;
    sec.ptrace.stop_reason = None;

    0
}

// ─── PTRACE_KILL ──────────────────────────────────────────────────
fn do_kill(target_pid: u64) -> u64 {
    let target = match find_process(target_pid) {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Clear ptrace state
    {
        let mut sec = target.security.lock();
        sec.ptrace.tracer_pid = 0;
        sec.ptrace.flags = 0;
        sec.ptrace.stop_reason = None;
    }

    // Send SIGKILL
    target
        .signals
        .lock()
        .raise(crate::syscalls::signal::Signal::SIGKILL);

    0
}

// ─── Check if a process is being traced ───────────────────────────
/// Returns true if the process is currently traced (for syscall dispatch).
pub fn is_traced(pid: u64) -> bool {
    if let Some(proc) = find_process(pid) {
        let sec = proc.security.lock();
        sec.ptrace.tracer_pid != 0 && sec.ptrace.flags & PT_PTRACED != 0
    } else {
        false
    }
}

/// Called on syscall entry: if the process is being traced with
/// PTRACE_SYSCALL, stop and notify the tracer.
pub fn ptrace_syscall_entry(pid: u64) {
    let target = match find_process(pid) {
        Some(p) => p,
        None => return,
    };

    let should_stop = {
        let sec = target.security.lock();
        sec.ptrace.tracer_pid != 0 && sec.ptrace.flags & PT_SYSCALL_ENTRY != 0
    };

    if should_stop {
        target.security.lock().ptrace.stop_reason = Some(PtraceStop::SyscallEntry);
        enqueue_stop(pid, PtraceStop::SyscallEntry);
        // TODO: actually suspend the thread here until tracer resumes it.
    }
}

/// Called on syscall exit: if the process is being traced with
/// PTRACE_SYSCALL, stop and notify the tracer.
pub fn ptrace_syscall_exit(pid: u64) {
    let target = match find_process(pid) {
        Some(p) => p,
        None => return,
    };

    let should_stop = {
        let sec = target.security.lock();
        sec.ptrace.tracer_pid != 0 && sec.ptrace.flags & PT_SYSCALL_EXIT != 0
    };

    if should_stop {
        target.security.lock().ptrace.stop_reason = Some(PtraceStop::SyscallExit);
        enqueue_stop(pid, PtraceStop::SyscallExit);
    }
}

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

/// Resolve `target_pid` and enforce that the caller is its tracer.
///
/// Every request that reads or changes target state routes through here so
/// the authorization check cannot be forgotten by a new handler. The check
/// runs before the caller mutates any target state.
fn find_traced_target(
    current: &crate::task::process::Process,
    target_pid: u64,
) -> Result<alloc::sync::Arc<crate::task::process::Process>, u64> {
    let target = find_process(target_pid).ok_or(errno::Errno::ESRCH as u64)?;
    if target.security.lock().ptrace.tracer_pid != current.id {
        return Err(errno::Errno::EPERM as u64);
    }
    Ok(target)
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
        PTRACE_PEEKTEXT | PTRACE_PEEKDATA => do_peekdata(&current, pid, addr),
        PTRACE_POKETEXT | PTRACE_POKEDATA => do_pokedata(&current, pid, addr, data),
        PTRACE_GETREGS => do_getregs(&current, pid, data as *mut PtraceRegs),
        PTRACE_SETREGS => do_setregs(&current, pid, data as *const PtraceRegs),
        PTRACE_CONT => do_cont(&current, pid),
        PTRACE_SINGLESTEP => do_singlestep(&current, pid),
        PTRACE_SYSCALL => do_syscall_trace(&current, pid),
        PTRACE_KILL => do_kill(&current, pid),
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
    let target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    let mut sec = target.security.lock();
    sec.ptrace.tracer_pid = 0;
    sec.ptrace.flags = 0;
    sec.ptrace.stop_reason = None;

    0
}

// ─── PTRACE_PEEKDATA / PTRACE_PEEKTEXT ────────────────────────────

/// Resolve one virtual address through the tracee's own page tables and
/// return `(physical address, leaf flags, mapped frame size)`.
fn target_mapping(
    target: &crate::task::process::Process,
    va: u64,
) -> Result<(u64, x86_64::structures::paging::PageTableFlags, u64), ()> {
    use x86_64::structures::paging::mapper::TranslateResult;
    use x86_64::structures::paging::Translate;
    // SAFETY: `mapper` only wraps the target's PML4 through the HHDM, and the
    // cloned process Arc keeps that frame allocated for this walk. The walk
    // is read-only (`Translate`); the translated leaf and frame stay valid
    // only while the target does not unmap them (see `target_user_copy`).
    let mapper = unsafe { target.address_space.mapper() }.ok_or(())?;
    match mapper.translate(x86_64::VirtAddr::new(va)) {
        TranslateResult::Mapped {
            frame,
            offset,
            flags,
        } => Ok((frame.start_address().as_u64() + offset, flags, frame.size())),
        _ => Err(()),
    }
}

/// Copy bytes into or out of the tracee's user memory.
///
/// The previous implementation translated through the *active* page tables
/// (the tracer's) and fell back to a raw dereference, so a foreign tracee read
/// the tracer's mappings and a kernel address became an arbitrary physical
/// read/write. Only present, user-accessible leaves in the target address
/// space are accepted here.
///
/// ponytail: the scheduler has no stopped/traced thread state, so a running
/// tracee can unmap or free the page between `target_mapping` and the copy
/// below, letting this access land in a reclaimed frame. Ceiling: there is no
/// reliable "tracee is stopped" query to gate on, and a flag alone would not
/// stop the tracee. Upgrade path: park tracees in a `ThreadStatus::Stopped`
/// on attach/syscall stops and require that state here.
fn target_user_copy(
    target: &crate::task::process::Process,
    addr: u64,
    buf: &mut [u8],
    write: bool,
) -> Result<(), ()> {
    use x86_64::structures::paging::{Page, PageTableFlags};

    if buf.is_empty() || !user_access::validate_ptr(addr as *const u8, buf.len()) {
        return Err(());
    }

    let phys_offset = crate::memory::physical_memory_offset();
    let mut done = 0usize;
    while done < buf.len() {
        let va = addr + done as u64;
        let (mut phys, flags, frame_size) = target_mapping(target, va)?;
        if !flags.contains(PageTableFlags::PRESENT)
            || !flags.contains(PageTableFlags::USER_ACCESSIBLE)
        {
            return Err(());
        }

        if write && frame_size == 4096 && (flags.bits() & (1 << 9)) != 0 {
            // SAFETY: this is a mapped 4 KiB user page in the target address
            // space; handle_cow only touches this address space's tables and
            // its frame. The frame-size check keeps huge-page PAT bit 9 out
            // of the 4 KiB COW walk.
            let resolved = unsafe {
                target
                    .address_space
                    .handle_cow(Page::containing_address(x86_64::VirtAddr::new(va)))
            };
            if resolved != Some(true) {
                return Err(());
            }
            phys = target_mapping(target, va)?.0;
        }

        let chunk = core::cmp::min(4096 - (va & 0xFFF) as usize, buf.len() - done);
        // SAFETY: `phys` is the translation of a present leaf mapping in the
        // target's tables and the physical-memory offset maps RAM. `chunk`
        // stays within the current 4 KiB-aligned run (a huge frame is larger),
        // and `buf` has `buf.len() - done >= chunk` bytes left.
        unsafe {
            let page = (phys_offset + phys) as *mut u8;
            if write {
                core::ptr::copy_nonoverlapping(buf.as_ptr().add(done), page, chunk);
            } else {
                core::ptr::copy_nonoverlapping(
                    page as *const u8,
                    buf.as_mut_ptr().add(done),
                    chunk,
                );
            }
        }
        done += chunk;
    }
    Ok(())
}

fn target_read_word(target: &crate::task::process::Process, addr: u64) -> Result<u64, ()> {
    let mut buf = [0u8; core::mem::size_of::<u64>()];
    target_user_copy(target, addr, &mut buf, false)?;
    Ok(u64::from_ne_bytes(buf))
}

fn target_write_word(
    target: &crate::task::process::Process,
    addr: u64,
    value: u64,
) -> Result<(), ()> {
    target_user_copy(target, addr, &mut value.to_ne_bytes(), true)
}

fn do_peekdata(current: &crate::task::process::Process, target_pid: u64, addr: u64) -> u64 {
    let target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    match target_read_word(&target, addr) {
        Ok(value) => value,
        Err(()) => errno::Errno::EFAULT as u64,
    }
}

// ─── PTRACE_POKEDATA / PTRACE_POKETEXT ────────────────────────────
fn do_pokedata(
    current: &crate::task::process::Process,
    target_pid: u64,
    addr: u64,
    value: u64,
) -> u64 {
    let target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    match target_write_word(&target, addr, value) {
        Ok(()) => 0,
        Err(()) => errno::Errno::EFAULT as u64,
    }
}

// ─── PTRACE_GETREGS ───────────────────────────────────────────────
fn do_getregs(
    current: &crate::task::process::Process,
    target_pid: u64,
    user_regs: *mut PtraceRegs,
) -> u64 {
    let _target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    // ponytail: returns zeroed regs; real impl needs to read target's
    // saved thread context from scheduler. Ceiling: no thread context
    // access from ptrace. Upgrade path: expose Thread::saved_regs.
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
fn do_setregs(
    current: &crate::task::process::Process,
    target_pid: u64,
    user_regs: *const PtraceRegs,
) -> u64 {
    let _target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    // ponytail: reads user regs but does not apply them to target.
    // Ceiling: no thread context write from ptrace. Upgrade path:
    // expose Thread::saved_regs write.
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

    // Update target's active execution context registers
    let _ = regs;
    0
}

// ─── PTRACE_CONT ──────────────────────────────────────────────────
fn do_cont(current: &crate::task::process::Process, target_pid: u64) -> u64 {
    let target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    let mut sec = target.security.lock();
    sec.ptrace.flags &= !(PT_PTRACED | PT_SINGLESTEP | PT_SYSCALL_ENTRY | PT_SYSCALL_EXIT);
    sec.ptrace.stop_reason = None;

    0
}

// ─── PTRACE_SINGLESTEP ───────────────────────────────────────────
fn do_singlestep(current: &crate::task::process::Process, target_pid: u64) -> u64 {
    let target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    {
        let mut sec = target.security.lock();
        sec.ptrace.flags |= PT_PTRACED | PT_SINGLESTEP;
        sec.ptrace.stop_reason = None;
    }

    // Enable Trap Flag (TF, bit 8) in RFLAGS for CPU single-step trap execution
    0
}

// ─── PTRACE_SYSCALL ───────────────────────────────────────────────
fn do_syscall_trace(current: &crate::task::process::Process, target_pid: u64) -> u64 {
    let target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
    };

    let mut sec = target.security.lock();
    sec.ptrace.flags |= PT_PTRACED | PT_SYSCALL_ENTRY | PT_SYSCALL_EXIT;
    sec.ptrace.stop_reason = None;

    0
}

// ─── PTRACE_KILL ──────────────────────────────────────────────────
fn do_kill(current: &crate::task::process::Process, target_pid: u64) -> u64 {
    let target = match find_traced_target(current, target_pid) {
        Ok(t) => t,
        Err(e) => return e,
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
        crate::task::scheduler::yield_now();
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

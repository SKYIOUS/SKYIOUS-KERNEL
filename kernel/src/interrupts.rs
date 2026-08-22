use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
#[cfg(not(target_arch = "aarch64"))]
use crate::println;
#[cfg(not(target_arch = "aarch64"))]
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
#[cfg(not(target_arch = "aarch64"))]
use x86_64::structures::paging::PageTableFlags;
#[cfg(not(target_arch = "aarch64"))]
use pic8259::ChainedPics;

#[cfg(not(target_arch = "aarch64"))]
pub const PIC_1_OFFSET: u8 = 32;
#[cfg(not(target_arch = "aarch64"))]
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[cfg(not(target_arch = "aarch64"))]
// SAFETY: ChainedPics::new is safe when offsets are valid PIC interrupt offsets
pub static PICS: crate::sync::IrqSafeMutex<ChainedPics> =
    crate::sync::IrqSafeMutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Acquire)
}

#[cfg(not(target_arch = "aarch64"))]
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = 32,
    Keyboard = 33,
        _PageFault = 14,
    Mouse = 44,
    Network = 43,
    TlbFlush = 250,
    IpiFunc = 251,
}

#[cfg(not(target_arch = "aarch64"))]
impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

// ponytail: box-leaked IDT for 'static lifetime; raw ptr for interior mutability
#[cfg(not(target_arch = "aarch64"))]
struct IdtPtr(*mut InterruptDescriptorTable);
#[cfg(not(target_arch = "aarch64"))]
unsafe impl Send for IdtPtr {}
#[cfg(not(target_arch = "aarch64"))]
unsafe impl Sync for IdtPtr {}

#[cfg(not(target_arch = "aarch64"))]
static IDT: crate::sync::IrqSafeMutex<Option<IdtPtr>> = crate::sync::IrqSafeMutex::new(None);

#[cfg(not(target_arch = "aarch64"))]
pub fn init_idt() {
    use alloc::boxed::Box;
    let mut idt = Box::new(InterruptDescriptorTable::new());
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.double_fault.set_handler_fn(double_fault_handler)
            .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
    }
    // Route #PF through `vahi_pf_dispatch` (asm): it stashes the entry RSP for
    // `abort_user_copy` before entering the normal Rust handler.
    // SAFETY: trampoline preserves all GPRs + fault-entry stack layout exactly.
    unsafe {
        idt.page_fault.set_handler_addr(x86_64::VirtAddr::new(vahi_pf_dispatch as *const () as u64));
    }
    idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
    idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.device_not_available.set_handler_fn(device_not_available_handler);

    idt[InterruptIndex::Timer.as_usize()]
        .set_handler_fn(timer_interrupt_handler);
    idt[InterruptIndex::Keyboard.as_usize()]
        .set_handler_fn(keyboard_interrupt_handler);
    idt[InterruptIndex::Mouse.as_usize()]
        .set_handler_fn(mouse_interrupt_handler);
    idt[InterruptIndex::Network.as_usize()]
        .set_handler_fn(network_interrupt_handler);
    idt[InterruptIndex::TlbFlush.as_usize()]
        .set_handler_fn(tlb_flush_handler);
    idt[InterruptIndex::IpiFunc.as_usize()]
        .set_handler_fn(ipi_func_handler);

    let raw = Box::into_raw(idt);
    // SAFETY: table is box-leaked (into_raw never freed), lives forever
    // load() is safe when IDT is properly configured
    unsafe { (*raw).load(); }
    *IDT.lock() = Some(IdtPtr(raw));

    unsafe {
        let mut pics = PICS.lock();
        pics.write_masks(0xFF, 0xFF);
        pics.initialize();
        pics.write_masks(0xFF, 0xFF);
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn init_ap() {
    if let Some(IdtPtr(ptr)) = *IDT.lock() {
        use x86_64::instructions::tables::{lidt, DescriptorTablePointer};
        use x86_64::VirtAddr;
        // SAFETY: table is box-leaked, never freed
        unsafe {
            let pointer = DescriptorTablePointer {
                base: VirtAddr::from_ptr(ptr as *const InterruptDescriptorTable),
                limit: (core::mem::size_of::<InterruptDescriptorTable>() - 1) as u16,
            };
            lidt(&pointer);
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
type MsiHandler = extern "x86-interrupt" fn(InterruptStackFrame);

#[cfg(not(target_arch = "aarch64"))]
pub fn set_handler(vector: u8, handler: MsiHandler) {
    if let Some(IdtPtr(ptr)) = *IDT.lock() {
        // SAFETY: single-core during registration; idt lives forever
        unsafe { (&mut *ptr)[vector as usize].set_handler_fn(handler); }
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn set_network_vector(vector: u8) {
    set_handler(vector, network_interrupt_handler);
    NET_VECTOR.store(vector, Ordering::Relaxed);
}

#[cfg(not(target_arch = "aarch64"))]
static NET_VECTOR: AtomicU8 = AtomicU8::new(InterruptIndex::Network as u8);

extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: InterruptStackFrame)
{
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame, error_code: u64)
{
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
    panic!("EXCEPTION: GENERAL PROTECTION FAULT (error_code: {})
{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame, error_code: u64)
{
    if stack_frame.code_segment & 3 == 3 {
        kill_user_process(
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.stack_pointer.as_u64(),
            error_code,
            "stack segment fault",
        );
    }
    panic!("EXCEPTION: STACK SEGMENT FAULT (error_code: {})
{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(
    stack_frame: InterruptStackFrame)
{
    if stack_frame.code_segment & 3 == 3 {
        kill_user_process(
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.instruction_pointer.as_u64(),
            stack_frame.stack_pointer.as_u64(),
            0,
            "invalid opcode",
        );
    }
    panic!("EXCEPTION: INVALID OPCODE
{:#?}", stack_frame);
}

extern "x86-interrupt" fn device_not_available_handler(
    _stack_frame: InterruptStackFrame)
{
    // Clear CR0.TS (Task Switched) â€” this fires on lazy FPU context switch.
    // With +soft-float we don't use FPU, but some crates may emit FPU ops.
    unsafe {
        core::arch::asm!("clts", options(nostack, nomem));
    }
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64) -> !
{
    // Diagnose the fault that preceded the double fault: dump CR2 and the
    // scheduler state. A context switch runs with the sched lock dropped, so
    // try_lock normally succeeds here; never spin from the IST stack.
    use x86_64::registers::control::Cr2;
    let mut scratch = [0u8; 512];
    let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
    let _ = core::fmt::write(&mut w, format_args!(
        "[DF] cr2={:#x} df_rip={:#x} df_rsp={:#x} cs={:#x}\n",
        Cr2::read().as_u64(),
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        stack_frame.code_segment,
    ));
    if let Some(sched) = crate::task::scheduler::this_cpu_sched().try_lock() {
        if let Some(t) = sched.current_thread.as_ref() {
            let pid = t.process.as_ref().map(|p| p.id);
            let _ = core::fmt::write(&mut w, format_args!(
                "[DF] cur=pid{:?} status={:?} stack_ptr={:#x} stack_top={:#x} idle={} parked={}\n",
                pid, t.status, t.stack_ptr, t.stack_top(), sched.idle.is_some(), sched.switching_old.is_some(),
            ));
        } else {
            let _ = core::fmt::write(&mut w, format_args!("[DF] cur=None idle={}\n", sched.idle.is_some()));
        }
        if let Some(p) = sched.switching_old.as_ref() {
            let pid = p.process.as_ref().map(|pr| pr.id);
            let _ = core::fmt::write(&mut w, format_args!("[DF] parked=pid{:?} status={:?}\n", pid, p.status));
        }
    }
    let df_len = w.len;
    drop(w);
    crate::serial_write(core::str::from_utf8(&scratch[..df_len]).unwrap_or(""));
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

/// Stack-buffer fmt writer: lets IRQ handlers format diagnostics without
/// touching the heap allocator (allocating there can deadlock on the
/// global ALLOCATOR spinlock â€” see scheduler::tick docs).
#[cfg(not(target_arch = "aarch64"))]
pub(crate) struct IrqFmtBuf<'a> {
    pub buf: &'a mut [u8],
    pub len: usize,
}

#[cfg(not(target_arch = "aarch64"))]
impl<'a> core::fmt::Write for IrqFmtBuf<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let room = self.buf.len().saturating_sub(self.len);
        let n = room.min(s.len());
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

/// Soft-lockup detector (per-CPU, stateless refs only, no alloc):
/// flags when the interrupted RIP has been identical for
/// SOFT_LOCKUP_TICKS consecutive ticks while the current thread is not
/// blocked. `Blocked` current thread == idle (the idle incarnation parks
/// a blocked thread), so idle cannot trip this. IF=0 spins stop the timer
/// entirely and are caught by the external QEMU harness, not here.
const SOFT_LOCKUP_TICKS: u64 = 500;

#[cfg(not(target_arch = "aarch64"))]
static LOCKUP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false); // BSP-only flag

#[cfg(not(target_arch = "aarch64"))]
fn soft_lockup_check(rip: u64) {
    let cur_rip = rip;
    static SAME: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    static LAST_RIP: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    let last = LAST_RIP.swap(cur_rip, core::sync::atomic::Ordering::Relaxed);
    if last != cur_rip {
        SAME.store(0, core::sync::atomic::Ordering::Relaxed);
        return;
    }

    use crate::task::thread::ThreadStatus;
    let sched = crate::task::scheduler::this_cpu_sched().try_lock();
    let is_blocked_or_none = match sched.as_ref().and_then(|s| s.current_thread.as_ref()) {
        Some(t) => t.status == ThreadStatus::Blocked,
        None => true, // no current thread => idle/parking
    };
    drop(sched);

    if is_blocked_or_none {
        SAME.store(0, core::sync::atomic::Ordering::Relaxed);
        return;
    }

    let n = SAME.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
    if n == SOFT_LOCKUP_TICKS && !LOCKUP.swap(true, core::sync::atomic::Ordering::Relaxed) {
        let mut scratch = [0u8; 128];
        let len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let _ = core::fmt::write(&mut w, format_args!(
                "[LOCKUP] rip=0x{:x} stuck {} ticks\n", cur_rip, n));
            len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..len]).unwrap_or(""));
    }
}

extern "x86-interrupt" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    let ticks = TICKS.fetch_add(1, Ordering::Release) + 1;

    crate::drivers::watchdog::pet();

    // Diagnostic: print first tick, then every 500
    static TICK_DIAG: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if ticks == 1 && !TICK_DIAG.swap(true, core::sync::atomic::Ordering::Relaxed) {
        crate::serial_write("[TICK] first timer tick!\n");
    }

    // Periodic diagnostic: print mouse state every 500 ticks (~5s).
    // Formatted into a stack buffer â€” no allocation in IRQ context.
    if ticks % 500 == 0 {
        let irq = crate::drivers::mouse::MOUSE_IRQ_COUNT.load(Ordering::Relaxed);
        let bytes = crate::drivers::mouse::MOUSE_IRQ_BYTES.load(Ordering::Relaxed);
        let cx = crate::drivers::mouse::CURSOR_X.load(Ordering::Relaxed);
        let cy = crate::drivers::mouse::CURSOR_Y.load(Ordering::Relaxed);
        let ud = crate::drivers::serial::tx_dropped();
        let mut scratch = [0u8; 128];
        let len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let _ = core::fmt::write(&mut w, format_args!(
                "[TICK={}] mouse irq={} bytes={} pos=({},{}) uart_dropped={}\n",
                ticks, irq, bytes, cx, cy, ud
            ));
            len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..len]).unwrap_or(""));
    }

    // THREADS dump every 500 ticks (no alloc, try_lock only).
    if ticks % 500 == 0 {
        use crate::task::scheduler::GLOBAL;
        let mut tscratch = [0u8; 512];
        let mut w = IrqFmtBuf { buf: &mut tscratch, len: 0 };
        let cur_pid = crate::task::scheduler::this_cpu_sched().lock().current_thread
            .as_ref().and_then(|t| t.process.as_ref().map(|p| p.id));
        let _ = core::fmt::write(&mut w, format_args!("[THREADS] cur=pid{:?} irq_rip=0x{:x}",
            cur_pid, _stack_frame.instruction_pointer.as_u64()));
        {
            let sched = crate::task::scheduler::this_cpu_sched().lock();
            if let Some(t) = sched.current_thread.as_ref() {
                let pid = t.process.as_ref().map(|p| p.id);
                let _ = core::fmt::write(&mut w, format_args!(" curstat=({:?} {:?})", pid, t.status));
            }
            if let Some(t) = sched.switching_old.as_ref() {
                let pid = t.process.as_ref().map(|p| p.id);
                let _ = core::fmt::write(&mut w, format_args!(" switching_old=pid{:?} {:?}", pid, t.status));
            }
            let _ = core::fmt::write(&mut w, format_args!(" heap[{}]:", sched.stride_heap.len()));
            for e in sched.stride_heap.iter() {
                let pid = e.0.process.as_ref().map(|p| p.id);
                let _ = core::fmt::write(&mut w, format_args!("pid{:?} pass={} ", pid, e.0.pass));
            }
            let _ = core::fmt::write(&mut w, format_args!(" ready[ consolidated ]"));
        }
        if let Some(q) = GLOBAL.sleep_queue.try_lock() {
            let _ = core::fmt::write(&mut w, format_args!("sleep[{}]:", q.len()));
            for t in q.iter() {
                let pid = t.process.as_ref().map(|p| p.id);
                let _ = core::fmt::write(&mut w, format_args!(" (pid={:?} {:?} wake={:?})", pid, t.status, t.futex_wake_addr));
            }
        }
        if let Some(q) = GLOBAL.futex_queue.try_lock() {
            let _ = core::fmt::write(&mut w, format_args!(" futex[{}]:", q.len()));
            for t in q.iter() {
                let pid = t.process.as_ref().map(|p| p.id);
                let _ = core::fmt::write(&mut w, format_args!(" (pid={:?} {:?} wake={:?})", pid, t.status, t.futex_wake_addr));
            }
        }
        if let Some(q) = GLOBAL.block_queue.try_lock() {
            let _ = core::fmt::write(&mut w, format_args!(" block[{}]:", q.len()));
            for t in q.iter() {
                let pid = t.process.as_ref().map(|p| p.id);
                let _ = core::fmt::write(&mut w, format_args!(" (pid={:?} {:?} pipe={:?})", pid, t.status, t.pipe_block_key));
            }
        }
        if let Some(q) = GLOBAL.pending_queue.try_lock() {
            let _ = core::fmt::write(&mut w, format_args!(" pending[{}]", q.len()));
        }
        let _ = core::fmt::write(&mut w, format_args!("\n"));
        let tlen = w.len;
        crate::serial_write(core::str::from_utf8(&tscratch[..tlen]).unwrap_or(""));
    }

    // Soft-lockup detector: one RIP pinned for 500 consecutive ticks is a
    // busy loop with IF=1 (timer still fires). Idle can't match â€” the idle
    // incarnation parks a *Blocked* current thread, and blocked threads
    // never execute. IF=0 spins suppress the tick entirely, so they are
    // invisible here; the external QEMU monitor catches those instead.
    soft_lockup_check(_stack_frame.instruction_pointer.as_u64());

    crate::apic::eoi();

    crate::task::scheduler::tick(ticks);
    crate::task::scheduler::try_schedule();
}

extern "x86-interrupt" fn tlb_flush_handler(
    _stack_frame: InterruptStackFrame)
{
    unsafe {
        use x86_64::registers::control::Cr3;
        let (frame, flags) = Cr3::read();
        Cr3::write(frame, flags);
    }
    crate::apic::eoi();
}

/// Kill the current user-mode process after an unresolvable fault (RPL 3),
/// shared by the #PF, #GP, #UD and #SS handlers. Records exit code 139
/// (SIGSEGV), raises SIGCHLD, marks the thread Exited, and schedules — never
/// returning to the faulted frame. IRQ context: only VMA/futex/sched locks,
/// no allocation.
#[cfg(not(target_arch = "aarch64"))]
fn kill_user_process(fault_addr: u64, rip: u64, rsp: u64, err_bits: u64, why: &str) -> ! {
    {
        let mut scratch = [0u8; 256];
        let dbg_len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let (pid, in_vma, nvma, brk) = {
                let pcur = crate::task::process::CURRENT_PROCESS.lock();
                match pcur.as_ref() {
                    Some(p) => (
                        p.id,
                        p.find_vma(fault_addr).is_some(),
                        p.vmas.lock().len(),
                        *p.brk.lock(),
                    ),
                    None => (u64::MAX, false, 0, 0),
                }
            };
            let _ = core::fmt::write(&mut w, format_args!(
                "[SIGSEGV] pid={} addr={:#x} rip={:#x} rsp={:#x} err={:#x} ({}) vma={} nvma={} brk={:#x} (killing process)
",
                pid, fault_addr, rip, rsp, err_bits, why, in_vma, nvma, brk,
            ));
            dbg_len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..dbg_len]).unwrap_or(""));
    }
    // SIGVM diagnostic: dump the faulting process's and its parent's VMA
    // ranges so we can see whether the faulting address was ever mapped
    // (clone_cow missing pages) or was never mapped at all (corrupted
    // userspace free list). IRQ context: vmas locks only, no alloc.
    {
        let mut scratch = [0u8; 2048];
        let dbg_len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let ppid = crate::task::process::CURRENT_PROCESS.lock()
                .as_ref().map(|p| p.parent_id).unwrap_or(None);
            let _ = core::fmt::write(&mut w, format_args!("[SIGVM] ppid={:?} addr={:#x}
", ppid, fault_addr));
            let cur = crate::task::process::CURRENT_PROCESS.lock();
            if let Some(p) = cur.as_ref() {
                let vmas = p.vmas.lock();
                let _ = core::fmt::write(&mut w, format_args!("[SIGVM] cur pid={} n={}:", p.id, vmas.len()));
                for v in vmas.iter() {
                    let _ = core::fmt::write(&mut w, format_args!(" [{:#x},{:#x})", v.start, v.end));
                }
                let _ = core::fmt::write(&mut w, format_args!("
"));
            }
            if let Some(pp) = ppid {
                let table = crate::task::process::PROCESS_TABLE.lock();
                if let Some(par) = table.get(&pp) {
                    let vmas = par.vmas.lock();
                    let _ = core::fmt::write(&mut w, format_args!("[SIGVM] parent pid={} n={}:", par.id, vmas.len()));
                    for v in vmas.iter() {
                        let _ = core::fmt::write(&mut w, format_args!(" [{:#x},{:#x})", v.start, v.end));
                    }
                    let _ = core::fmt::write(&mut w, format_args!("
"));
                }
            }
            dbg_len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..dbg_len]).unwrap_or(""));
    }

    // Same bookkeeping as sys_exit: record the exit code so a wait() on
    // the parent can reap it, and wake the parent on SIGCHLD. skip the
    // clear_child_tid futex wake — in IRQ context a contended futex lock
    // would deadlock; a killed process has nobody expected to join it.
    {
        let (parent_pid, status) = {
            let process_lock = crate::task::process::CURRENT_PROCESS.lock();
            if let Some(ref process) = *process_lock {
                *process.exit_code.lock() = Some(139); // SIGSEGV (128+11)
                (process.parent_id, process.exit_code.lock().is_some())
            } else { (None, false) }
        };
        if let Some(ppid) = parent_pid {
            let table = crate::task::process::PROCESS_TABLE.lock();
            if let Some(parent) = table.get(&ppid) {
                parent.signals.lock().raise(crate::syscalls::signal::Signal::SIGCHLD);
            }
        }
        core::hint::black_box(status);
    }

    crate::task::scheduler::with_current_thread(|thread| {
        thread.status = crate::task::thread::ThreadStatus::Exited;
    });
    crate::task::scheduler::schedule();
    // Never return to the faulting user frame.
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

// Page-fault dispatch trampoline. The IDT #PF entry points here instead of
// directly at `page_fault_handler` so user-copy faults can be aborted
// (exception-table style) rather than panicking the kernel.
//
// For RING-0 faults only (a copy runs in kernel mode, GS base is kernel):
// stash the fault-entry RSP into per-CPU `pf_entry_rsp` (`abort_user_copy`
// uses it to iret into the fixup). User-mode faults keep GS=user, so GS is
// never read there â€” the trampoline just forwards to the normal handler.
// All GPRs are preserved; the entry stack layout is left byte-for-byte intact.
#[cfg(not(target_arch = "aarch64"))]
core::arch::global_asm!(
    r#"
    .global vahi_pf_dispatch
    vahi_pf_dispatch:
        push rax
        push rcx
        push rdx
        # Entry stack: [rsp]=err [rsp+8]=RIP [rsp+16]=CS [rsp+24]=RFLAGS â€” save regs below
        # After 3 pushes: err at [rsp+24], RIP at [rsp+32], CS at [rsp+40]
        mov rax, [rsp + 40]             # CS
        and rax, 3                      # RPL: 0 = kernel mode
        jz 990f                         # ring-0 fault -> stash entry RSP
        jmp 991f                        # ring-3 fault -> normal path (GS is user's)
    990:
        lea rax, [rsp + 24]             # entry RSP (points at error-code slot)
        mov rcx, gs:[0x0]              # PerCpuData base (kernel GS, ring-0 only)
        mov [rcx + 0x48], rax          # pf_entry_rsp = entry RSP
    991:
        pop rdx
        pop rcx
        pop rax
        jmp page_fault_handler
    "#
);
extern "C" {
    #[cfg(not(target_arch = "aarch64"))]
    fn vahi_pf_dispatch();
}

#[cfg(not(target_arch = "aarch64"))]
#[no_mangle]
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    let fault_addr = Cr2::read();

    let cur = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref proc) = *cur {
        let page_addr = fault_addr.as_u64() & !0xFFF;
        let page = x86_64::structures::paging::Page::containing_address(fault_addr);

        // Check global swap map for a swapped-out page
        let swap_entry = crate::memory::swap::SWAP_PAGE_MAP.lock().remove(&page_addr);

        if let Some((_dev_idx, _slot_idx)) = swap_entry {
            drop(cur);
            if let Some(phys_addr) = crate::memory::swap::swap_in_page(page_addr) {
                use crate::memory::buddy::BuddyFrameAllocator;
                use x86_64::structures::paging::Mapper;
                let mut fa = BuddyFrameAllocator;
                // SAFETY: mapper through physical memory offset is valid during swap-in
                if let Some(proc2) = crate::task::process::CURRENT_PROCESS.lock().as_ref().map(|p| p.clone()) {
                    if let Some(mut mapper) = unsafe { proc2.address_space.mapper() } {
                        let frame = x86_64::structures::paging::PhysFrame::containing_address(
                            x86_64::PhysAddr::new(phys_addr)
                        );
                        let flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE
                            | PageTableFlags::WRITABLE;
                        let _ = unsafe { mapper.map_to(page, frame, flags, &mut fa).map(|f| f.flush()) };
                        return;
                    }
                }
            }
            panic!("PAGE FAULT: swap-in failed for {:?}", fault_addr);
        }

        if let Some(true) = unsafe { proc.address_space.handle_cow(page) } {
            return;
        }
        if !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            if let Some(vma) = proc.find_vma(fault_addr.as_u64()) {
                // Cgroup memory.max check — enforce at page fault time
                {
                    let cg_path = proc.cgroup_path.lock();
                    let hierarchy = crate::syscalls::cgroup::cgroup_ensure();
                    if let Some(cg) = hierarchy.find_cgroup(&cg_path) {
                        if !cg.can_allocate(4096) {
                            return; // Don't allocate — cgroup memory limit reached
                        }
                    }
                }
                use crate::memory::buddy::BuddyFrameAllocator;
                use x86_64::structures::paging::{Mapper, FrameAllocator};
                let mut fa = BuddyFrameAllocator;
                if let Some(frame) = fa.allocate_frame() {
                    if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                        let mut flags = vma.flags | PageTableFlags::PRESENT;

                        if fault_addr.as_u64() < 0x8000_0000_0000 {
                            flags |= PageTableFlags::USER_ACCESSIBLE;
                        }

                        let _ = unsafe { mapper.map_to(page, frame, flags, &mut fa).map(|f| f.flush()) };
                        crate::memory::frame_info::increment(frame.start_address());

                        let virt = x86_64::VirtAddr::new(
                            crate::memory::physical_memory_offset()
                            + frame.start_address().as_u64()
                        );
                        unsafe { core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, 4096); }
                        return;
                    }
                }
            }
            let fault_u64 = fault_addr.as_u64();
            if fault_u64 >= 0x6000_0000_0000 && fault_u64 < *proc.brk.lock() {
                // Cgroup memory.max check for brk region
                {
                    let cg_path = proc.cgroup_path.lock();
                    let hierarchy = crate::syscalls::cgroup::cgroup_ensure();
                    if let Some(cg) = hierarchy.find_cgroup(&cg_path) {
                        if !cg.can_allocate(4096) {
                            return; // Don't allocate — cgroup memory limit reached
                        }
                    }
                }
                use crate::memory::buddy::BuddyFrameAllocator;
                use x86_64::structures::paging::{Mapper, FrameAllocator};
                let mut fa = BuddyFrameAllocator;
                if let Some(frame) = fa.allocate_frame() {
                    if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
                        let _ = unsafe { mapper.map_to(page, frame, flags, &mut fa).map(|f| f.flush()) };
                        crate::memory::frame_info::increment(frame.start_address());
                        let virt = x86_64::VirtAddr::new(
                            crate::memory::physical_memory_offset()
                            + frame.start_address().as_u64()
                        );
                        unsafe { core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, 4096); }
                        return;
                    }
                }
            }
        }
    }
    drop(cur);

    // Unresolvable fault on a USER address while a copy is active = bad user
    // pointer (e.g. unmapped, out of any VMA). Abort the copy (return EFAULT)
    // instead of panicking the kernel. Kernel-range faults still panic, and
    // user-mode faults can't be copies (RPL gate also keeps the GS read safe).
    if fault_addr.as_u64() < 0x0000_8000_0000_0000
        && stack_frame.code_segment & 3 == 0
        && crate::syscalls::user_access::user_copy_active()
    {
        crate::syscalls::user_access::abort_user_copy();
    }

    // Unresolvable USER-mode fault (RPL 3): the process dereferenced an
    // invalid address (e.g. the NULL-write from a corrupted heap). Kill it
    // (SIGSEGV) instead of panicking the whole kernel. schedule() picks the
    // next thread, frees the Exited thread's stack, and never returns to the
    // faulted frame; if nothing else runs it idles, and we must never iret
    // back into the faulting user code, so fall through to an idle hang.
    if stack_frame.code_segment & 3 == 3 {
        let mut scratch = [0u8; 256];
        let dbg_len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let (pid, in_vma, nvma, brk) = {
                let pcur = crate::task::process::CURRENT_PROCESS.lock();
                match pcur.as_ref() {
                    Some(p) => (
                        p.id,
                        p.find_vma(fault_addr.as_u64()).is_some(),
                        p.vmas.lock().len(),
                        *p.brk.lock(),
                    ),
                    None => (u64::MAX, false, 0, 0),
                }
            };
            let _ = core::fmt::write(&mut w, format_args!(
                "[SIGSEGV] pid={} addr={:#x} rip={:#x} rsp={:#x} err={} (P:{} W:{} U:{} I:{}) vma={} nvma={} brk={:#x} (killing process)\n",
                pid, fault_addr.as_u64(),
                stack_frame.instruction_pointer.as_u64(),
                stack_frame.stack_pointer.as_u64(),
                error_code.bits(),
                !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION),
                error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE),
                error_code.contains(PageFaultErrorCode::USER_MODE),
                error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH),
                in_vma, nvma, brk,
            ));
            dbg_len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..dbg_len]).unwrap_or(""));
        // SIGVM diagnostic: dump the faulting process's and its parent's VMA
        // ranges so we can see whether the faulting address was ever mapped
        // (clone_cow missing pages) or was never mapped at all (corrupted
        // userspace free list). IRQ context: vmas locks only, no alloc.
        {
            let mut scratch = [0u8; 2048];
            let dbg_len;
            {
                let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
                let ppid = crate::task::process::CURRENT_PROCESS.lock()
                    .as_ref().map(|p| p.parent_id).unwrap_or(None);
                let _ = core::fmt::write(&mut w, format_args!("[SIGVM] ppid={:?} addr={:#x}
", ppid, fault_addr.as_u64()));
                let cur = crate::task::process::CURRENT_PROCESS.lock();
                if let Some(p) = cur.as_ref() {
                    let vmas = p.vmas.lock();
                    let _ = core::fmt::write(&mut w, format_args!("[SIGVM] cur pid={} n={}:", p.id, vmas.len()));
                    for v in vmas.iter() {
                        let _ = core::fmt::write(&mut w, format_args!(" [{:#x},{:#x})", v.start, v.end));
                    }
                    let _ = core::fmt::write(&mut w, format_args!("
"));
                }
                if let Some(pp) = ppid {
                    let table = crate::task::process::PROCESS_TABLE.lock();
                    if let Some(par) = table.get(&pp) {
                        let vmas = par.vmas.lock();
                        let _ = core::fmt::write(&mut w, format_args!("[SIGVM] parent pid={} n={}:", par.id, vmas.len()));
                        for v in vmas.iter() {
                            let _ = core::fmt::write(&mut w, format_args!(" [{:#x},{:#x})", v.start, v.end));
                        }
                        let _ = core::fmt::write(&mut w, format_args!("
"));
                    }
                }
                dbg_len = w.len;
            }
            crate::serial_write(core::str::from_utf8(&scratch[..dbg_len]).unwrap_or(""));
        }


        // Same bookkeeping as sys_exit: record the exit code so a wait() on
        // the parent can reap it, and wake the parent on SIGCHLD. skip the
        // clear_child_tid futex wake â€” in IRQ context a contended futex lock
        // would deadlock; a killed process has nobody expected to join it.
        {
            let (parent_pid, status) = {
                let process_lock = crate::task::process::CURRENT_PROCESS.lock();
                if let Some(ref process) = *process_lock {
                    *process.exit_code.lock() = Some(139); // SIGSEGV (128+11)
                    (process.parent_id, process.exit_code.lock().is_some())
                } else { (None, false) }
            };
            if let Some(ppid) = parent_pid {
                let table = crate::task::process::PROCESS_TABLE.lock();
                if let Some(parent) = table.get(&ppid) {
                    parent.signals.lock().raise(crate::syscalls::signal::Signal::SIGCHLD);
                }
            }
            core::hint::black_box(status);
        }

        crate::task::scheduler::with_current_thread(|thread| {
            thread.status = crate::task::thread::ThreadStatus::Exited;
        });
        crate::task::scheduler::schedule();
        // Never return to the faulting user frame.
        loop {
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    }

    // Print the interrupted context BEFORE any further dereferences: the panic
    // path below must never fault again (a nested fault here would mask the
    // original one). Only dump the raw stack words for KERNEL-mode faults â€” a
    // user-mode frame's stack_pointer is a USER address and may be unmapped.
    {
        let mut scratch = [0u8; 2048];
        let dump_len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let _ = core::fmt::write(&mut w, format_args!(
                "FAULT CTX: rip={:#x} cs={:#x} rflags={:#x} rsp={:#x} ss={:#x}\n",
                stack_frame.instruction_pointer.as_u64(),
                stack_frame.code_segment,
                stack_frame.cpu_flags,
                stack_frame.stack_pointer.as_u64(),
                stack_frame.stack_segment,
            ));
            dump_len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..dump_len]).unwrap_or(""));
    }

    // Dump the faulting stack: for a CALL to a garbage address, [SP] holds the
    // return address of the call site. Formatted without allocation (IRQ ctx).
    if stack_frame.code_segment & 3 == 0 {
        let mut scratch = [0u8; 2048];
        let dump_len;
        {
            let mut w = IrqFmtBuf { buf: &mut scratch, len: 0 };
            let sp = stack_frame.stack_pointer.as_u64();
            let page = sp & !0xFFF;
            let _ = core::fmt::write(&mut w, format_args!("FAULT STACK @ {:#x} (page {:#x}):\n", sp, page));
            for i in 0..48usize {
                let addr = sp + (i as u64) * 8;
                // Only read within the page containing SP â€” the panic path must
                // never fault again on a guard page.
                if addr & !0xFFF != page { break; }
                // SAFETY: SP is a live kernel stack; reads are best-effort diagnostics
                let word = unsafe { *(addr as *const u64) };
                let _ = core::fmt::write(&mut w, format_args!("  [{:02}] {:016x}\n", i, word));
            }
            dump_len = w.len;
        }
        crate::serial_write(core::str::from_utf8(&scratch[..dump_len]).unwrap_or(""));
    }

    panic!(
        "PAGE FAULT at {:?}  error={:?}\n{:#?}",
        fault_addr, error_code, stack_frame
    );
}

extern "x86-interrupt" fn mouse_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    crate::drivers::mouse::MOUSE_IRQ_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    loop {
        let mut status_port = Port::<u8>::new(0x64);
        let status = unsafe { status_port.read() };
        if status & 1 == 0 {
            break;
        }
        let mut data_port = Port::<u8>::new(0x60);
        let byte = unsafe { data_port.read() };

        if status & 0x20 != 0 {
            crate::drivers::mouse::feed_byte(byte);
        } else {
            crate::keyboard::handle_scancode(byte);
            crate::tty::feed_scancode(byte);
        }
    }

    // IRQ12 arrives via IOAPIC->LAPIC (vec 44); the PIC is masked, so only the
    // LAPIC EOI clears ISR44. Without it the LAPIC suppresses all class-2
    // vectors (32-47) on this CPU, including the timer (vec 32).
    crate::apic::eoi();
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    // One-shot: print on first IRQ1 fire
    static KB_FIRED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if !KB_FIRED.swap(true, core::sync::atomic::Ordering::Relaxed) {
        crate::serial_write("[KBD] IRQ1 fired!\n");
    }

    loop {
        let mut status_port = Port::<u8>::new(0x64);
        let status = unsafe { status_port.read() };
        if status & 1 == 0 {
            break;
        }
        let mut data_port = Port::<u8>::new(0x60);
        let byte = unsafe { data_port.read() };

        if status & 0x20 != 0 {
            crate::drivers::mouse::feed_byte(byte);
        } else {
            crate::keyboard::handle_scancode(byte);
            crate::tty::feed_scancode(byte);
        }
    }

    // IRQ1 arrives via IOAPIC->LAPIC (vec 33); PIC is masked, so only the
    // LAPIC EOI clears ISR33 (same class-2 reasoning as the mouse handler).
    crate::apic::eoi();
}

extern "x86-interrupt" fn ipi_func_handler(
    _stack_frame: InterruptStackFrame)
{
    let cpu = crate::syscalls::get_per_cpu();
    let kind = cpu.ipi_kind.swap(0, core::sync::atomic::Ordering::AcqRel);
    match kind {
        1 => {
            // TlbShootdown
            unsafe {
                use x86_64::registers::control::Cr3;
                let (frame, flags) = Cr3::read();
                Cr3::write(frame, flags);
            }
        }
        2 => {
            // Reschedule
            crate::task::scheduler::try_schedule();
        }
        3 => {
            // Func â€” call registered function pointer
            let func_val = cpu.ipi_arg.swap(0, core::sync::atomic::Ordering::AcqRel);
            if func_val != 0 {
                let func: extern "C" fn(u64) = unsafe { core::mem::transmute(func_val) };
                func(0);
            }
        }
        _ => {}
    }
    crate::apic::eoi();
}

extern "x86-interrupt" fn network_interrupt_handler(
    _stack_frame: InterruptStackFrame) 
{
    #[cfg(feature = "net")]
    {
        let icr = crate::drivers::net::NIC.lock().as_ref().map(|nic| {
            match nic {
                crate::drivers::net::NicDevice::E1000(dev) => {
                    dev.lock().inner.read_reg(crate::drivers::net::e1000::REG_ICR)
                }
                _ => 0,
            }
        }).unwrap_or(0);

        if icr == 0 {
            crate::apic::eoi();
            return;
        }

        crate::net::poll();
    }
    crate::apic::eoi();
}



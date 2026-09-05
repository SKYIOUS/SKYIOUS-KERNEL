//! Page fault handler (#PF) and its inline-assembly trampoline.
//!
//! The trampoline (`vahi_pf_dispatch`) stashes the entry RSP for
//! `abort_user_copy` before entering the Rust handler. The handler itself
//! handles: swap page-in, copy-on-write, demand paging with cgroup
//! enforcement, user-copy abort, user-mode SIGSEGV (with VMA dumps),
//! and kernel-mode fault diagnostics.

use x86_64::structures::idt::{InterruptStackFrame, PageFaultErrorCode};
use x86_64::structures::paging::PageTableFlags;

use super::diag::IrqFmtBuf;

// Page-fault dispatch trampoline. The IDT #PF entry points here instead of
// directly at `page_fault_handler` so user-copy faults can be aborted
// (exception-table style) rather than panicking the kernel.
//
// For RING-0 faults only (a copy runs in kernel mode, GS base is kernel):
// stash the fault-entry RSP into per-CPU `pf_entry_rsp` (`abort_user_copy`
// uses it to iret into the fixup). User-mode faults keep GS=user, so GS is
// never read there — the trampoline just forwards to the normal handler.
// All GPRs are preserved; the entry stack layout is left byte-for-byte intact.
#[cfg(not(target_arch = "aarch64"))]
core::arch::global_asm!(
    r#"
    .global vahi_pf_dispatch
    vahi_pf_dispatch:
        push rax
        push rcx
        push rdx
        # Entry stack: [rsp]=err [rsp+8]=RIP [rsp+16]=CS [rsp+24]=RFLAGS — save regs below
        # After 3 pushes: err at [rsp+24], RIP at [rsp+32], CS at [rsp+40]
        mov rax, [rsp + 40]             # CS
        and rax, 3                      # RPL: 0 = kernel mode
        jz 990f                         # ring-0 fault -> stash entry RSP
        jmp 991f                        # ring-3 fault -> normal path (GS is user's)
    990:
        lea rax, [rsp + 24]             # entry RSP (points at error-code slot)
        mov rcx, gs:[0x0]              # PerCpuData base (kernel GS, ring-0 only)
        mov [rcx + 0x48], rax          # pf_entry_rsp = entry RSP
        # Stash callee-saved regs: the x86-interrupt handler clobbers them
        # while running, and abort_user_copy iretq's out of the handler
        # directly, bypassing the ABI epilogue that would restore them.
        # Offsets must match PerCpuData::pf_callee_saved (PF_CALLEE_SAVED_OFFSET).
        mov [rcx + 0x50], rbx
        mov [rcx + 0x58], rbp
        mov [rcx + 0x60], r12
        mov [rcx + 0x68], r13
        mov [rcx + 0x70], r14
        mov [rcx + 0x78], r15
    991:
        pop rdx
        pop rcx
        pop rax
        jmp page_fault_handler
    "#
);
extern "C" {
    #[cfg(not(target_arch = "aarch64"))]
    pub(super) fn vahi_pf_dispatch();
}

#[cfg(not(target_arch = "aarch64"))]
#[no_mangle]
pub(super) extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    let fault_addr = Cr2::read();

    // Fast path: if a user_copy is active and this is a user-range fault,
    // abort the copy immediately — no process context needed.
    if fault_addr.as_u64() < 0x0000_8000_0000_0000
        && stack_frame.code_segment & 3 == 0
        && crate::syscalls::user_access::user_copy_active()
    {
        crate::syscalls::user_access::abort_user_copy();
    }

    // The faulting process, acquired WITHOUT the global CURRENT_PROCESS
    // lock. For a user-mode fault the faulting context IS this CPU's
    // current thread, so its `process` Arc is the correct process and is
    // reachable per-CPU. The global is a single mirror updated at every
    // context switch; another CPU can hold it for a long stretch (sys_fork
    // clones the whole address space under it), which wedged the fault
    // handler in a cross-CPU spin on -smp boots (lock holder never
    // releases while the faulting CPU spins with interrupts off). Fall
    // back to the global (bounded spin) only when no thread is running on
    // this CPU (boot/idle faults), where no other CPU can be holding it.
    let cur: Option<alloc::sync::Arc<crate::task::process::Process>> = {
        let sched = crate::task::scheduler::this_cpu_sched().lock();
        match sched
            .current_thread
            .as_ref()
            .and_then(|t| t.process.clone())
        {
            Some(p) => Some(p),
            None => {
                drop(sched);
                let mut attempts = 0u32;
                let guard = loop {
                    if let Some(g) = crate::task::process::CURRENT_PROCESS.try_lock() {
                        break g;
                    }
                    attempts += 1;
                    if attempts >= 8_000_000 {
                        panic!(
                            "PAGE FAULT at {:?}  error={:?} (CURRENT_PROCESS locked after {} retries)",
                            fault_addr, error_code, attempts
                        );
                    }
                    core::hint::spin_loop();
                };
                guard.as_ref().map(|p| p.clone())
            }
        }
    };
    if let Some(ref proc) = cur {
        let page_addr = fault_addr.as_u64() & !0xFFF;
        let page = x86_64::structures::paging::Page::containing_address(fault_addr);

        // Check global swap map for a swapped-out page
        let swap_entry = crate::memory::swap::SWAP_PAGE_MAP.lock().remove(&page_addr);

        if let Some((_dev_idx, _slot_idx)) = swap_entry {
            if let Some(phys_addr) = crate::memory::swap::swap_in_page(page_addr) {
                use crate::memory::buddy::BuddyFrameAllocator;
                use x86_64::structures::paging::Mapper;
                let mut fa = BuddyFrameAllocator;
                // SAFETY: mapper through physical memory offset is valid during swap-in
                if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                    let frame = x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(phys_addr),
                    );
                    let flags = PageTableFlags::PRESENT
                        | PageTableFlags::USER_ACCESSIBLE
                        | PageTableFlags::WRITABLE;
                    let _ = unsafe {
                        mapper
                            .map_to(page, frame, flags, &mut fa)
                            .map(|f| f.flush())
                    };
                    return;
                }
            }
            panic!("PAGE FAULT: swap-in failed for {:?}", fault_addr);
        }

        if let Some(true) = unsafe { proc.address_space.handle_cow(page) } {
            return;
        }
        if !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            if let Some(vma) = proc.find_vma(fault_addr.as_u64()) {
                // vm.overcommit_memory mode 2: reject fault if commit limit reached
                if !crate::memory::overcommit::check_fault_commit() {
                    return; // Overcommit policy denial — page not allocated
                }
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
                use x86_64::structures::paging::{FrameAllocator, Mapper};
                let mut fa = BuddyFrameAllocator;
                if let Some(frame) = fa.allocate_frame() {
                    if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                        let mut flags = vma.flags | PageTableFlags::PRESENT;

                        if fault_addr.as_u64() < 0x8000_0000_0000 {
                            flags |= PageTableFlags::USER_ACCESSIBLE;
                        }

                        let _ = unsafe {
                            mapper
                                .map_to(page, frame, flags, &mut fa)
                                .map(|f| f.flush())
                        };
                        crate::memory::frame_info::increment(frame.start_address());

                        // Account memory in cgroup
                        {
                            let cg_path = proc.cgroup_path.lock();
                            crate::syscalls::cgroup::cgroup_account_memory(&cg_path, 4096);
                        }

                        let virt = x86_64::VirtAddr::new(
                            crate::memory::physical_memory_offset()
                                + frame.start_address().as_u64(),
                        );
                        unsafe {
                            core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, 4096);
                        }
                        return;
                    }
                }
            } else {
                // Diagnostic: no VMA found for fault address
            }
            let fault_u64 = fault_addr.as_u64();
            if fault_u64 >= 0x6000_0000_0000 && fault_u64 < proc.memory.lock().brk {
                // vm.overcommit_memory mode 2: reject fault if commit limit reached
                if !crate::memory::overcommit::check_fault_commit() {
                    return; // Overcommit policy denial — page not allocated
                }
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
                use x86_64::structures::paging::{FrameAllocator, Mapper};
                let mut fa = BuddyFrameAllocator;
                if let Some(frame) = fa.allocate_frame() {
                    if let Some(mut mapper) = unsafe { proc.address_space.mapper() } {
                        let flags = PageTableFlags::PRESENT
                            | PageTableFlags::WRITABLE
                            | PageTableFlags::USER_ACCESSIBLE;
                        let _ = unsafe {
                            mapper
                                .map_to(page, frame, flags, &mut fa)
                                .map(|f| f.flush())
                        };
                        crate::memory::frame_info::increment(frame.start_address());
                        // Account memory in cgroup
                        {
                            let cg_path = proc.cgroup_path.lock();
                            crate::syscalls::cgroup::cgroup_account_memory(&cg_path, 4096);
                        }
                        let virt = x86_64::VirtAddr::new(
                            crate::memory::physical_memory_offset()
                                + frame.start_address().as_u64(),
                        );
                        unsafe {
                            core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, 4096);
                        }
                        return;
                    }
                }
            }
        }
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
            let mut w = IrqFmtBuf {
                buf: &mut scratch,
                len: 0,
            };
            let (pid, in_vma, nvma, brk) = match cur.as_ref() {
                Some(p) => {
                    // One try_lock (I4): never block in IF=0 context, and no
                    // nested acquisition — the range scan runs inline under
                    // the guard instead of via find_vma (which re-locks
                    // memory internally). On contention, dump what we can
                    // without the VMA details.
                    let (in_vma, nvma, brk) = match p.memory.try_lock() {
                        Some(mem) => (
                            mem.vmas.iter().any(|v| {
                                fault_addr.as_u64() >= v.start && fault_addr.as_u64() < v.end
                            }),
                            mem.vmas.len(),
                            mem.brk,
                        ),
                        None => (false, 0, 0),
                    };
                    (p.id, in_vma, nvma, brk)
                }
                None => (u64::MAX, false, 0, 0),
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
                let mut w = IrqFmtBuf {
                    buf: &mut scratch,
                    len: 0,
                };
                let ppid = cur.as_ref().map(|p| p.parent_id).unwrap_or(None);
                let _ = core::fmt::write(
                    &mut w,
                    format_args!("[SIGVM] ppid={:?} addr={:#x}\n", ppid, fault_addr.as_u64()),
                );
                // try_lock + format under the guard (I4): no blocking and no
                // heap allocation (the previous vmas.clone() allocated in
                // IF=0 context — an I4 violation of its own). On contention
                // the dump is skipped; it is best-effort diagnostics.
                if let Some(p) = cur.as_ref() {
                    if let Some(mem) = p.memory.try_lock() {
                        let _ = core::fmt::write(
                            &mut w,
                            format_args!("[SIGVM] cur pid={} n={}:", p.id, mem.vmas.len()),
                        );
                        for v in mem.vmas.iter() {
                            let _ = core::fmt::write(
                                &mut w,
                                format_args!(" [{:#x},{:#x})", v.start, v.end),
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
                                format_args!("[SIGVM] parent pid={} n={}:", par.id, mem.vmas.len()),
                            );
                            for v in mem.vmas.iter() {
                                let _ = core::fmt::write(
                                    &mut w,
                                    format_args!(" [{:#x},{:#x})", v.start, v.end),
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
        {
            if let Some(ref proc) = cur {
                proc.kill_from_fault(); // -> !
            }
        }
        // Should not reach here for user faults.
        loop {
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    }

    // Print the interrupted context BEFORE any further dereferences: the panic
    // path below must never fault again (a nested fault here would mask the
    // original one). Only dump the raw stack words for KERNEL-mode faults — a
    // user-mode frame's stack_pointer is a USER address and may be unmapped.
    {
        let mut scratch = [0u8; 2048];
        let dump_len;
        {
            let mut w = IrqFmtBuf {
                buf: &mut scratch,
                len: 0,
            };
            let _ = core::fmt::write(
                &mut w,
                format_args!(
                    "FAULT CTX: rip={:#x} cs={:#x} rflags={:#x} rsp={:#x} ss={:#x}\n",
                    stack_frame.instruction_pointer.as_u64(),
                    stack_frame.code_segment,
                    stack_frame.cpu_flags,
                    stack_frame.stack_pointer.as_u64(),
                    stack_frame.stack_segment,
                ),
            );
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
            let mut w = IrqFmtBuf {
                buf: &mut scratch,
                len: 0,
            };
            let sp = stack_frame.stack_pointer.as_u64();
            let page = sp & !0xFFF;
            let _ = core::fmt::write(
                &mut w,
                format_args!("FAULT STACK @ {:#x} (page {:#x}):\n", sp, page),
            );
            for i in 0..48usize {
                let addr = sp + (i as u64) * 8;
                // Only read within the page containing SP — the panic path must
                // never fault again on a guard page.
                if addr & !0xFFF != page {
                    break;
                }
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

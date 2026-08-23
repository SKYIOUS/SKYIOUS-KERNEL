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
            if fault_u64 >= 0x6000_0000_0000 && fault_u64 < proc.memory.lock().brk {
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
                        p.memory.lock().vmas.len(),
                        p.memory.lock().brk,
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
                let _ = core::fmt::write(&mut w, format_args!("[SIGVM] ppid={:?} addr={:#x}\n", ppid, fault_addr.as_u64()));
                let cur = crate::task::process::CURRENT_PROCESS.lock();
                if let Some(p) = cur.as_ref() {
                    let vmas = p.memory.lock().vmas.clone();
                    let _ = core::fmt::write(&mut w, format_args!("[SIGVM] cur pid={} n={}:", p.id, vmas.len()));
                    for v in vmas.iter() {
                        let _ = core::fmt::write(&mut w, format_args!(" [{:#x},{:#x})", v.start, v.end));
                    }
                    let _ = core::fmt::write(&mut w, format_args!("\n"));
                }
                if let Some(pp) = ppid {
                    let table = crate::task::process::PROCESS_TABLE.lock();
                    if let Some(par) = table.get(&pp) {
                        let vmas = par.memory.lock().vmas.clone();
                        let _ = core::fmt::write(&mut w, format_args!("[SIGVM] parent pid={} n={}:", par.id, vmas.len()));
                        for v in vmas.iter() {
                            let _ = core::fmt::write(&mut w, format_args!(" [{:#x},{:#x})", v.start, v.end));
                        }
                        let _ = core::fmt::write(&mut w, format_args!("\n"));
                    }
                }
                dbg_len = w.len;
            }
            crate::serial_write(core::str::from_utf8(&scratch[..dbg_len]).unwrap_or(""));
        }


        // Delegate exit bookkeeping to Process::kill_from_fault()
        {
            let pcur = crate::task::process::CURRENT_PROCESS.lock();
            if let Some(ref proc) = *pcur {
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
                // Only read within the page containing SP — the panic path must
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

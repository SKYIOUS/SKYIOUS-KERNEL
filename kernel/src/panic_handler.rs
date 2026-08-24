//! Panic handler — register dumps, stack trace, process info, halt.
//!
//! Extracted from main.rs to keep the crate root under 400 lines.

use core::panic::PanicInfo;

/// Full panic handler body: message → CPU info → registers → backtrace → halt.
pub fn handle_panic(info: &PanicInfo) -> ! {
    crate::serial_write("\n========================================\n");
    crate::serial_write("           KERNEL PANIC\n");
    crate::serial_write("========================================\n");

    // ── 1. Panic message ──
    {
        let msg = info.message();
        let panic_str = alloc::format!("{:?}", msg);
        crate::serial_write("[PANIC] ");
        crate::serial_write(&panic_str);
        crate::serial_write("\n");
    }
    if let Some(loc) = info.location() {
        crate::serial_write(&alloc::format!("[PANIC] at {}:{}\n", loc.file(), loc.line()));
    }

    // ── 2. CPU & process info ──
    #[cfg(target_arch = "x86_64")]
    {
        let cpu = crate::apic::current_lapic_id();
        crate::serial_write(&alloc::format!("[PANIC] CPU: {}\n", cpu));
    }
    #[cfg(target_arch = "aarch64")]
    {
        let mpidr: u64;
        unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr); }
        crate::serial_write(&alloc::format!("[PANIC] CPU: {}\n", mpidr & 0xFF));
    }
    if let Some(tid) = crate::task::scheduler::with_current_thread(|t| {
        let pid = t.process.as_ref().map(|p| p.id).unwrap_or(0);
        (t.id, pid)
    }) {
        crate::serial_write(&alloc::format!("[PANIC] Thread: {}, PID: {}\n", tid.0, tid.1));
    }

    // ── 3. Register dump ──
    #[cfg(target_arch = "x86_64")]
    dump_registers_x86_64();
    #[cfg(target_arch = "aarch64")]
    dump_registers_aarch64();

    // ── 4. Stack backtrace ──
    crate::debug::print_stack_trace();

    // ── 5. Boot trace (if available) ──
    crate::boot::with_trace(|trace, paths| {
        if let Some(events) = trace {
            crate::serial_write("[PANIC] Boot trace:\n");
            for event in events {
                crate::serial_write(&alloc::format!("  {:?}\n", event));
            }
        }
        if let Some(p) = paths {
            crate::serial_write("[PANIC] Init paths searched:\n");
            for path in p {
                crate::serial_write(&alloc::format!("  {}\n", path));
            }
        }
    });

    crate::serial_write("========================================\n");
    crate::serial_write("         SYSTEM HALTED\n");
    crate::serial_write("========================================\n");

    loop { crate::arch::CurrentArch::halt(); }
}

/// Dump x86_64 general-purpose and control registers.
#[cfg(target_arch = "x86_64")]
fn dump_registers_x86_64() {
    unsafe {
        let (rax, rbx, rcx, rdx);
        core::arch::asm!("mov {}, rax", out(reg) rax);
        core::arch::asm!("mov {}, rbx", out(reg) rbx);
        core::arch::asm!("mov {}, rcx", out(reg) rcx);
        core::arch::asm!("mov {}, rdx", out(reg) rdx);
        let (rsi, rdi, rbp, rsp);
        core::arch::asm!("mov {}, rsi", out(reg) rsi);
        core::arch::asm!("mov {}, rdi", out(reg) rdi);
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
        let (r8, r9, r10, r11);
        core::arch::asm!("mov {}, r8",  out(reg) r8);
        core::arch::asm!("mov {}, r9",  out(reg) r9);
        core::arch::asm!("mov {}, r10", out(reg) r10);
        core::arch::asm!("mov {}, r11", out(reg) r11);
        let (r12, r13, r14, r15);
        core::arch::asm!("mov {}, r12", out(reg) r12);
        core::arch::asm!("mov {}, r13", out(reg) r13);
        core::arch::asm!("mov {}, r14", out(reg) r14);
        core::arch::asm!("mov {}, r15", out(reg) r15);
        let rip: u64 = unsafe { *rbp.add(1) as u64 };
        let rflags: u64;
        core::arch::asm!("pushfq; pop {}", out(reg) rflags);
        let cr2: u64;
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3);

        crate::serial_write("[PANIC] Registers:\n");
        crate::serial_write(&alloc::format!(
            "  RAX={:016x} RBX={:016x} RCX={:016x} RDX={:016x}\n", rax, rbx, rcx, rdx));
        crate::serial_write(&alloc::format!(
            "  RSI={:016x} RDI={:016x} RBP={:016x} RSP={:016x}\n", rsi, rdi, rbp, rsp));
        crate::serial_write(&alloc::format!(
            "  R8 ={:016x} R9 ={:016x} R10={:016x} R11={:016x}\n", r8, r9, r10, r11));
        crate::serial_write(&alloc::format!(
            "  R12={:016x} R13={:016x} R14={:016x} R15={:016x}\n", r12, r13, r14, r15));
        crate::serial_write(&alloc::format!(
            "  RIP={:016x} RFLAGS={:016x}\n", rip, rflags));
        crate::serial_write(&alloc::format!(
            "  CR2={:016x} (page fault addr)\n", cr2));
        crate::serial_write(&alloc::format!(
            "  CR3={:016x} (page table root)\n", cr3));
    }
}

/// Dump aarch64 general-purpose registers.
#[cfg(target_arch = "aarch64")]
fn dump_registers_aarch64() {
    unsafe {
        let (x0, x1, x2, x3);
        core::arch::asm!("mov {}, x0", out(reg) x0);
        core::arch::asm!("mov {}, x1", out(reg) x1);
        core::arch::asm!("mov {}, x2", out(reg) x2);
        core::arch::asm!("mov {}, x3", out(reg) x3);
        let (x4, x5, x6, x7);
        core::arch::asm!("mov {}, x4", out(reg) x4);
        core::arch::asm!("mov {}, x5", out(reg) x5);
        core::arch::asm!("mov {}, x6", out(reg) x6);
        core::arch::asm!("mov {}, x7", out(reg) x7);
        let (x8, x9, x10, x11);
        core::arch::asm!("mov {}, x8",  out(reg) x8);
        core::arch::asm!("mov {}, x9",  out(reg) x9);
        core::arch::asm!("mov {}, x10", out(reg) x10);
        core::arch::asm!("mov {}, x11", out(reg) x11);
        let (x29, x30);
        core::arch::asm!("mov {}, x29", out(reg) x29);
        core::arch::asm!("mov {}, x30", out(reg) x30);
        let sp: u64;
        core::arch::asm!("mov {}, sp", out(reg) sp);
        let elr_el1: u64;
        core::arch::asm!("mrs {}, elr_el1", out(reg) elr_el1);
        let spsr_el1: u64;
        core::arch::asm!("mrs {}, spsr_el1", out(reg) spsr_el1);
        let esr_el1: u64;
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr_el1);
        let ttbr0_el1: u64;
        core::arch::asm!("mrs {}, ttbr0_el1", out(reg) ttbr0_el1);
        let ttbr1_el1: u64;
        core::arch::asm!("mrs {}, ttbr1_el1", out(reg) ttbr1_el1);

        crate::serial_write("[PANIC] Registers:\n");
        crate::serial_write(&alloc::format!(
            "  X0={:016x} X1={:016x} X2={:016x} X3={:016x}\n", x0, x1, x2, x3));
        crate::serial_write(&alloc::format!(
            "  X4={:016x} X5={:016x} X6={:016x} X7={:016x}\n", x4, x5, x6, x7));
        crate::serial_write(&alloc::format!(
            "  X8={:016x} X9={:016x} X10={:016x} X11={:016x}\n", x8, x9, x10, x11));
        crate::serial_write(&alloc::format!(
            "  X29(FP)={:016x} X30(LR)={:016x}\n", x29, x30));
        crate::serial_write(&alloc::format!(
            "  SP={:016x} ELR_EL1={:016x} (PC)\n", sp, elr_el1));
        crate::serial_write(&alloc::format!(
            "  SPSR_EL1={:016x} ESR_EL1={:016x}\n", spsr_el1, esr_el1));
        crate::serial_write(&alloc::format!(
            "  TTBR0_EL1={:016x} (user) TTBR1_EL1={:016x} (kernel)\n", ttbr0_el1, ttbr1_el1));
    }
}

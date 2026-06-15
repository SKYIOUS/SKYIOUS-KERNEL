//! aarch64 architecture implementation.
//!
//! Provides the `Arch` trait implementation for aarch64.
//! This is the foundation for the ARM64 port of the Vahi kernel.

use super::Arch;

pub struct AArch64Arch;

impl Arch for AArch64Arch {
    unsafe fn init_boot() {
        // Initialize exception vector table
        init_vector_table();
        // Initialize generic timer
        init_timer();
        // Initialize GIC (interrupt controller)
        init_gic();
    }

    unsafe fn init_syscalls() {
        // aarch64 syscall via SVC instruction
        // Set VBAR_EL1, configure SVC exception handler
        crate::serial_write("[ARCH] aarch64 syscalls init (stub)\n");
    }

    fn read_sp() -> u64 {
        let sp: u64;
        unsafe { core::arch::asm!("mov {}, sp", out(reg) sp, options(nostack, preserves_flags)); }
        sp
    }

    fn read_fp() -> u64 {
        let fp: u64;
        unsafe { core::arch::asm!("mov {}, x29", out(reg) fp, options(nostack, preserves_flags)); }
        fp
    }

    fn halt() {
        unsafe { core::arch::asm!("wfi", options(nostack, preserves_flags)); }
    }

    unsafe fn jump_to_usermode(entry: u64, rsp: u64) -> ! {
        // aarch64: ERET to EL0 with given PC and SP
        // Set SPSR_EL1, ELR_EL1, SP_EL0, then ERET
        core::arch::asm!(
            "msr sp_el0, {sp}",
            "msr elr_el1, {entry}",
            "mov x0, 0",
            "msr spsr_el1, x0",
            "eret",
            sp = in(reg) rsp,
            entry = in(reg) entry,
            options(noreturn)
        )
    }

    unsafe fn switch_thread(old_sp: *mut u64, new_sp: u64, new_fs_base: u64) {
        // Save callee-saved registers (x19-x28, fp/x29, lr/x30)
        // Restore from new stack
        // new_fs_base = TPIDR_EL0
        core::arch::asm!(
            // Save context
            "stp x19, x20, [x0, #0]",
            "stp x21, x22, [x0, #16]",
            "stp x23, x24, [x0, #32]",
            "stp x25, x26, [x0, #48]",
            "stp x27, x28, [x0, #64]",
            "stp x29, x30, [x0, #80]",
            // Set SP_EL0 (thread pointer)
            "msr tpidr_el0, {tpidr}",
            // Restore context
            "mov sp, {new_sp}",
            "ldp x19, x20, [sp, #0]",
            "ldp x21, x22, [sp, #16]",
            "ldp x23, x24, [sp, #32]",
            "ldp x25, x26, [sp, #48]",
            "ldp x27, x28, [sp, #64]",
            "ldp x29, x30, [sp, #80]",
            "ret",
            tpidr = in(reg) new_fs_base,
            new_sp = in(reg) new_sp,
            in("x0") old_sp,
            options(noreturn)
        )
    }

    fn read_thread_pointer() -> u64 {
        let tp: u64;
        unsafe { core::arch::asm!("mrs {}, tpidr_el0", out(reg) tp, options(nostack, preserves_flags)); }
        tp
    }

    unsafe fn write_thread_pointer(val: u64) {
        core::arch::asm!("msr tpidr_el0, {}", in(reg) val, options(nostack, preserves_flags));
    }
}

/// Initialize the exception vector table (VBAR_EL1).
unsafe fn init_vector_table() {
    extern "C" {
        static exception_vector_table: [u8; 0x800];
    }
    core::arch::asm!("msr vbar_el1, {}", in(reg) (&exception_vector_table as *const _ as u64));
}

/// Initialize the ARM generic timer as the system timer.
unsafe fn init_timer() {
    // Set timer frequency (CNTFRQ_EL0) - typically read from hardware
    // Enable timer interrupt (CNTP_CTL_EL0.ENABLE = 1)
    crate::serial_write("[ARCH] aarch64 timer init (stub)\n");
}

/// Initialize the Generic Interrupt Controller (GICv2/GICv3).
unsafe fn init_gic() {
    // GICv2: MMIO at 0x0800_0000 (GICD) and 0x0801_0000 (GICC) for QEMU virt
    // Configure CPU interface, enable distributor
    crate::serial_write("[ARCH] aarch64 GIC init (stub)\n");
}

/// Early boot entry point for aarch64.
/// Called from the bootloader or vector table reset handler.
#[no_mangle]
pub extern "C" fn _start_aarch64() -> ! {
    // 1. Set up stack pointer
    // 2. Clear BSS
    // 3. Set up exception vectors
    // 4. Initialize MMU (page tables)
    // 5. Call Rust main
    crate::serial_write("[ARCH] aarch64 _start_aarch64 entry\n");

    // Zero BSS
    extern "C" {
        static mut __bss_start: u64;
        static mut __bss_end: u64;
    }
    let bss_start = &raw mut __bss_start as u64;
    let bss_end = &raw mut __bss_end as u64;
    if bss_end > bss_start {
        core::ptr::write_bytes(bss_start as *mut u8, 0, (bss_end - bss_start) as usize);
    }

    // For now, halt
    loop {
        unsafe { core::arch::asm!("wfi", options(nostack, preserves_flags)); }
    }
}

/// Exception vector table (aligned to 0x800).
#[link_section = ".text._vector_table"]
#[no_mangle]
#[used]
pub static exception_vector_table: [u8; 0x800] = [0; 0x800];

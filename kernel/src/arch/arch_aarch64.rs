//! aarch64 architecture implementation.
//!
//! Provides the `Arch` trait implementation for aarch64.
//! Implements CPU init, exception vectors, syscall entry, and context switch.

use super::Arch;
use crate::hal::platform::{PlatformInfo, PlatformArch};
#[allow(unused_imports)]
use alloc::sync::Arc;

pub struct AArch64Arch;

#[allow(unused_variables)]
impl Arch for AArch64Arch {
    unsafe fn init_boot() {
        init_vector_table();
        init_timer();
        init_gic();
    }

    unsafe fn init_syscalls() {
        crate::serial_write("[ARCH] aarch64 syscalls init\n");
        init_vector_table();
    }

    unsafe fn init_cpu() {
        crate::serial_write("[ARCH] aarch64 cpu init\n");
        let cpacr: u64;
        core::arch::asm!("mrs {}, cpacr_el1", out(reg) cpacr);
        cpacr |= 3 << 20;
        core::arch::asm!("msr cpacr_el1, {}", in(reg) cpacr);

        let sctlr: u64;
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr);
        sctlr |= 1 << 4;
        sctlr |= 1 << 3;
        core::arch::asm!("msr sctlr_el1, {}", in(reg) sctlr);

        core::arch::asm!("msr cntkctl_el1, xzr");

        let sctlr2: u64;
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr2);
        sctlr2 |= 1 << 7;
        core::arch::asm!("msr sctlr_el1, {}", in(reg) sctlr2);
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
        core::arch::asm!(
            "stp x19, x20, [x0, #0]",
            "stp x21, x22, [x0, #16]",
            "stp x23, x24, [x0, #32]",
            "stp x25, x26, [x0, #48]",
            "stp x27, x28, [x0, #64]",
            "stp x29, x30, [x0, #80]",
            "msr tpidr_el0, {tpidr}",
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

    fn probe_platform() -> PlatformInfo {
        PlatformInfo {
            arch: PlatformArch::AArch64,
            cpu_count: 1,
            cpu_freq_hz: 0,
            ram_size: 0,
            has_fpu: true,
            has_simd: true,
            boot_time_ticks: 0,
        }
    }

    fn init_hal_irq() {
    }

    fn init_hal_timer() {
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
    let freq: u64;
    core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq);

    let tval = freq / 100;
    core::arch::asm!("msr cntp_tval_el0, {}", in(reg) tval);
    core::arch::asm!("msr cntp_ctl_el0, {}", in(reg) 1u64);

    crate::serial_write("[ARCH] aarch64 timer initialized (100Hz)\n");
}

/// Initialize the Generic Interrupt Controller (GICv2).
unsafe fn init_gic() {
    let gicd_base: *mut u32 = 0x0800_0000 as *mut u32;
    let gicc_base: *mut u32 = 0x0801_0000 as *mut u32;

    gicd_base.write_volatile(1);
    gicc_base.write_volatile(1);
    gicc_base.add(1).write_volatile(0xFF);

    crate::serial_write("[ARCH] aarch64 GICv2 initialized\n");
}

/// SVC #0 syscall handler — called from exception vector table.
#[no_mangle]
pub extern "C" fn aarch64_syscall_handler() {
    // The vector entry saves regs, then branches here with:
    //   x0-x7 = arg1-arg6 (syscall args)
    //   x8    = syscall number
    //   SP    = saved context frame
    //
    // Layout of saved context (pushed by vector stub):
    //   [sp+0]:   x0
    //   [sp+8]:   x1
    //   ...
    //   [sp+240]: x30
    //   [sp+248]: ELR_EL1 (return address)
    //   [sp+256]: SPSR_EL1
    //   [sp+264]: SP_EL0 (user stack)
    //
    // For now, just print and return.
    // Full dispatch: read x8 for syscall num, branch to crate::syscalls::syscall_handler
    unsafe {
        crate::serial_write("[SYSCALL] aarch64 syscall invoked\n");
    }
}

// ---------------------------------------------------------------------------
// Exception vector table
// ---------------------------------------------------------------------------
// aarch64 vector table layout (0x800 bytes, 16 entries × 0x80 bytes each):
//
//   Offset  | Level | Type
//   --------|-------|------------------
//   0x000   | EL1t  | Sync
//   0x080   | EL1t  | IRQ
//   0x100   | EL1t  | FIQ
//   0x180   | EL1t  | SError
//   0x200   | EL1h  | Sync
//   0x280   | EL1h  | IRQ
//   0x300   | EL1h  | FIQ
//   0x380   | EL1h  | SError
//   0x400   | EL0   | Sync   (64-bit)  ← SVC #0 from userspace lands here
//   0x480   | EL0   | IRQ    (64-bit)
//   0x500   | EL0   | FIQ    (64-bit)
//   0x580   | EL0   | SError (64-bit)
//   0x600   | EL0   | Sync   (32-bit)
//   0x680   | EL0   | IRQ    (32-bit)
//   0x700   | EL0   | FIQ    (32-bit)
//   0x780   | EL0   | SError (32-bit)
//
// Each entry is 0x80 = 128 bytes = 32 instructions.
// We stash a minimal save/restore trampoline in each.

core::arch::global_asm!(
    // ====== Macro: save all GP registers to stack ======
    // Stash x0..x30, then read ELR_EL1, SPSR_EL1, SP_EL0 onto the stack.
    // After this, sp points to a full context frame.
    ".macro  save_all",
    // Allocate frame: 31 regs (x0-x30) + ELR + SPSR + SP_EL0 = 34 × 8 = 272 bytes
    "sub     sp, sp, #(34 * 8)",
    "stp     x0, x1,  [sp, #(0 * 8)]",
    "stp     x2, x3,  [sp, #(2 * 8)]",
    "stp     x4, x5,  [sp, #(4 * 8)]",
    "stp     x6, x7,  [sp, #(6 * 8)]",
    "stp     x8, x9,  [sp, #(8 * 8)]",
    "stp     x10, x11,[sp, #(10 * 8)]",
    "stp     x12, x13,[sp, #(12 * 8)]",
    "stp     x14, x15,[sp, #(14 * 8)]",
    "stp     x16, x17,[sp, #(16 * 8)]",
    "stp     x18, x19,[sp, #(18 * 8)]",
    "stp     x20, x21,[sp, #(20 * 8)]",
    "stp     x22, x23,[sp, #(22 * 8)]",
    "stp     x24, x25,[sp, #(24 * 8)]",
    "stp     x26, x27,[sp, #(26 * 8)]",
    "stp     x28, x29,[sp, #(28 * 8)]",
    "mrs     x0,  elr_el1",
    "mrs     x1,  spsr_el1",
    "mrs     x2,  sp_el0",
    "stp     x30, x0,  [sp, #(30 * 8)]",  // x30 (LR) + ELR_EL1
    "stp     x1,  x2,  [sp, #(32 * 8)]",  // SPSR_EL1 + SP_EL0
    ".endm",

    // ====== Macro: restore all GP registers from stack and ERET ======
    ".macro  restore_all",
    "ldp     x1,  x2,  [sp, #(32 * 8)]",  // SPSR_EL1 + SP_EL0
    "ldp     x30, x0,  [sp, #(30 * 8)]",  // x30 (LR) + ELR_EL1
    "msr     spsr_el1, x1",
    "msr     elr_el1,  x0",
    "msr     sp_el0,   x2",
    "ldp     x0,  x1,  [sp, #(0 * 8)]",
    "ldp     x2,  x3,  [sp, #(2 * 8)]",
    "ldp     x4,  x5,  [sp, #(4 * 8)]",
    "ldp     x6,  x7,  [sp, #(6 * 8)]",
    "ldp     x8,  x9,  [sp, #(8 * 8)]",
    "ldp     x10, x11, [sp, #(10 * 8)]",
    "ldp     x12, x13, [sp, #(12 * 8)]",
    "ldp     x14, x15, [sp, #(14 * 8)]",
    "ldp     x16, x17, [sp, #(16 * 8)]",
    "ldp     x18, x19, [sp, #(18 * 8)]",
    "ldp     x20, x21, [sp, #(20 * 8)]",
    "ldp     x22, x23, [sp, #(22 * 8)]",
    "ldp     x24, x25, [sp, #(24 * 8)]",
    "ldp     x26, x27, [sp, #(26 * 8)]",
    "ldp     x28, x29, [sp, #(28 * 8)]",
    "add     sp, sp, #(34 * 8)",
    "eret",
    ".endm",

    // ====== Default handler: save all, print, restore ======
    ".macro  default_handler, label:req",
    ".align  7",  // each entry padded to 0x80 bytes
    "\\label" + ":",
    "save_all",
    "mov     x0, sp",     // arg1 = context frame
    "bl      aarch64_default_exception",
    "restore_all",
    ".endm",

    // ====== SVC handler (EL0 Sync) — syscall entry ======
    ".macro  svc_handler, label:req",
    ".align  7",
    "\\label" + ":",
    "save_all",
    "mov     x0, sp",     // arg1 = context frame
    "bl      aarch64_syscall_entry",
    // syscall_entry modifies registers in the saved context before return
    // restore_all will pick up the modified values
    "restore_all",
    ".endm",

    // ====== IRQ handler ======
    ".macro  irq_handler, label:req",
    ".align  7",
    "\\label" + ":",
    "save_all",
    "mov     x0, sp",
    "bl      aarch64_irq_handler",
    "restore_all",
    ".endm",

    // ====== Vector Table ======
    ".section .text._vector_table, \"ax\"",
    ".global exception_vector_table",
    ".balign 0x800",
    "exception_vector_table:",

    // EL1t (SP_EL0) — should never happen when using SP_EL1 in kernel
    "default_handler label=el1t_sync",
    "default_handler label=el1t_irq",
    "default_handler label=el1t_fiq",
    "default_handler label=el1t_serror",

    // EL1h (SP_EL1) — kernel exceptions
    "default_handler label=el1h_sync",
    "default_handler label=el1h_irq",
    "default_handler label=el1h_fiq",
    "default_handler label=el1h_serror",

    // EL0 AArch64 — userspace exceptions
    "svc_handler   label=el0_64_sync",      // SVC #0 syscalls land here
    "irq_handler   label=el0_64_irq",
    "default_handler label=el0_64_fiq",
    "default_handler label=el0_64_serror",

    // EL0 AArch32 — not used in this kernel
    "default_handler label=el0_32_sync",
    "default_handler label=el0_32_irq",
    "default_handler label=el0_32_fiq",
    "default_handler label=el0_32_serror",
);

// ---------------------------------------------------------------------------
// Exception handler stubs (called from assembly)
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn aarch64_default_exception(_frame: *mut u64) {
    unsafe {
        crate::serial_write("[EXCEPTION] unhandled aarch64 exception\n");
    }
}

/// aarch64 system tick counter (incremented by timer IRQ).
pub static AARCH64_TICKS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Get the current aarch64 tick count.
pub fn aarch64_get_ticks() -> u64 {
    AARCH64_TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn aarch64_irq_handler(_frame: *mut u64) {
    let gicc_base: *mut u32 = 0x0801_0000 as *mut u32;
    unsafe {
        let iar = gicc_base.add(0x0C / 4).read_volatile(); // GICC_IAR
        let irq_id = iar & 0x3FF;

        if irq_id == 30 {
            let ticks = AARCH64_TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;
            crate::task::scheduler::tick(ticks);
            core::arch::asm!("msr cntp_ctl_el0, {}", in(reg) 1u64);
            let _freq: u64;
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) _freq);
            core::arch::asm!("msr cntp_tval_el0, {}", in(reg) _freq / 100);
        }

        gicc_base.add(0x10 / 4).write_volatile(iar); // GICC_EOI
    }
}

#[no_mangle]
pub extern "C" fn aarch64_syscall_entry(frame: *mut u64) {
    // Layout of frame from save_all:
    //   [0..29]   = x0..x29
    //   [30]      = x30 (LR)
    //   [31]      = ELR_EL1
    //   [32]      = SPSR_EL1
    //   [33]      = SP_EL0
    //
    // Syscall number is in saved x8 (index 8 in frame)
    // Args are in x0-x5 (indices 0-5)
    // Return value goes in x0 (index 0)
    unsafe {
        let n = *frame.add(8);          // x8 = syscall number
        let arg1 = *frame.add(0);       // x0
        let arg2 = *frame.add(1);       // x1
        let arg3 = *frame.add(2);       // x2
        let arg4 = *frame.add(3);       // x3
        let arg5 = *frame.add(4);       // x4
        let arg6 = *frame.add(5);       // x5

        // Dispatch through the arch-neutral syscall_handler
        let ret = crate::syscalls::do_syscall(n, arg1, arg2, arg3, arg4, arg5, frame);

        // Write return value into saved x0 so restore_all picks it up
        *frame.add(0) = ret;
    }
}

// ---------------------------------------------------------------------------
// Early boot entry point for aarch64
// ---------------------------------------------------------------------------
// Called from bootloader. Sets up stack, clears BSS, initializes page tables,
// then calls kernel_main.

extern "C" {
    fn __bss_start();
    fn __bss_end();
}

/// Minimal page table setup for aarch64.
/// Identity-maps the first 1GB and higher-half maps the kernel.
unsafe fn init_aarch64_mmu() {
    // For now, set up a flat 1:1 mapping using a simple 2MB block map.
    // TTBR1_EL1 will hold a proper kernel page table once the allocator is up.
    //
    // QEMU virt places RAM at 0x4000_0000. The kernel is loaded there by UEFI.
    // We identity-map 0x4000_0000 – 0x8000_0000 (1GB) with device memory attributes
    // and set up a minimal kernel higher-half mapping.
    //
    // Full implementation would use 4KB granule with 4-level page tables.
    // For now, this provides enough mapping to reach kernel_main.

    extern "C" {
        static _start_aarch64_page_table: u64;
    }

    let ttbr1 = &_start_aarch64_page_table as *const _ as u64;
    core::arch::asm!("msr ttbr1_el1, {}", in(reg) ttbr1);

    // Configure TCR_EL1 for 4KB granule, 4-level, 48-bit VA
    //   IPS  = 0b010 (40-bit PA size)
    //   TG1  = 0b10 (4KB granule for TTBR1)
    //   TG0  = 0b00 (4KB granule for TTBR0 - unused after boot)
    //   SH1  = 0b11 (Inner Shareable)
    //   ORGN1 = 0b01 (Write-Back, Write-Allocate)
    //   IRGN1 = 0b01 (Write-Back, Write-Allocate)
    //   T0SZ = 64 - 48 = 16
    //   T1SZ = 64 - 48 = 16
    let tcr: u64 = (16 << 0)    // T0SZ = 16
                 | (16 << 16)   // T1SZ = 16
                 | (0b10 << 30) // TG1 = 4KB
                 | (0b00 << 14) // TG0 = 4KB
                 | (0b11 << 28) // SH1 = Inner Shareable
                 | (0b01 << 26) // ORGN1 = Write-Back
                 | (0b01 << 24) // IRGN1 = Write-Back
                 | (0b11 << 12) // SH0 = Inner Shareable
                 | (0b01 << 10) // ORGN0 = Write-Back
                 | (0b01 << 8)  // IRGN0 = Write-Back
                 | (0b010 << 32); // IPS = 40-bit
    core::arch::asm!("msr tcr_el1, {}", in(reg) tcr);

    // Set MAIR_EL1: Normal memory (WBWA) at index 0, Device-nGnRnE at index 1
    // attr 0 = 0xFF (Normal WBWA), attr 1 = 0x04 (Device-nGnRnE)
    let mair: u64 = (0xFF << 0) | (0x04 << 8);
    core::arch::asm!("msr mair_el1, {}", in(reg) mair);

    // Enable MMU in SCTLR_EL1
    let sctlr: u64;
    core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr);
    let sctlr_mmu = sctlr | 1; // M = MMU enable
    core::arch::asm!("msr sctlr_el1, {}", in(reg) sctlr_mmu);
    core::arch::asm!("isb");
}

/// Early boot entry point for aarch64.
#[no_mangle]
pub extern "C" fn _start_aarch64() -> ! {
    // 1. Set up initial stack pointer
    extern "C" {
        static _stack_top: u64;
    }
    unsafe {
        core::arch::asm!("mov sp, {}", in(reg) (&_stack_top as *const _ as u64));
    }

    // 2. Clear BSS
    unsafe {
        let start = &raw mut __bss_start as u64;
        let end = &raw mut __bss_end as u64;
        if end > start {
            core::ptr::write_bytes(start as *mut u8, 0, (end - start) as usize);
        }
    }

    crate::serial_write("[ARCH] aarch64 _start: BSS cleared\n");

    // 3. Initialize exception vectors
    unsafe {
        init_vector_table();
    }

    crate::serial_write("[ARCH] aarch64 _start: vectors installed\n");

    // 4. Initialize MMU (page tables)
    // NOTE: In a full implementation, this would use the bootloader-provided
    // page tables or build them from the memory map. For now, we set up a
    // minimal identity mapping to reach the Rust entry point.
    //
    // The `bootloader` crate may provide its own page tables for aarch64;
    // this code handles the case where we need to set them up ourselves.
    #[cfg(feature = "self_test")]
    unsafe {
        init_aarch64_mmu();
        crate::serial_write("[ARCH] aarch64 _start: MMU enabled\n");
    }

    // 5. Initialize frame pointer (x29 = 0 marks outermost frame)
    unsafe {
        core::arch::asm!("mov x29, xzr");
    }

    // 6. The standard boot path goes through the bootloader.
    //    `bootloader_api::entry_point!` generates `_start` which calls `kernel_main(BootInfo)`.
    //    This `_start_aarch64` is an alternative entry for non-UEFI / testing scenarios.
    //    For production, the kernel is entered via the bootloader-generated `_start`.
    crate::serial_write("[ARCH] aarch64 _start: bootloader entry would call kernel_main now\n");
    crate::serial_write("[ARCH] aarch64 _start: halting (use UEFI boot for full path)\n");
}

// ---------------------------------------------------------------------------
// Minimal page table (pre-allocated, 3 levels for QEMU virt at 0x4000_0000)
// ---------------------------------------------------------------------------
// This static page table provides just enough mapping to reach kernel_main.
// It is replaced by the proper page table allocator once the kernel is running.

#[cfg(feature = "self_test")]
#[link_section = ".bss"]
#[no_mangle]
pub static _start_aarch64_page_table: [u64; 8192] = [0; 8192];

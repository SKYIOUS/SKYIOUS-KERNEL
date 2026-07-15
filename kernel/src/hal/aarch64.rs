use crate::hal::cpu::{CpuContext, CpuId};
use crate::hal::irq::{InterruptController, IrqVector};

pub struct AArch64InterruptController;

impl InterruptController for AArch64InterruptController {
    fn eoi(&self, _vector: IrqVector) {
    }

    fn mask_irq(&self, _irq: u8, _masked: bool) {
    }

    fn route_pci_irq(&self, _bus: u8, _device: u8, _pin: u8, _vector: IrqVector) {
    }

    fn controller_id(&self) -> u32 {
        0
    }

    unsafe fn enable_cpu(&self) {
        core::arch::asm!("msr daifclr, #2");
    }
}

pub struct AArch64CpuContext;

impl CpuContext for AArch64CpuContext {
    fn halt(&self) {
        unsafe { core::arch::asm!("wfi"); }
    }

    fn read_sp(&self) -> u64 {
        let sp: u64;
        unsafe { core::arch::asm!("mov {}, sp", out(reg) sp); }
        sp
    }

    fn read_fp(&self) -> u64 {
        let fp: u64;
        unsafe { core::arch::asm!("mov {}, x29", out(reg) fp); }
        fp
    }

    unsafe fn jump_to_usermode(&self, entry: u64, rsp: u64) -> ! {
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

    unsafe fn switch_thread(&self, old_sp: *mut u64, new_sp: u64, new_tpidr: u64) {
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
            tpidr = in(reg) new_tpidr,
            new_sp = in(reg) new_sp,
            in("x0") old_sp,
            options(noreturn)
        )
    }

    fn read_thread_pointer(&self) -> u64 {
        let tp: u64;
        unsafe { core::arch::asm!("mrs {}, tpidr_el0", out(reg) tp); }
        tp
    }

    unsafe fn write_thread_pointer(&self, val: u64) {
        core::arch::asm!("msr tpidr_el0, {}", in(reg) val);
    }

    fn current_cpu_id(&self) -> CpuId {
        let id: u64;
        unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) id); }
        id & 0xFF
    }

    fn cpu_count(&self) -> usize {
        crate::hal::cpu::CPU_COUNT.load(core::sync::atomic::Ordering::Relaxed) as usize
    }
}

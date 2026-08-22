use crate::hal::cpu::{CpuContext, CpuId};
use crate::hal::irq::{InterruptController, IrqVector};

pub struct X86InterruptController;

impl InterruptController for X86InterruptController {
    fn eoi(&self, _vector: IrqVector) {
        crate::apic::eoi();
    }

    fn mask_irq(&self, _irq: u8, _masked: bool) {
    }

    fn route_pci_irq(&self, _bus: u8, _device: u8, pin: u8, vector: IrqVector) {
        crate::apic::route_pci_irq(pin, vector);
    }

    fn controller_id(&self) -> u32 {
        crate::apic::current_lapic_id() as u32
    }

    unsafe fn enable_cpu(&self) {
        x86_64::instructions::interrupts::enable();
    }
}

pub struct X86CpuContext;

impl CpuContext for X86CpuContext {
    fn halt(&self) {
        x86_64::instructions::hlt();
    }

    fn read_sp(&self) -> u64 {
        let sp: u64;
        unsafe { core::arch::asm!("mov {}, rsp", out(reg) sp, options(nostack, preserves_flags)); }
        sp
    }

    fn read_fp(&self) -> u64 {
        let fp: u64;
        unsafe { core::arch::asm!("mov {}, rbp", out(reg) fp, options(nostack, preserves_flags)); }
        fp
    }

    unsafe fn jump_to_usermode(&self, entry: u64, rsp: u64) -> ! {
        // SAFETY: caller guarantees valid userspace entry and stack
        crate::task::thread::jump_to_usermode(entry, rsp)
    }

    unsafe fn switch_thread(&self, old_sp: *mut u64, new_sp: u64, new_fs_base: u64) {
        // SAFETY: caller guarantees valid saved and new stack pointers
        crate::task::thread::switch_thread(old_sp, new_sp, new_fs_base)
    }

    fn read_thread_pointer(&self) -> u64 {
        crate::task::thread::read_fs_base()
    }

    unsafe fn write_thread_pointer(&self, val: u64) {
        // SAFETY: caller ensures FS/GS base is writable
        crate::task::thread::write_fs_base(val)
    }

    fn current_cpu_id(&self) -> CpuId {
        crate::apic::current_lapic_id() as u64
    }

    fn cpu_count(&self) -> usize {
        crate::hal::cpu::CPU_COUNT.load(core::sync::atomic::Ordering::Relaxed) as usize
    }
}


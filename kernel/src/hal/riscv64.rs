use crate::hal::cpu::{CpuContext, CpuId};
use crate::hal::irq::{InterruptController, IrqVector};

pub struct RiscV64InterruptController;

impl InterruptController for RiscV64InterruptController {
    fn eoi(&self, _vector: IrqVector) {
        unimplemented!("RISC-V not yet supported")
    }

    fn mask_irq(&self, _irq: u8, _masked: bool) {
        unimplemented!("RISC-V not yet supported")
    }

    fn route_pci_irq(&self, _bus: u8, _device: u8, _pin: u8, _vector: IrqVector) {
        unimplemented!("RISC-V not yet supported")
    }

    fn controller_id(&self) -> u32 {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn enable_cpu(&self) {
        unimplemented!("RISC-V not yet supported")
    }
}

pub struct RiscV64CpuContext;

impl CpuContext for RiscV64CpuContext {
    fn halt(&self) {
        unimplemented!("RISC-V not yet supported")
    }

    fn read_sp(&self) -> u64 {
        unimplemented!("RISC-V not yet supported")
    }

    fn read_fp(&self) -> u64 {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn jump_to_usermode(&self, _entry: u64, _rsp: u64) -> ! {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn switch_thread(&self, _old_sp: *mut u64, _new_sp: u64, _new_tp: u64) {
        unimplemented!("RISC-V not yet supported")
    }

    fn read_thread_pointer(&self) -> u64 {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn write_thread_pointer(&self, _val: u64) {
        unimplemented!("RISC-V not yet supported")
    }

    fn current_cpu_id(&self) -> CpuId {
        unimplemented!("RISC-V not yet supported")
    }

    fn cpu_count(&self) -> usize {
        unimplemented!("RISC-V not yet supported")
    }
}

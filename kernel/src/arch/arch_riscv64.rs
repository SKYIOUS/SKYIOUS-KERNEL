use super::Arch;
use crate::hal::platform::PlatformInfo;

pub struct RiscV64Arch;

#[allow(unused_variables)]
impl Arch for RiscV64Arch {
    unsafe fn init_boot() {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn init_syscalls() {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn init_cpu() {
        unimplemented!("RISC-V not yet supported")
    }

    fn read_sp() -> u64 {
        unimplemented!("RISC-V not yet supported")
    }

    fn read_fp() -> u64 {
        unimplemented!("RISC-V not yet supported")
    }

    fn halt() {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn jump_to_usermode(entry: u64, rsp: u64) -> ! {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn switch_thread(old_sp: *mut u64, new_sp: u64, new_fs_base: u64) {
        unimplemented!("RISC-V not yet supported")
    }

    fn read_thread_pointer() -> u64 {
        unimplemented!("RISC-V not yet supported")
    }

    unsafe fn write_thread_pointer(val: u64) {
        unimplemented!("RISC-V not yet supported")
    }

    fn probe_platform() -> PlatformInfo {
        unimplemented!("RISC-V not yet supported")
    }

    fn init_hal_irq() {
        unimplemented!("RISC-V not yet supported")
    }

    fn init_hal_timer() {
        unimplemented!("RISC-V not yet supported")
    }
}

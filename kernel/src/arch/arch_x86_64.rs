//! x86_64 architecture implementation.
//!
//! Delegates to the existing x86_64-specific modules (gdt, interrupts, task::thread, etc.).

use super::Arch;

pub struct X86_64Arch;

impl Arch for X86_64Arch {
    unsafe fn init_boot() {
        crate::gdt::init();
        crate::interrupts::init_idt();
        unsafe { crate::interrupts::PICS.lock().initialize() };
        crate::syscalls::init();
        crate::apic::init();
    }

    unsafe fn init_syscalls() {
        crate::syscalls::init();
    }

    fn read_sp() -> u64 {
        let sp: u64;
        unsafe { core::arch::asm!("mov {}, rsp", out(reg) sp, options(nostack, preserves_flags)); }
        sp
    }

    fn read_fp() -> u64 {
        let fp: u64;
        unsafe { core::arch::asm!("mov {}, rbp", out(reg) fp, options(nostack, preserves_flags)); }
        fp
    }

    fn halt() {
        x86_64::instructions::hlt();
    }

    unsafe fn jump_to_usermode(entry: u64, rsp: u64) -> ! {
        crate::task::thread::jump_to_usermode(entry, rsp)
    }

    unsafe fn switch_thread(old_sp: *mut u64, new_sp: u64, new_fs_base: u64) {
        crate::task::thread::switch_thread(old_sp, new_sp, new_fs_base)
    }

    fn read_thread_pointer() -> u64 {
        crate::task::thread::read_fs_base()
    }

    unsafe fn write_thread_pointer(val: u64) {
        crate::task::thread::write_fs_base(val)
    }
}

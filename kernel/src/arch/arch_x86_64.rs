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

    unsafe fn init_cpu() {
        use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
        use core::sync::atomic::Ordering;

        // 1. Enable SSE in CR0
        Cr0::update(|flags| {
            flags.remove(Cr0Flags::EMULATE_COPROCESSOR);
            flags.insert(Cr0Flags::MONITOR_COPROCESSOR);
            flags.insert(Cr0Flags::NUMERIC_ERROR);
        });

        // 2. Query leaf 7 for FSGSBASE, SMEP, UMIP
        let mut ebx7: u32 = 0;
        let mut ecx7: u32 = 0;
        core::arch::asm!(
            "push rbx",
            "mov eax, 7",
            "xor ecx, ecx",
            "cpuid",
            "mov {0:e}, ebx",
            "mov {1:e}, ecx",
            "pop rbx",
            out(reg) ebx7, out(reg) ecx7,
            out("eax") _, out("edx") _,
            options(nostack, preserves_flags));

        Cr4::update(|flags| {
            flags.insert(Cr4Flags::OSFXSR);
            flags.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
            if ebx7 & 1 != 0 {
                flags.insert(Cr4Flags::FSGSBASE);
                crate::task::thread::HAS_FSGSBASE.store(true, Ordering::SeqCst);
            }
            // SMEP (bit 20): EBX bit 7
            if ebx7 & (1 << 7) != 0 {
                flags.insert(Cr4Flags::from_bits_truncate(0x100000));
            }
            // UMIP (bit 11): ECX bit 2
            if ecx7 & (1 << 2) != 0 {
                flags.insert(Cr4Flags::from_bits_truncate(0x800));
            }
        });
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

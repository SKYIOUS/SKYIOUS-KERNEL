//! x86_64 architecture implementation.
//!
//! Delegates to the existing x86_64-specific modules (gdt, interrupts, task::thread, etc.).

use super::Arch;
use crate::hal::platform::{PlatformInfo, PlatformArch};
use alloc::sync::Arc;

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

        Cr0::update(|flags| {
            flags.remove(Cr0Flags::EMULATE_COPROCESSOR);
            flags.insert(Cr0Flags::MONITOR_COPROCESSOR);
            flags.insert(Cr0Flags::NUMERIC_ERROR);
        });

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
            if ebx7 & (1 << 7) != 0 {
                flags.insert(Cr4Flags::from_bits_truncate(0x100000));
            }
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

    fn probe_platform() -> PlatformInfo {
        let cpu_count = if let Some(ids) = crate::acpi::AP_LAPIC_IDS.get() {
            ids.len() + 1
        } else {
            1
        };

        // SAFETY: CPUID is always available on x86_64
        let (has_fpu, has_simd, cpu_freq_hz) = unsafe {
            let mut eax1: u32;
            let mut edx1: u32;
            core::arch::asm!(
                "push rbx",
                "mov eax, 1",
                "cpuid",
                "mov {0:e}, ebx",
                "pop rbx",
                out(reg) _,
                lateout("ecx") _,
                lateout("edx") edx1,
                lateout("eax") eax1,
                options(nostack, preserves_flags)
            );

            let fpu = (edx1 & 1) != 0;
            let simd = (edx1 & (1 << 26)) != 0;

            let freq = if eax1 >= 0x16 {
                let mut ecx16: u32;
                core::arch::asm!(
                    "push rbx",
                    "mov eax, 0x16",
                    "cpuid",
                    "pop rbx",
                    lateout("ecx") ecx16,
                    lateout("edx") _,
                    lateout("eax") _,
                    options(nostack, preserves_flags)
                );
                (ecx16 as u64) * 1_000_000
            } else {
                0
            };

            (fpu, simd, freq)
        };

        let boot_ticks = {
            let lo: u32; let hi: u32;
            unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
            ((hi as u64) << 32) | lo as u64
        };

        PlatformInfo {
            arch: PlatformArch::X86_64,
            cpu_count,
            cpu_freq_hz,
            ram_size: 0,
            has_fpu,
            has_simd,
            boot_time_ticks: boot_ticks,
        }
    }

    fn init_hal_irq() {
        let ctrl = Arc::new(X86IrqController);
        crate::hal::irq::register_controller(ctrl);
    }

    fn init_hal_timer() {
        let pf = crate::hal::platform::get();
        let tsc_timer = crate::hal::timer::TscTimer::new();
        if pf.cpu_freq_hz > 0 {
            tsc_timer.init(pf.cpu_freq_hz);
        }
        crate::hal::timer::register_timer(Arc::new(tsc_timer));
    }
}

struct X86IrqController;

impl crate::hal::irq::InterruptController for X86IrqController {
    fn eoi(&self, _vector: crate::hal::irq::IrqVector) {
        crate::apic::eoi();
    }

    fn mask_irq(&self, _irq: u8, _masked: bool) {
    }

    fn route_pci_irq(&self, _bus: u8, _device: u8, pin: u8, vector: crate::hal::irq::IrqVector) {
        crate::apic::route_pci_irq(pin, vector);
    }

    fn controller_id(&self) -> u32 {
        crate::apic::current_lapic_id() as u32
    }

    unsafe fn enable_cpu(&self) {
        x86_64::instructions::interrupts::enable();
    }
}

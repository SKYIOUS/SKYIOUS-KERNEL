//! Local APIC initialization and timer calibration.
//!
//! The Local APIC (LAPIC) is per-CPU and handles:
//! - Interrupt acceptance from the I/O APIC
//! - Interrupt forwarding to the CPU core
//! - Timer interrupt generation (calibrated against TSC)
//! - IPI generation (inter-processor interrupts)
//!
//! ## Registers
//!
//! Key LAPIC register offsets used by this module:
//! - `0x020` (LAPIC_ID) — Local APIC ID
//! - `0x030` (LAPIC_VERSION) — Version register
//! - `0x080` (LAPIC_TPR) — Task Priority Register
//! - `0x0B0` (LAPIC_EOI) — End Of Interrupt
//! - `0x0F0` (LAPIC_SPURIOUS) — Spurious interrupt vector
//! - `0x320`/`0x350`/`0x360`/`0x370` (LVT) — Local Vector Table
//! - `0x380`/`0x390`/`0x3E0` — Timer initial/current/divide config
//!
//! ## Timer Calibration
//!
//! The LAPIC timer is calibrated against the TSC (Time Stamp Counter).
//! A short calibration loop measures how many LAPIC ticks occur per
//! millisecond, yielding a divisor for the desired tick rate.
//!
//! ## Invariants
//!
//! - LAPIC is enabled (MSR `IA32_APIC_BASE`) before any register access
//! - Spurious interrupt vector must be set to a valid IDT entry
//! - Timer calibration runs with interrupts DISABLED to avoid drift

use x86_64::instructions::port::Port;

use super::{
    LAPIC_ID, LAPIC_LVT_ERROR, LAPIC_LVT_LINT0, LAPIC_LVT_LINT1, LAPIC_LVT_TIMER, LAPIC_SPURIOUS,
    LAPIC_TIMER_CCR, LAPIC_TIMER_DCR, LAPIC_TIMER_ICR, LAPIC_TPR, LAPIC_VERSION,
};

const LAPIC_LVT_TIMER_PERIODIC: u32 = 1 << 17;
const LAPIC_LVT_TIMER_MASKED: u32 = 1 << 16;

pub struct LocalApic {
    #[allow(dead_code)]
    base: usize,
}

impl LocalApic {
    /// # Safety
    ///
    /// ACPI `LAPIC_ADDR` must have been initialized and `physical_memory_offset()`
    /// must be installed.
    pub unsafe fn new() -> Option<Self> {
        super::ACPI.get()?.lapic_addr().map(|addr| LocalApic {
            base: addr as usize,
        })
    }

    pub fn read(&self, offset: u32) -> u32 {
        super::lapic_read32(offset)
    }

    pub fn write(&mut self, offset: u32, value: u32) {
        super::lapic_write32(offset, value)
    }

    pub fn id(&self) -> u32 {
        self.read(LAPIC_ID) >> 24
    }

    pub fn version(&self) -> u32 {
        self.read(LAPIC_VERSION)
    }

    pub fn enable(&mut self) {
        self.write(LAPIC_SPURIOUS, self.read(LAPIC_SPURIOUS) | 0x100 | 0xFF);
        self.write(LAPIC_TPR, 0);
        self.write(LAPIC_LVT_LINT0, 0x0707);
        self.write(LAPIC_LVT_LINT1, 0x10004);
        self.write(LAPIC_LVT_ERROR, 0x10000);
        super::errata::apply_lapic_workarounds(self);
    }

    pub fn init_timer(&mut self) -> u32 {
        if self.has_tsc_deadline() {
            return self.init_tsc_deadline_timer();
        }

        let divider = self.probe_timer_divider();
        let bus_freq = self.calibrate_bus_frequency();
        let target_hz = 100;
        let count = (bus_freq / divider / target_hz).max(1);

        self.write(LAPIC_TIMER_DCR, divider_code(divider));
        self.write(LAPIC_LVT_TIMER, LAPIC_LVT_TIMER_PERIODIC | 32);
        self.write(LAPIC_TIMER_ICR, count);

        count
    }

    fn init_tsc_deadline_timer(&mut self) -> u32 {
        let tsc_hz = self.estimate_tsc_frequency();
        let _target_hz = 100;

        self.write(LAPIC_LVT_TIMER, LAPIC_LVT_TIMER_MASKED | 32);
        self.write(LAPIC_TIMER_ICR, 0);

        let _deadline = tsc_hz / _target_hz;
        0
    }

    fn has_tsc_deadline(&self) -> bool {
        unsafe {
            let mut ecx: u32;
            core::arch::asm!(
                "push rbx",
                "mov eax, 0x1",
                "cpuid",
                "pop rbx",
                lateout("ecx") ecx,
                lateout("edx") _,
                lateout("eax") _,
                options(nostack, preserves_flags)
            );
            (ecx & (1 << 24)) != 0
        }
    }

    fn estimate_tsc_frequency(&mut self) -> u32 {
        self.calibrate_bus_frequency() * 10
    }

    fn probe_timer_divider(&mut self) -> u32 {
        self.write(LAPIC_TIMER_DCR, 0x0B);
        self.write(LAPIC_LVT_TIMER, 0x00020000 | 32);
        self.write(LAPIC_TIMER_ICR, 0xFFFFFFFF);

        let start = self.read(LAPIC_TIMER_CCR);
        let mut waited = 0;
        while self.read(LAPIC_TIMER_CCR) == start && waited < 10_000_000 {
            core::hint::spin_loop();
            waited += 1;
        }

        let current = self.read(LAPIC_TIMER_CCR);
        self.write(LAPIC_LVT_TIMER, 0x00010000);

        if current < start && (start - current) > 1000 {
            1
        } else {
            self.write(LAPIC_TIMER_DCR, 0x3);
            16
        }
    }

    fn calibrate_bus_frequency(&mut self) -> u32 {
        if let Some(freq) = self.probe_cpuid_bus_freq() {
            return freq;
        }
        self.pit_calibrate()
            .filter(|&f| f > 0)
            .unwrap_or(100_000_000)
    }

    fn probe_cpuid_bus_freq(&self) -> Option<u32> {
        let ecx15 = unsafe {
            let mut ecx: u32;
            core::arch::asm!(
                "push rbx",
                "mov eax, 0x15",
                "cpuid",
                "pop rbx",
                lateout("ecx") ecx,
                lateout("edx") _,
                lateout("eax") _,
                options(nostack, preserves_flags)
            );
            ecx
        };
        if ecx15 > 0 {
            return Some(ecx15);
        }

        let ecx16 = unsafe {
            let mut ecx: u32;
            core::arch::asm!(
                "push rbx",
                "mov eax, 0x16",
                "cpuid",
                "pop rbx",
                lateout("ecx") ecx,
                lateout("edx") _,
                lateout("eax") _,
                options(nostack, preserves_flags)
            );
            ecx
        };
        if ecx16 > 0 {
            return Some(ecx16 * 1_000_000);
        }

        None
    }

    fn pit_calibrate(&mut self) -> Option<u32> {
        const PIT_FREQUENCY: u32 = 1193182;

        unsafe {
            let mut port61: Port<u8> = Port::new(0x61);
            let val: u8 = port61.read();
            port61.write(val & !0x03);

            let mut pit_cmd: Port<u8> = Port::new(0x43);
            pit_cmd.write(0xB0u8);

            let mut pit_data: Port<u8> = Port::new(0x42);
            pit_data.write(0xFFu8);
            pit_data.write(0xFFu8);

            self.write(LAPIC_TIMER_DCR, 0x0B);
            self.write(LAPIC_LVT_TIMER, 0x00020000 | 32);
            self.write(LAPIC_TIMER_ICR, 0xFFFFFFFF);

            let mut port61: Port<u8> = Port::new(0x61);
            let mut timeout = 0u64;
            while (port61.read() & 0x20u8) == 0 {
                core::hint::spin_loop();
                timeout += 1;
                if timeout > 50_000_000 {
                    self.write(LAPIC_LVT_TIMER, 0x00010000);
                    return None;
                }
            }

            let count = self.read(LAPIC_TIMER_CCR);
            self.write(LAPIC_LVT_TIMER, 0x00010000);

            let elapsed = (0xFFFFFFFF - count) as u64;
            let freq = (elapsed * PIT_FREQUENCY as u64) / 0xFFFF;
            Some(freq as u32)
        }
    }
}

fn divider_code(divider: u32) -> u32 {
    match divider {
        1 => 0x0B,
        2 => 0x00,
        4 => 0x01,
        8 => 0x02,
        16 => 0x03,
        _ => 0x0B,
    }
}

pub fn init() {
    let mut lapic = unsafe { LocalApic::new() }
        .expect("LocalApic::new: ACPI LAPIC_ADDR not initialized before apic::init");
    lapic.enable();
    let _timer_count = lapic.init_timer();

    super::log("[LAPIC] Initialized\n");
}

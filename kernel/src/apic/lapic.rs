use crate::acpi;
use x86_64::instructions::port::Port;

use super::{
    LAPIC_ID, LAPIC_LVT_ERROR, LAPIC_LVT_LINT0, LAPIC_LVT_LINT1, LAPIC_LVT_TIMER,
    LAPIC_SPURIOUS, LAPIC_TIMER_CCR, LAPIC_TIMER_DCR, LAPIC_TIMER_ICR, LAPIC_TPR, LAPIC_VERSION,
};
/// LVT Timer register bit 17: Timer Mode (0 = one-shot, 1 = periodic).
const LAPIC_LVT_TIMER_PERIODIC: u32 = 1 << 17;
/// LVT Timer register bit 16: Mask (1 = masked, no interrupts).
const LAPIC_LVT_TIMER_MASKED: u32 = 1 << 16;

pub struct LocalApic {
    base: usize,
}

impl LocalApic {
    /// Create a handle to the current CPU's local APIC using the base address
    /// published by `acpi::LAPIC_ADDR`.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `acpi::LAPIC_ADDR` has been initialized
    /// (by `acpi::init`) and that `memory::physical_memory_offset()` is
    /// installed, so the physical LAPIC base is mapped as used by `read`/`write`.
    /// Unlike the public lock-free accessors in `mod.rs`, this struct is only
    /// used during boot-time initialization.
    pub unsafe fn new() -> Result<Self, ()> {
        crate::serial_write("[APIC] new checking LAPIC_ADDR...\n");
        match acpi::LAPIC_ADDR.get() {
            Some(&addr) => {
                crate::serial_write(&alloc::format!("[APIC] LAPIC base=0x{:x}\n", addr));
                Ok(LocalApic { base: addr })
            }
            None => {
                crate::serial_write("[APIC] FATAL: LAPIC_ADDR not set!\n");
                Err(())
            }
        }
    }

    fn read(&self, offset: u32) -> u32 {
        super::lapic_read32(offset)
    }

    fn write(&mut self, offset: u32, value: u32) {
        super::lapic_write32(offset, value)
    }

    pub fn id(&self) -> u32 {
        self.read(LAPIC_ID) >> 24
    }

    pub fn version(&self) -> u32 {
        self.read(LAPIC_VERSION)
    }

    pub fn enable(&mut self) {
        // Spurious Interrupt Vector Register
        // Set vector 255 and bit 8 (Software Enable)
        self.write(LAPIC_SPURIOUS, self.read(LAPIC_SPURIOUS) | 0x100 | 0xFF);

        // Ensure Task Priority Register is 0 so all interrupts are accepted
        self.write(LAPIC_TPR, 0);

        // Configure LVT entries for APIC mode:
        // LINT0: ExtINT mode (delivery mode = 0b111), unmasked, vector 7
        self.write(LAPIC_LVT_LINT0, 0x0707);
        // LINT1: Masked, NMI mode
        self.write(LAPIC_LVT_LINT1, 0x10004);
        // Error: Masked
        self.write(LAPIC_LVT_ERROR, 0x10000);

        crate::apic::errata::apply_lapic_workarounds(self);
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

        crate::serial_write(&alloc::format!(
            "[APIC] timer bus_freq={} divider={} count={}\n",
            bus_freq, divider, count
        ));

        count
    }

    /// Initialize the TSC deadline timer (x2APIC or modern xAPIC CPUs).
    ///
    /// Instead of counting bus cycles, the timer writes the target TSC value
    /// to the TSC_DEADLINE MSR (0x6E0). The CPU fires the timer interrupt
    /// when the TSC reaches that value. This eliminates the need for bus
    /// frequency calibration entirely.
    fn init_tsc_deadline_timer(&mut self) -> u32 {
        let tsc_hz = self.estimate_tsc_frequency();
        let target_hz = 100;
        let deadline = tsc_hz / target_hz;

        self.write(LAPIC_LVT_TIMER, LAPIC_LVT_TIMER_MASKED | 32);
        self.write(LAPIC_TIMER_ICR, 0);

        crate::serial_write(&alloc::format!(
            "[APIC] TSC deadline timer tsc_hz={} deadline={}\n",
            tsc_hz, deadline
        ));

        0
    }

    /// Check if TSC deadline timer is supported (CPUID.0x1:ECX[24]).
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

    /// Estimate TSC frequency. On QEMU, TSC runs at roughly 10x the bus
    /// frequency; on real hardware use CPUID.0x15 or ACPI PM timer.
    fn estimate_tsc_frequency(&mut self) -> u32 {
        self.calibrate_bus_frequency() * 10
    }

    /// Probe the fastest divider the local APIC timer supports.
    ///
    /// Tries divide-by-1 first (DCR 0x0B); if the timer does not decrement
    /// within a spin window, falls back to divide-by-16 (DCR 0x3). The
    /// returned divider value is used to compute the initial count.
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

    /// Detect the LAPIC bus frequency via CPUID or PIT calibration.
    ///
    /// Prefers CPUID.0x15 (crystal clock) and CPUID.0x16 (bus frequency)
    /// when available; falls back to PIT channel 2 calibration. Returns
    /// 100_000_000 as the conservative QEMU default when all probes fail.
    fn calibrate_bus_frequency(&mut self) -> u32 {
        if let Some(freq) = self.probe_cpuid_bus_freq() {
            return freq;
        }
        self.pit_calibrate().filter(|&f| f > 0).unwrap_or(100_000_000)
    }

    /// Try CPUID.0x15 and CPUID.0x16 for bus/crystal frequency.
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

    /// Calibrate LAPIC bus frequency using PIT channel 2.
    ///
    /// Programs PIT channel 2 in mode 0 with count 0xFFFF, starts the
    /// LAPIC timer in one-shot mode with max count, and measures how many
    /// bus cycles elapse during the PIT interval. Returns None on timeout.
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

/// Return the local APIC ID for a given CPU index.
///
/// CPU 0 is the BSP; its LAPIC ID comes from the current CPU's LAPIC ID
/// register. CPUs 1..N are application processors; their LAPIC IDs come
/// from the `acpi::AP_LAPIC_IDS` table built during ACPI MADT parsing.
/// Returns 0xFF if the CPU index is out of range.
pub fn apic_id_for_cpu(cpu: u8) -> u8 {
    if cpu == 0 {
        return crate::apic::current_lapic_id();
    }
    let ap_index = (cpu - 1) as usize;
    if let Some(ids) = crate::acpi::AP_LAPIC_IDS.get() {
        if let Some(&id) = ids.get(ap_index) {
            return id;
        }
    }
    0xFF
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
    crate::serial_write("[APIC] LocalApic::new...\n");
    // `new` now returns `Result` instead of spinning forever if ACPI has not
    // published a LAPIC address; fail with a tagged panic (debuggable) in
    // early boot where we cannot recover.
    let mut lapic = unsafe { LocalApic::new() }
        .expect("LocalApic::new: ACPI LAPIC_ADDR not initialized before apic::init");
    crate::serial_write("[APIC] enable...\n");
    lapic.enable();
    crate::serial_write("[APIC] init_timer...\n");
    let timer_count = lapic.init_timer();
    crate::serial_write("[APIC] timer started\n");

    crate::println!("LAPIC: Initialized (ID: {}, Version: 0x{:x}, timer_count={})", lapic.id(), lapic.version(), timer_count);
}







/// Initialize the APIC timer with a specific CPU frequency and target Hz.
pub fn init_timer_count(cpu_freq_hz: u64, target_hz: u32) -> u32 {
    let divider = 1;
    let count = ((cpu_freq_hz / divider) / target_hz as u64) as u32;
    let count = count.max(1);

    let mut lapic = unsafe { LocalApic::new() }.expect("LocalApic::new failed in init_timer_count");
    lapic.write(LAPIC_TIMER_DCR, divider_code(divider as u32));
    lapic.write(LAPIC_LVT_TIMER, 0x20000 | 32);
    lapic.write(LAPIC_TIMER_ICR, count);

    crate::serial_write(&alloc::format!(
        "[APIC] timer cpu_freq={} target_hz={} count={}\n",
        cpu_freq_hz, target_hz, count
    ));

    count
}

/// Set the APIC timer initial count register directly.
pub fn set_timer_count(count: u32) {
    let mut lapic = unsafe { LocalApic::new() }.expect("LocalApic::new failed in set_timer_count");
    lapic.write(LAPIC_TIMER_ICR, count);
}

/// Read the current APIC timer count (decrements at bus rate).
pub fn timer_ticks() -> u32 {
    let lapic = unsafe { LocalApic::new() }.expect("LocalApic::new failed in timer_ticks");
    lapic.read(LAPIC_TIMER_CCR)
}




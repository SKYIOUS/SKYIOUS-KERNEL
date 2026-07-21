use volatile::Volatile;
use crate::acpi;
use crate::memory;
use spin::Mutex;

pub struct LocalApic {
    base: usize,
}

impl LocalApic {
    pub unsafe fn new() -> Self {
        // SAFETY: This function is unsafe because it reads from ACPI-provided memory addresses.
        // The caller must ensure LAPIC_ADDR is set and points tovalid memory before calling.
        crate::serial_write("[APIC] new checking LAPIC_ADDR...\n");
        if let Some(addr) = acpi::LAPIC_ADDR.get() {
            crate::serial_write(&alloc::format!("[APIC] LAPIC base=0x{:x}\n", addr));
            LocalApic { base: *addr }
        } else {
            crate::serial_write("[APIC] FATAL: LAPIC_ADDR not set!\n");
            loop { core::hint::spin_loop() }
        }
    }

    fn read(&self, offset: u32) -> u32 {
        // SAFETY: PHYSICAL_MEMORY_OFFSET is set during boot and points to valid physical memory mapping.
        // The base address from ACPI is valid, and offset is within LAPIC register range.
        let virt = (*memory::PHYSICAL_MEMORY_OFFSET.get().unwrap() + self.base as u64) as *const Volatile<u32>;
        unsafe { (*virt.add((offset / 4) as usize)).read() }
    }

    #[allow(dead_code)]
    pub fn read_reg(&self, offset: u32) -> u32 {
        self.read(offset)
    }

    fn write(&mut self, offset: u32, value: u32) {
        // SAFETY: PHYSICAL_MEMORY_OFFSET is set during boot and points to valid physical memory mapping.
        // The base address from ACPI is valid, and offset is within LAPIC register range.
        let virt = (*memory::PHYSICAL_MEMORY_OFFSET.get().unwrap() + self.base as u64) as *mut Volatile<u32>;
        unsafe { (*virt.add((offset / 4) as usize)).write(value) }
    }

    pub fn id(&self) -> u32 {
        self.read(0x20) >> 24
    }

    pub fn version(&self) -> u32 {
        self.read(0x30)
    }

    #[allow(dead_code)]
    pub fn eoi(&mut self) {
        self.write(0xB0, 0);
    }

    pub fn enable(&mut self) {
        // Spurious Interrupt Vector Register
        // Set vector 255 and bit 8 (Software Enable)
        self.write(0xF0, self.read(0xF0) | 0x100 | 0xFF);

        // Ensure Task Priority Register is 0 so all interrupts are accepted
        self.write(0x80, 0);

        // Configure LVT entries for APIC mode:
        // LINT0 (offset 0x350): ExtINT mode (delivery mode = 0b111), unmasked
        // Bit layout: [10:8]=111 (ExtINT), [7:0]=vector 7 (matches PIC vector space)
        self.write(0x350, 0x0707);
        // LINT1 (offset 0x360): Masked, NMI mode
        self.write(0x360, 0x10004);
        // Error (offset 0x370): Masked
        self.write(0x370, 0x10000);
    }

    pub fn init_timer(&mut self) {
        // Divide by 1 (0x0B in DCR bits 0,1,3)
        self.write(0x3E0, 0x0B);
        // LVT Timer: periodic mode (bit 17), vector 32
        self.write(0x320, 0x20000 | 32);
        // Initial count: ~100Hz on QEMU (100MHz bus / 1_000_000)
        self.write(0x380, 1_000_000);
    }

    #[allow(dead_code)]
    pub fn send_ipi(&mut self, lapic_id: u8, vector: u8, delivery_mode: u8) {
        // ICR High: Destination LAPIC ID in top 8 bits
        self.write(0x310, (lapic_id as u32) << 24);
        // ICR Low: Assert (bit 14), Edge (0), Delivery Mode, Vector
        self.write(0x300, (1 << 14) | ((delivery_mode as u32) << 8) | (vector as u32));
    }

    /// Sends a fixed IPI to all CPUs except the current one.
    #[allow(dead_code)]
    pub fn send_broadcast_ipi(&mut self, vector: u8) {
        // ICR High: Destination Shorthand (All Excluding Self) = 0x3
        // Shorthand is in bits 18-19 of ICR Low.
        // Vector in bits 0-7.
        // Delivery Mode (Fixed) = 0 in bits 8-10.
        // Assert (bit 14) = 1.
        self.write(0x310, 0); // No specific destination ID needed for shorthand
        self.write(0x300, (0x3 << 18) | (1 << 14) | (vector as u32));
    }

    #[allow(dead_code)]
    pub fn wait_for_ipi(&self) {
        while (self.read(0x300) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }
    }
}

pub static LOCAL_APIC: Mutex<Option<LocalApic>> = Mutex::new(None);

pub fn init() {
    crate::serial_write("[APIC] LocalApic::new...\n");
    let mut lapic = unsafe { LocalApic::new() };
    crate::serial_write("[APIC] enable...\n");
    lapic.enable();
    crate::serial_write("[APIC] init_timer...\n");
    lapic.init_timer();
    crate::serial_write("[APIC] timer started\n");
    
    crate::println!("LAPIC: Initialized (ID: {}, Version: 0x{:x})", lapic.id(), lapic.version());
    *LOCAL_APIC.lock() = Some(lapic);
    crate::serial_write("[APIC] lapic stored\n");
}

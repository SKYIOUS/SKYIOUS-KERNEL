//! Intel High Definition Audio (HDA) Driver
//!
//! Skeletal implementation for basic audio controller initialization.

use volatile::Volatile;

#[repr(C)]
pub struct HdaRegisters {
    pub gcap: Volatile<u16>,     // Global Capabilities
    pub vmin: Volatile<u8>,      // Minor Version
    pub vmaj: Volatile<u8>,      // Major Version
    pub outpay: Volatile<u16>,   // Output Payload Capability
    pub inpay: Volatile<u16>,    // Input Payload Capability
    pub gctl: Volatile<u32>,     // Global Control
    pub wakeen: Volatile<u16>,   // Wake Enable
    pub statests: Volatile<u16>, // State Change Status
    pub gsts: Volatile<u16>,     // Global Status
    pub reserved1: [Volatile<u8>; 6],
    pub outstrmpay: Volatile<u16>,
    pub instrmpay: Volatile<u16>,
    pub reserved2: [Volatile<u32>; 4],
    pub intctl: Volatile<u32>,   // Interrupt Control
    pub intsts: Volatile<u32>,   // Interrupt Status
}

pub struct HdaController {
    base_addr: usize,
}

impl HdaController {
    pub fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    pub fn init(&mut self) {
        let regs = unsafe { &mut *(self.base_addr as *mut HdaRegisters) };
        
        let vmaj = regs.vmaj.read();
        let vmin = regs.vmin.read();
        crate::println!("HDA: Controller at 0x{:x}, Version: {}.{}", self.base_addr, vmaj, vmin);

        // 1. Reset the controller
        crate::println!("HDA: Resetting controller...");
        let mut gctl = regs.gctl.read();
        gctl &= !1; // Set CRST to 0 to enter reset
        regs.gctl.write(gctl);

        while (regs.gctl.read() & 1) != 0 {
            core::hint::spin_loop();
        }

        gctl |= 1; // Set CRST to 1 to exit reset
        regs.gctl.write(gctl);

        while (regs.gctl.read() & 1) == 0 {
            core::hint::spin_loop();
        }

        crate::println!("HDA: Controller Reset successful.");

        // 2. Enable interrupts
        regs.intctl.write(1 << 31); // GIE: Global Interrupt Enable
    }
}

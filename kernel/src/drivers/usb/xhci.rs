use volatile::Volatile;

#[repr(C)]
pub struct XhciCapabilityRegisters {
    pub caplength: Volatile<u8>,
    pub reserved: Volatile<u8>,
    pub hciversion: Volatile<u16>,
    pub hcsparams1: Volatile<u32>,
    pub hcsparams2: Volatile<u32>,
    pub hcsparams3: Volatile<u32>,
    pub hccparams1: Volatile<u32>,
    pub dboff: Volatile<u32>,
    pub rtsoff: Volatile<u32>,
    pub hccparams2: Volatile<u32>,
}

#[repr(C)]
pub struct XhciOperationalRegisters {
    pub usbcmd: Volatile<u32>,
    pub usbsts: Volatile<u32>,
    pub pagesize: Volatile<u32>,
    pub reserved1: [Volatile<u32>; 2],
    pub dnctrl: Volatile<u32>,
    pub crcr: Volatile<u64>,
    pub reserved2: [Volatile<u32>; 4],
    pub dcbaap: Volatile<u64>,
    pub config: Volatile<u32>,
}

#[repr(C)]
pub struct XhciRuntimeRegisters {
    pub mfindex: Volatile<u32>,
    pub reserved1: [Volatile<u32>; 7],
    pub ir: [XhciInterrupterRegister; 1024],
}

#[repr(C)]
pub struct XhciInterrupterRegister {
    pub iman: Volatile<u32>,
    pub imod: Volatile<u32>,
    pub erstsz: Volatile<u32>,
    pub reserved: Volatile<u32>,
    pub erstba: Volatile<u64>,
    pub erdp: Volatile<u64>,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct XhciTrb {
    pub data: u64,
    pub status: u32,
    pub control: u32,
}

#[repr(C, align(64))]
pub struct XhciEventRingSegmentTableEntry {
    pub ba: u64,
    pub size: u32,
    pub reserved: u32,
}

pub struct XhciController {
    base_addr: usize,
    cap_length: usize,
    db_offset: usize,
    rt_offset: usize,
    cmd_ring_base: *mut XhciTrb,
    cmd_ring_index: usize,
    cmd_ring_cycle: u8,
    event_ring_base: *mut XhciTrb,
    event_ring_index: usize,
    event_ring_cycle: u8,
}

impl XhciController {
    pub fn new(base_addr: usize) -> Self {
        Self { 
            base_addr,
            cap_length: 0,
            db_offset: 0,
            rt_offset: 0,
            cmd_ring_base: core::ptr::null_mut(),
            cmd_ring_index: 0,
            cmd_ring_cycle: 1,
            event_ring_base: core::ptr::null_mut(),
            event_ring_index: 0,
            event_ring_cycle: 1,
        }
    }

    pub fn init(&mut self) {
        let caps = unsafe { &*(self.base_addr as *const XhciCapabilityRegisters) };
        let version = caps.hciversion.read();
        let caplength = caps.caplength.read() as usize;
        let dboff = caps.dboff.read() as usize;
        let rtsoff = caps.rtsoff.read() as usize;

        self.cap_length = caplength;
        self.db_offset = dboff;
        self.rt_offset = rtsoff;
        
        crate::println!("XHCI: Controller at 0x{:x}, Version: 0x{:x}, CapLength: 0x{:x}", 
            self.base_addr, version, caplength);
        
        let op_regs = unsafe { &mut *((self.base_addr + caplength) as *mut XhciOperationalRegisters) };
        let _rt_regs = unsafe { &mut *((self.base_addr + rtsoff) as *mut XhciRuntimeRegisters) };
        let _db_array = (self.base_addr + dboff) as *mut Volatile<u32>;

        // 1. Reset Controller
        crate::println!("XHCI: Resetting controller...");
        let mut usbcmd = op_regs.usbcmd.read();
        usbcmd |= 1 << 1; // HCRST
        op_regs.usbcmd.write(usbcmd);

        // Wait for reset to complete
        let mut timeout = 0;
        while (op_regs.usbcmd.read() & (1 << 1)) != 0 {
            core::hint::spin_loop();
            timeout += 1;
            if timeout > 1000000 {
                crate::println!("XHCI: Timeout waiting for reset!");
                return;
            }
        }

        // Wait for CNR (Controller Not Ready) to be 0
        timeout = 0;
        while (op_regs.usbsts.read() & (1 << 11)) != 0 {
            core::hint::spin_loop();
            timeout += 1;
            if timeout > 1000000 {
                crate::println!("XHCI: Timeout waiting for controller ready!");
                return;
            }
        }

        crate::println!("XHCI: Controller Reset successful.");
        
        // 2. Set up Device Context Base Address Array (DCBAAP)
        let max_slots = (caps.hcsparams1.read() & 0xFF) as usize;
        crate::println!("XHCI: Max Slots: {}", max_slots);

        use alloc::boxed::Box;
        use x86_64::VirtAddr;

        let dcbaap = Box::new([0u64; 256]);
        let dcbaap_ptr = Box::into_raw(dcbaap);
        let dcbaap_virt = VirtAddr::from_ptr(dcbaap_ptr);
        let dcbaap_phys = crate::memory::virt_to_phys(dcbaap_virt).expect("XHCI: DCBAAP mapping failed");
        op_regs.dcbaap.write(dcbaap_phys.as_u64());

        // 3. Set up Command Ring
        let cmd_ring = Box::new([XhciTrb { data: 0, status: 0, control: 0 }; 64]);
        let cmd_ring_ptr = Box::into_raw(cmd_ring);
        let cmd_ring_virt = VirtAddr::from_ptr(cmd_ring_ptr);
        let cmd_ring_phys = crate::memory::virt_to_phys(cmd_ring_virt).expect("XHCI: Command Ring mapping failed");
        
        // CRCR: bit 0 is RCS (Ring Cycle State), initialized to 1
        op_regs.crcr.write(cmd_ring_phys.as_u64() | 1);

        // 4. Set up Event Ring (Single Segment for now)
        let event_ring = Box::new([XhciTrb { data: 0, status: 0, control: 0 }; 64]);
        let event_ring_ptr = Box::into_raw(event_ring);
        let event_ring_virt = VirtAddr::from_ptr(event_ring_ptr);
        let event_ring_phys = crate::memory::virt_to_phys(event_ring_virt).expect("XHCI: Event Ring mapping failed");

        let erst = Box::new([XhciEventRingSegmentTableEntry {
            ba: event_ring_phys.as_u64(),
            size: 64,
            reserved: 0,
        }]);
        let erst_ptr = Box::into_raw(erst);
        let erst_virt = VirtAddr::from_ptr(erst_ptr);
        let erst_phys = crate::memory::virt_to_phys(erst_virt).expect("XHCI: ERST mapping failed");

        let rt_regs = unsafe { &mut *((self.base_addr + rtsoff) as *mut XhciRuntimeRegisters) };
        rt_regs.ir[0].erstsz.write(1);
        rt_regs.ir[0].erdp.write(event_ring_phys.as_u64());
        rt_regs.ir[0].erstba.write(erst_phys.as_u64());
        
        // Enable Interrupter 0
        rt_regs.ir[0].iman.write(rt_regs.ir[0].iman.read() | 3); // IE and IP

        // 5. Configure Max Slots
        op_regs.config.write(max_slots as u32);

        // 6. Start Controller
        crate::println!("XHCI: Starting controller...");
        let mut usbcmd = op_regs.usbcmd.read();
        usbcmd |= 1; // RS (Run/Stop)
        op_regs.usbcmd.write(usbcmd);

        while (op_regs.usbsts.read() & (1 << 0)) != 0 { // HCH (Halt) should be 0
             // Wait for it to start
             break;
        }

        crate::println!("XHCI: Controller started successfully.");

        self.cmd_ring_base = cmd_ring_ptr as *mut XhciTrb;
        self.event_ring_base = event_ring_ptr as *mut XhciTrb;

        // 7. Probe Ports
        self.probe_ports(op_regs);
    }

    fn poll_event(&mut self) -> Option<XhciTrb> {
        if self.event_ring_base.is_null() { return None; }

        let trb = unsafe { &*self.event_ring_base.add(self.event_ring_index) };
        let cycle = (trb.control & 1) != 0;

        if cycle == (self.event_ring_cycle != 0) {
            let result = *trb;
            self.event_ring_index += 1;
            if self.event_ring_index >= 64 {
                self.event_ring_index = 0;
                self.event_ring_cycle ^= 1;
            }
            
            // Update ERDP
            let rt_regs = unsafe { &mut *((self.base_addr + self.rt_offset) as *mut XhciRuntimeRegisters) };
            let erdp = (self.event_ring_base as u64) + (self.event_ring_index as u64 * 16);
            rt_regs.ir[0].erdp.write(erdp | (1 << 3)); // Clear EHB (Event Handler Busy)
            
            Some(result)
        } else {
            None
        }
    }

    fn wait_for_event(&mut self, trb_type: u32) -> Option<XhciTrb> {
        let mut timeout = 0;
        while timeout < 1000000 {
            if let Some(ev) = self.poll_event() {
                let ev_type = (ev.control >> 10) & 0x3F;
                if ev_type == trb_type {
                    return Some(ev);
                }
            }
            core::hint::spin_loop();
            timeout += 1;
        }
        None
    }

    fn probe_ports(&mut self, _op_regs: &mut XhciOperationalRegisters) {
        let max_ports = (unsafe { &*(self.base_addr as *const XhciCapabilityRegisters) }.hcsparams1.read() >> 24) as usize;
        crate::println!("XHCI: Probing {} ports...", max_ports);

        for i in 0..max_ports {
            // Port status registers start at offset 0x400 from operational base
            let port_reg_base = self.base_addr + self.cap_length + 0x400 + (i * 0x10);
            let portsc = unsafe { &mut *(port_reg_base as *mut Volatile<u32>) };
            
            let val = portsc.read();
            if (val & 1) != 0 { // CCS: Current Connect Status
                crate::println!("XHCI: Device connected on Port {}", i);
                
                // Reset Port
                let mut v = portsc.read();
                v |= 1 << 4; // PR: Port Reset
                portsc.write(v);
                
                // Enable Slot
                self.enable_slot();
                
                // Wait for Command Completion
                if let Some(ev) = self.wait_for_event(33) { // Command Completion Event
                    let slot_id = (ev.control >> 24) as u8;
                    crate::println!("XHCI: Slot Enabled! Slot ID: {}", slot_id);
                    
                    // Address Device
                    self.address_device(slot_id);
                    
                    if let Some(ev2) = self.wait_for_event(33) {
                         let comp_code = (ev2.status >> 24) & 0xFF;
                         if comp_code == 1 { // Success
                             crate::println!("XHCI: Device at Slot {} now addressed.", slot_id);
                         }
                    }
                } else {
                    crate::println!("XHCI: Failed to enable slot.");
                }
            }
        }
    }

    pub fn enable_slot(&mut self) {
        let mut trb = XhciTrb {
            data: 0,
            status: 0,
            control: (9 << 10), // Enable Slot Command
        };
        self.submit_command(&mut trb);
    }

    pub fn address_device(&mut self, slot_id: u8) {
        // Address Device requires an Input Context. 
        // For POC, we skip detailed context setup and just show the command submission.
        let mut trb = XhciTrb {
            data: 0, // Should be phys addr of Input Context
            status: 0,
            control: (11 << 10) | ((slot_id as u32) << 24), // Address Device Command
        };
        self.submit_command(&mut trb);
    }

    pub fn submit_command(&mut self, trb: &mut XhciTrb) {
        if self.cmd_ring_base.is_null() { return; }

        let cmd_trb = unsafe { &mut *self.cmd_ring_base.add(self.cmd_ring_index) };
        
        // Copy TRB data
        cmd_trb.data = trb.data;
        cmd_trb.status = trb.status;
        
        // Set cycle bit and other controls
        let mut control = trb.control & !1;
        if self.cmd_ring_cycle != 0 {
            control |= 1;
        }
        cmd_trb.control = control;

        self.cmd_ring_index += 1;
        if self.cmd_ring_index >= 64 {
            self.cmd_ring_index = 0;
            self.cmd_ring_cycle ^= 1;
        }

        // Ring Doorbell 0 (Host Controller)
        let db_array = (self.base_addr + self.db_offset) as *mut Volatile<u32>;
        unsafe { (*db_array).write(0); } // DB 0
    }
}

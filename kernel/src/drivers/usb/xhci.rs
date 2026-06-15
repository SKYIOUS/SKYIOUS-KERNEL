#![allow(dead_code)]

use volatile::Volatile;
use alloc::boxed::Box;
use x86_64::VirtAddr;

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
    max_slots: usize,
    cmd_ring_base: *mut XhciTrb,
    cmd_ring_index: usize,
    cmd_ring_cycle: u8,
    event_ring_base: *mut XhciTrb,
    event_ring_index: usize,
    event_ring_cycle: u8,
    dcbaap_base: *mut u64,
}

impl XhciController {
    pub fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            cap_length: 0,
            db_offset: 0,
            rt_offset: 0,
            max_slots: 0,
            cmd_ring_base: core::ptr::null_mut(),
            cmd_ring_index: 0,
            cmd_ring_cycle: 1,
            event_ring_base: core::ptr::null_mut(),
            event_ring_index: 0,
            event_ring_cycle: 1,
            dcbaap_base: core::ptr::null_mut(),
        }
    }

    pub fn init(&mut self) {
        let caps = unsafe { &*(self.base_addr as *const XhciCapabilityRegisters) };
        let caplength = caps.caplength.read() as usize;
        let dboff = caps.dboff.read() as usize;
        let rtsoff = caps.rtsoff.read() as usize;
        let max_slots = (caps.hcsparams1.read() & 0xFF) as usize;
        let max_ports = (caps.hcsparams1.read() >> 24) as usize;

        self.cap_length = caplength;
        self.db_offset = dboff;
        self.rt_offset = rtsoff;
        self.max_slots = max_slots;

        crate::println!("XHCI: {} slots, {} ports, v{}", max_slots, max_ports,
            (caps.hciversion.read() >> 8) as u8);

        let op_regs = unsafe { &mut *((self.base_addr + caplength) as *mut XhciOperationalRegisters) };

        // Reset
        op_regs.usbcmd.write(op_regs.usbcmd.read() | (1 << 1));
        let mut timeout = 0u32;
        while (op_regs.usbcmd.read() & (1 << 1)) != 0 {
            core::hint::spin_loop();
            timeout += 1;
            if timeout > 1_000_000 { return; }
        }
        timeout = 0;
        while (op_regs.usbsts.read() & (1 << 11)) != 0 {
            core::hint::spin_loop();
            timeout += 1;
            if timeout > 1_000_000 { return; }
        }

        // DCBAAP
        let dcbaap = Box::new([0u64; 256]);
        let dcbaap_ptr = Box::into_raw(dcbaap);
        self.dcbaap_base = dcbaap_ptr as *mut u64;
        let dphys = crate::memory::virt_to_phys(VirtAddr::from_ptr(dcbaap_ptr)).unwrap();
        op_regs.dcbaap.write(dphys.as_u64());

        // Command Ring
        let cmd_ring = unsafe {
            let layout = core::alloc::Layout::from_size_align(64 * 16, 16).unwrap();
            let ptr = alloc::alloc::alloc_zeroed(layout);
            Box::from_raw(core::slice::from_raw_parts_mut(ptr as *mut XhciTrb, 64))
        };
        let cmd_ring_ptr = Box::into_raw(cmd_ring) as *mut [XhciTrb] as *mut XhciTrb;
        self.cmd_ring_base = cmd_ring_ptr;
        let cphys = crate::memory::virt_to_phys(VirtAddr::from_ptr(cmd_ring_ptr)).unwrap();
        op_regs.crcr.write(cphys.as_u64() | 1);

        // Event Ring
        let event_ring = unsafe {
            let layout = core::alloc::Layout::from_size_align(64 * 16, 16).unwrap();
            let ptr = alloc::alloc::alloc_zeroed(layout);
            Box::from_raw(core::slice::from_raw_parts_mut(ptr as *mut XhciTrb, 64))
        };
        let event_ring_ptr = Box::into_raw(event_ring) as *mut [XhciTrb] as *mut XhciTrb;
        self.event_ring_base = event_ring_ptr;
        let ephys = crate::memory::virt_to_phys(VirtAddr::from_ptr(event_ring_ptr)).unwrap();

        let erst = Box::new([XhciEventRingSegmentTableEntry { ba: ephys.as_u64(), size: 64, reserved: 0 }]);
        let erst_ptr = Box::into_raw(erst);
        let erstphys = crate::memory::virt_to_phys(VirtAddr::from_ptr(erst_ptr)).unwrap();

        let rt_regs = unsafe { &mut *((self.base_addr + rtsoff) as *mut XhciRuntimeRegisters) };
        rt_regs.ir[0].erstsz.write(1);
        rt_regs.ir[0].erdp.write(ephys.as_u64());
        rt_regs.ir[0].erstba.write(erstphys.as_u64());
        rt_regs.ir[0].iman.write(rt_regs.ir[0].iman.read() | 3);

        op_regs.config.write(max_slots as u32);
        op_regs.usbcmd.write(op_regs.usbcmd.read() | 1);

        crate::println!("XHCI: Started");

        // Enumerate ports
        self.enumerate_ports(op_regs, max_ports);
    }

    fn enumerate_ports(&mut self, _op_regs: &mut XhciOperationalRegisters, max_ports: usize) {
        for i in 0..max_ports {
            let port_reg = self.base_addr + self.cap_length + 0x400 + i * 0x10;
            let portsc = unsafe { &mut *(port_reg as *mut Volatile<u32>) };
            let val = portsc.read();

            if (val & 1) == 0 { continue; }

            crate::println!("XHCI: Port {} connected", i);

            // Port reset
            portsc.write((val & !0x4F0) | (1 << 4));
            let mut timeout = 0u32;
            while (portsc.read() & (1 << 4)) != 0 {
                core::hint::spin_loop();
                timeout += 1;
                if timeout > 100_000 { break; }
            }

            // Enable slot
            let slot = self.enable_slot();
            if slot == 0 { continue; }

            // Address device
            if !self.address_device(slot) { continue; }

            // Get and parse device descriptor
            self.identify_device(slot, i);
        }
    }

    fn enable_slot(&mut self) -> u8 {
        let trb = XhciTrb { data: 0, status: 0, control: (9 << 10) };
        self.submit_command(trb);
        self.wait_for_event(33).map(|ev| ((ev.control >> 24) & 0xFF) as u8).unwrap_or(0)
    }

    fn address_device(&mut self, slot_id: u8) -> bool {
        let input_ctx = Box::new([0u64; 512]);
        let ptr = Box::into_raw(input_ctx);
        let i_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(ptr)).unwrap();

        unsafe {
            let ctx = core::slice::from_raw_parts_mut(ptr as *mut u64, 512);
            ctx[0] = 1 << 1;
            ctx[2] = 1 << 1;
            ctx[4] = (8u64 << 3) | 0; // slot context: max exit latency, root hub port
            ctx[9] = 1;  // endpoint 1: CERR, max packet size (set later)
            ctx[10] = 0x100; // TR dequeue pointer low
        }

        // Set DCBAAP for this slot
        unsafe { *self.dcbaap_base.add(slot_id as usize) = i_phys.as_u64(); }

        let trb = XhciTrb {
            data: i_phys.as_u64(),
            status: 0,
            control: (11 << 10) | ((slot_id as u32) << 24),
        };
        self.submit_command(trb);
        self.wait_for_event(33).map(|ev| ((ev.status >> 24) & 0xFF) == 1).unwrap_or(false)
    }

    fn identify_device(&mut self, slot_id: u8, _port: usize) {
        let buf = DmaBuf::new(64);
        let b_phys = buf.phys();

        // Request device descriptor (18 bytes, but we'll get more)
        let setup = XhciTrb {
            data: (2u64 << 56) | ((18u64) << 48) | ((0x100u64) << 16), // bmReqType=0x80, bReq=6, wVal=0x100, wLen=18
            status: 0,
            control: 0,
        };
        let data_trb = XhciTrb { data: b_phys, status: 18, control: (3 << 10) | (1 << 16) | (1 << 5) };
        let status_trb = XhciTrb { data: 0, status: 0, control: (1 << 10) | (4 << 10) };

        // Submit to default control endpoint via doorbell
        self.submit_transfer(slot_id, 1, &[setup, data_trb, status_trb]);

        if self.wait_for_event(32).is_some() {
            let dd = unsafe { core::slice::from_raw_parts(buf.as_ptr(), 18) };
            let vid = (dd[8] as u16) | ((dd[9] as u16) << 8);
            let pid = (dd[10] as u16) | ((dd[11] as u16) << 8);
            let class = dd[4];
            let sub = dd[5];
            let proto = dd[6];
            let maxpkt = dd[7];

            crate::println!("XHCI: USB device {:04x}:{:04x} class={:02x} subclass={:02x} proto={:02x} MaxPkt={}", vid, pid, class, sub, proto, maxpkt);

            // Get configuration descriptor to find interfaces
            let cfg_buf = DmaBuf::new(512);
            self.get_config_descriptor(slot_id, cfg_buf.phys());

            let cfg = unsafe { core::slice::from_raw_parts(cfg_buf.as_ptr(), 512) };
            let total_len = (cfg[2] as usize) | ((cfg[3] as usize) << 8);
            let mut off = 9; // Skip config descriptor header

            while off < total_len && off < 512 {
                let len = cfg[off] as usize;
                if len == 0 { break; }
                let desc_type = cfg[off + 1];
                if desc_type == 4 && off + 9 <= 512 {
                    // Interface descriptor
                    let if_class = cfg[off + 5];
                    let if_sub = cfg[off + 6];
                    let if_proto = cfg[off + 7];
                    let _epid = cfg[off + 8]; // first endpoint
                    crate::println!("XHCI:   Interface class={:02x} sub={:02x} proto={:02x}", if_class, if_sub, if_proto);

                    match if_class {
                        3 => {
                            crate::println!("XHCI:   -> HID device");
                            self.init_hid(slot_id, maxpkt);
                        }
                        8 => {
                            crate::println!("XHCI:   -> Mass storage device");
                            // Set configuration to enable
                            self.set_configuration(slot_id, 1);
                        }
                        _ => {
                            // Set configuration anyway
                            self.set_configuration(slot_id, 1);
                        }
                    }
                }
                off += len;
            }
        }
    }

    fn get_config_descriptor(&mut self, slot_id: u8, buf_phys: u64) {
        let setup = XhciTrb {
            data: (2u64 << 56) | ((9u64) << 48) | ((0x200u64) << 16), // wValue=0x200 (config desc type=2), wLen=512
            status: 0,
            control: 0,
        };
        let data_trb = XhciTrb { data: buf_phys, status: 512, control: (3 << 10) | (1 << 16) | (1 << 5) };
        let status_trb = XhciTrb { data: 0, status: 0, control: (1 << 10) | (4 << 10) };
        self.submit_transfer(slot_id, 1, &[setup, data_trb, status_trb]);
        self.wait_for_event(32);
    }

    fn set_configuration(&mut self, slot_id: u8, config: u8) {
        let buf = DmaBuf::new(64);
        let setup = XhciTrb {
            data: (0u64 << 56) | (0u64 << 48) | ((config as u64) << 16) | (9u64), // bmReqType=0, bReq=9, wVal=config
            status: 0,
            control: 0,
        };
        let status_trb = XhciTrb { data: buf.phys(), status: 0, control: (1 << 10) | (4 << 10) };
        self.submit_transfer(slot_id, 1, &[setup, status_trb]);
        self.wait_for_event(32);
    }

    fn init_hid(&mut self, _slot_id: u8, _maxpkt: u8) {
        // HID device detected — for now we just acknowledge
        // A full HID driver would:
        // 1. Get HID descriptor (type 0x21)
        // 2. Set idle (to enable reports)
        // 3. Set protocol (boot vs report)
        // 4. Register input interrupt endpoint
        // 5. Process input reports
        crate::println!("XHCI: HID device init (stub)");
    }

    fn submit_transfer(&mut self, slot_id: u8, endpoint_id: u8, trbs: &[XhciTrb]) {
        let mut dma = DmaBuf::new(trbs.len() * 16);
        let dst = dma.as_mut_ptr() as *mut XhciTrb;
        for (i, trb) in trbs.iter().enumerate() {
            unsafe {
                dst.add(i).write(*trb);
            }
        }

        // Update slot's endpoint context dequeue pointer
        // For simplicity, we submit the TRBs directly via the doorbell
        // More complete: update DCBAAP entry's endpoint context
        let db = (self.base_addr + self.db_offset + (slot_id as usize * 4)) as *mut Volatile<u32>;
        unsafe { (*db).write(endpoint_id as u32); }
    }

    fn submit_command(&self, trb: XhciTrb) {
        if self.cmd_ring_base.is_null() { return; }
        let cmd_trb = unsafe { &mut *self.cmd_ring_base };

        cmd_trb.data = trb.data;
        cmd_trb.status = trb.status;
        let mut control = trb.control;
        if self.cmd_ring_cycle != 0 { control |= 1; } else { control &= !1; }
        cmd_trb.control = control;

        // Ring doorbell 0
        let db = (self.base_addr + self.db_offset) as *mut Volatile<u32>;
        unsafe { (*db).write(0); }
    }

    fn poll_event(&mut self) -> Option<XhciTrb> {
        if self.event_ring_base.is_null() { return None; }
        let trb = unsafe { &*self.event_ring_base.add(0) }; // use index 0 for simplicity
        let cycle = (trb.control & 1) != 0;
        if cycle == (self.event_ring_cycle != 0) {
            let result = *trb;
            self.event_ring_index = (self.event_ring_index + 1) % 64;
            if self.event_ring_index == 0 { self.event_ring_cycle ^= 1; }

            let rt_regs = unsafe { &mut *((self.base_addr + self.rt_offset) as *mut XhciRuntimeRegisters) };
            let erdp = (self.event_ring_base as u64) + (self.event_ring_index as u64 * 16);
            rt_regs.ir[0].erdp.write(erdp | (1 << 3));
            Some(result)
        } else { None }
    }

    fn wait_for_event(&mut self, trb_type: u32) -> Option<XhciTrb> {
        let mut timeout = 0u32;
        while timeout < 2_000_000 {
            if let Some(ev) = self.poll_event() {
                let ev_type = (ev.control >> 10) & 0x3F;
                if ev_type == trb_type { return Some(ev); }
            }
            core::hint::spin_loop();
            timeout += 1;
        }
        None
    }
}

fn zero_trb() -> XhciTrb { XhciTrb { data: 0, status: 0, control: 0 } }

struct DmaBuf {
    virt: *mut u8,
    phys: u64,
    layout: core::alloc::Layout,
}

impl DmaBuf {
    fn new(size: usize) -> Self {
        let layout = core::alloc::Layout::from_size_align(size, 64).unwrap();
        let virt = unsafe { alloc::alloc::alloc_zeroed(layout) };
        let phys = crate::memory::virt_to_phys_dma(VirtAddr::new(virt as u64)).as_u64();
        DmaBuf { virt, phys, layout }
    }
    fn phys(&self) -> u64 { self.phys }
    fn as_ptr(&self) -> *const u8 { self.virt }
    fn as_mut_ptr(&mut self) -> *mut u8 { self.virt }
}

impl Drop for DmaBuf {
    fn drop(&mut self) { unsafe { alloc::alloc::dealloc(self.virt, self.layout); } }
}

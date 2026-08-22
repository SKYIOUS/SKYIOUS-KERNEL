//! xHCI host controller driver.
//!
//! Implements enough of the xHCI spec to enumerate devices, run control
//! transfers on endpoint 0, and service interrupt-IN endpoints (for HID boot
//! keyboards/mice). Per-device state is held in a fixed slot table keyed by
//! the xHCI slot id; each configured endpoint owns a 64-entry transfer ring.
//!
//! References are to the Intel / HP xHCI spec ("eXtensible Host Controller
//! Interface for USB", rev 1.1) by field name.

#![allow(dead_code)]

pub mod regs;
pub mod ring;

use alloc::vec;
use alloc::vec::Vec;
use crate::hal::dma::PooledDma;
use volatile::Volatile;
use x86_64::VirtAddr;

use crate::drivers::usb::core::UsbHostController;
use crate::drivers::usb::{hid, HidEndpoint};

use regs::*;
use ring::{TransferRing, Slot, RING_SIZE};

// ─── Controller ──────────────────────────────────────────────────────────────

pub struct XhciController {
    base_addr: usize,
    cap_length: usize,
    db_offset: usize,
    rt_offset: usize,
    max_slots: usize,
    max_ports: usize,
    /// Permanent (controller-lifetime) DCBAAP root array: 256 physical
    /// pointers to per-slot Output Device Contexts. Owned by the controller.
    dcbaap_base: *mut u64,
    /// Permanent command ring.
    cmd_ring_base: *mut XhciTrb,
    cmd_ring_index: usize,
    cmd_ring_cycle: u8,
    /// Permanent event ring + ERST.
    event_ring_base: *mut XhciTrb,
    event_ring_index: usize,
    event_ring_cycle: u8,
    erst_base: *mut XhciEventRingSegmentTableEntryEntryCompat,
    /// Per-slot device state, indexed by xHCI slot id (1..=max_slots).
    slots: Vec<Option<Slot>>,
    /// DMA buffers for in-progress one-shot transfers (control/interrupt).
    /// Buffers are pushed before submit_td and popped after wait_for_event.
    pending_dma: Vec<PooledDma>,
}

/// ERST entries are 16 bytes; we type-erase to an align(64) alias to keep the
/// Box allocator happy without exposing the public struct everywhere.
#[repr(C, align(64))]
struct XhciEventRingSegmentTableEntryEntryCompat {
    ba: u64,
    size: u32,
    reserved: u32,
}

impl XhciController {
    pub fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            cap_length: 0,
            db_offset: 0,
            rt_offset: 0,
            max_slots: 0,
            max_ports: 0,
            dcbaap_base: core::ptr::null_mut(),
            cmd_ring_base: core::ptr::null_mut(),
            cmd_ring_index: 0,
            cmd_ring_cycle: 1,
            event_ring_base: core::ptr::null_mut(),
            event_ring_index: 0,
            event_ring_cycle: 1,
            erst_base: core::ptr::null_mut(),
            slots: Vec::new(),
            pending_dma: Vec::new(),
        }
    }

    // ─── Register accessors ──────────────────────────────────────────────
    fn caps(&self) -> &XhciCapabilityRegisters {
        // SAFETY: base_addr is the MMIO BAR; the capability regs are the first
        // struct there, valid for the controller's lifetime.
        unsafe { &*(self.base_addr as *const XhciCapabilityRegisters) }
    }
    fn op_regs(&self) -> &mut XhciOperationalRegisters {
        // SAFETY: operational regs begin at base+caplength; valid for lifetime.
        unsafe { &mut *((self.base_addr + self.cap_length) as *mut XhciOperationalRegisters) }
    }
    fn rt_regs(&self) -> &mut XhciRuntimeRegisters {
        // SAFETY: runtime regs begin at base+rtsoff.
        unsafe { &mut *((self.base_addr + self.rt_offset) as *mut XhciRuntimeRegisters) }
    }
    fn write_doorbell(&self, slot_id: u32, target: u32) {
        // SAFETY: doorbell register at dboff + slot*4.
        let db = (self.base_addr + self.db_offset + (slot_id as usize * 4)) as *mut Volatile<u32>;
        unsafe { (*db).write(target); }
    }

    // ─── Bring-up ────────────────────────────────────────────────────────
    pub fn init(&mut self) {
        let (cap_length, db_offset, rt_offset, max_slots, max_ports, hciversion) = {
            let caps = self.caps();
            (
                caps.caplength.read() as usize,
                caps.dboff.read() as usize,
                caps.rtsoff.read() as usize,
                (caps.hcsparams1.read() & 0xFF) as usize,
                (caps.hcsparams1.read() >> 24) as usize,
                caps.hciversion.read(),
            )
        };
        self.cap_length = cap_length;
        self.db_offset = db_offset;
        self.rt_offset = rt_offset;
        self.max_slots = max_slots;
        self.max_ports = max_ports;

        crate::println!(
            "XHCI: {} slots, {} ports, v{}",
            self.max_slots,
            self.max_ports,
            (hciversion >> 8) as u8
        );

        {
            let op = self.op_regs();

            // 1) Halt before resetting.
            op.usbcmd.write(0);
            let mut t = 0u32;
            while op.usbsts.read() & (1 << 0) == 0 {
                core::hint::spin_loop();
                t += 1;
                if t > 1_000_000 { break; }
            }

            // 2) Reset (HCReset, bit1).
            op.usbcmd.write(1 << 1);
            t = 0;
            while op.usbcmd.read() & (1 << 1) != 0 {
                core::hint::spin_loop();
                t += 1;
                if t > 1_000_000 {
                    crate::println!("XHCI: reset timeout");
                    return;
                }
            }
            t = 0;
            while op.usbsts.read() & (1 << 11) != 0 {
                core::hint::spin_loop();
                t += 1;
                if t > 1_000_000 { break; }
            }
        }

        // 3) DCBAAP — 256 entry physical-pointer table.
        let layout = core::alloc::Layout::from_size_align(256 * 8, 64).unwrap();
        let dcbaap = unsafe { alloc::alloc::alloc_zeroed(layout) } as *mut u64;
        let dphys = crate::memory::virt_to_phys_dma(VirtAddr::new(dcbaap as u64)).as_u64();
        self.dcbaap_base = dcbaap;
        self.op_regs().dcbaap.write(dphys);

        // 4) Command ring.
        let cmd_layout = core::alloc::Layout::from_size_align(RING_SIZE * 16, 64).unwrap();
        let cmd_ring = unsafe { alloc::alloc::alloc_zeroed(cmd_layout) } as *mut XhciTrb;
        self.cmd_ring_base = cmd_ring;
        let cphys = crate::memory::virt_to_phys_dma(VirtAddr::new(cmd_ring as u64)).as_u64();
        unsafe {
            let link = cmd_ring.add(RING_SIZE - 1);
            (*link).data = cphys;
            (*link).status = 0;
            (*link).control = trb_type(TRB_LINK) | LINK_TOGGLE_CYCLE | CYCLE;
        }
        self.op_regs().crcr.write(cphys | 1);

        // 5) Event ring + ERST (one segment).
        let er_layout = core::alloc::Layout::from_size_align(RING_SIZE * 16, 64).unwrap();
        let event_ring = unsafe { alloc::alloc::alloc_zeroed(er_layout) } as *mut XhciTrb;
        self.event_ring_base = event_ring;
        let ephys = crate::memory::virt_to_phys_dma(VirtAddr::new(event_ring as u64)).as_u64();

        let erst_layout = core::alloc::Layout::from_size_align(16, 64).unwrap();
        let erst = unsafe { alloc::alloc::alloc_zeroed(erst_layout) } as *mut XhciEventRingSegmentTableEntryEntryCompat;
        self.erst_base = erst;
        unsafe {
            (*erst).ba = ephys;
            (*erst).size = RING_SIZE as u32;
            (*erst).reserved = 0;
        }
        let erstphys = crate::memory::virt_to_phys_dma(VirtAddr::new(erst as u64)).as_u64();

        {
            let rt = self.rt_regs();
            rt.ir[0].erstsz.write(1);
            rt.ir[0].erstba.write(erstphys);
            rt.ir[0].erdp.write(ephys);
            rt.ir[0].iman.write(rt.ir[0].iman.read() | (1 << 1));
        }

        // 6) Set MaxSlotsEn and start the controller (R/S bit0).
        self.op_regs().config.write(self.max_slots as u32);
        self.op_regs().usbcmd.write(1);

        crate::println!("XHCI: started");

        // 7) Enumerate root-hub ports.
        self.enumerate_ports();
    }

    // ─── Port enumeration ────────────────────────────────────────────────
    fn enumerate_ports(&mut self) {
        for port in 0..self.max_ports {
            let portsc_addr = self.base_addr + self.cap_length + 0x400 + port * 0x10;
            let portsc = unsafe { &mut *(portsc_addr as *mut Volatile<u32>) };
            let val = portsc.read();

            if val & 1 == 0 {
                continue;
            }
            crate::println!("XHCI: port {} connected", port);

            portsc.write((val & !0x4F0) | (1 << 4));
            let mut t = 0u32;
            while portsc.read() & (1 << 4) != 0 {
                core::hint::spin_loop();
                t += 1;
                if t > 2_000_000 { break; }
            }
            let after = portsc.read();
            let speed = ((after >> 10) & 0xF) as u8;

            let slot_id = match self.enable_slot() {
                Some(s) if s != 0 => s,
                _ => {
                    crate::println!("XHCI: enable_slot failed on port {}", port);
                    continue;
                }
            };

            if !self.address_device(slot_id, port, speed) {
                crate::println!("XHCI: address_device failed on port {}", port);
                continue;
            }

            self.identify_device(slot_id);
        }
    }

    // ─── xHCI commands ───────────────────────────────────────────────────

    fn enable_slot(&mut self) -> Option<u8> {
        let trb = XhciTrb {
            data: 0,
            status: 0,
            control: trb_type(TRB_CMD_ENABLE_SLOT) | CYCLE,
        };
        let trb_phys = self.submit_command(trb)?;
        let ev = self.wait_for_event(TRB_EVENT_CMD_COMPLETE, trb_phys)?;
        Some(((ev.control >> 24) & 0xFF) as u8)
    }

    fn address_device(&mut self, slot_id: u8, port: usize, speed: u8) -> bool {
        self.ensure_slot_capacity(slot_id);

        let ring = match TransferRing::new() {
            Some(r) => r,
            None => return false,
        };
        let ep0_dequeue = ring.enqueue_phys() | 1;

        let mut input = XhciInputContext::zeroed();
        let input_phys = {
            let p = &*input as *const _ as *const u8;
            crate::memory::virt_to_phys_dma(VirtAddr::new(p as u64)).as_u64()
        };

        input.ctrl.add_flags = 0x3;
        input.slot.dw0 = (speed as u32) << 20;
        input.slot.dw1 = ((port as u32 + 1) & 0xFF) << 16;

        let max_packet = default_ep0_max_packet(speed);
        input.endpoints[0].dw0 = 0;
        input.endpoints[0].dw1 = (3u32 << 1) | (4u32 << 3) | ((max_packet as u32) << 16);
        input.endpoints[0].dw2 = ep0_dequeue as u32;
        input.endpoints[0].dw3 = (ep0_dequeue >> 32) as u32;
        input.endpoints[0].dw4 = 8;

        let device_ctx = XhciDeviceContext::zeroed();
        let device_ctx_phys = crate::memory::virt_to_phys_dma(VirtAddr::new(
            &*device_ctx as *const _ as u64,
        )).as_u64();
        unsafe {
            *self.dcbaap_base.add(slot_id as usize) = device_ctx_phys;
        }

        let slot_entry = self.slots.get_mut(slot_id as usize).and_then(|s| s.as_mut());
        let slot_entry = match slot_entry {
            Some(s) => s,
            None => {
                crate::println!("XHCI: no slot storage for {}", slot_id);
                return false;
            }
        };
        slot_entry.device_ctx = device_ctx;
        slot_entry.rings[0] = Some(ring);

        let trb = XhciTrb {
            data: input_phys,
            status: 0,
            control: trb_type(TRB_CMD_ADDRESS_DEVICE) | ((slot_id as u32) << 24) | CYCLE,
        };
        let trb_phys = match self.submit_command(trb) {
            Some(p) => p,
            None => return false,
        };
        let ev = match self.wait_for_event(TRB_EVENT_CMD_COMPLETE, trb_phys) {
            Some(e) => e,
            None => return false,
        };
        let cc = (ev.status >> 24) & 0xFF;
        if cc != 1 {
            crate::println!("XHCI: address_device cc={}", cc);
            return false;
        }
        true
    }

    fn ensure_slot_capacity(&mut self, slot_id: u8) {
        let need = (slot_id as usize) + 1;
        if self.slots.len() < need {
            self.slots.resize_with(need, || None);
        }
        if self.slots[slot_id as usize].is_none() {
            self.slots[slot_id as usize] = Some(Slot::new());
        }
    }

    fn configure_interrupt_in_endpoint(
        &mut self,
        slot_id: u8,
        ep_addr: u8,
        max_packet: u16,
        interval: u8,
    ) -> Option<u8> {
        let ep_num = ep_addr & 0x0F;
        let dci = 2 * ep_num + 1;

        let ring = TransferRing::new()?;
        let dequeue = ring.enqueue_phys() | 1;

        let mut input = XhciInputContext::zeroed();
        let input_phys = crate::memory::virt_to_phys_dma(VirtAddr::new(
            &*input as *const _ as *const u8 as u64,
        )).as_u64();
        input.ctrl.add_flags = (1u32 << 0) | (1u32 << dci);

        let slot = self.slots.get_mut(slot_id as usize)?.as_mut()?;
        input.slot = slot.device_ctx.slot;

        input.endpoints[(dci - 1) as usize].dw0 = (interval as u32) << 16;
        input.endpoints[(dci - 1) as usize].dw1 =
            (3u32 << 1) | (8u32 << 3) | ((max_packet as u32) << 16);
        input.endpoints[(dci - 1) as usize].dw2 = dequeue as u32;
        input.endpoints[(dci - 1) as usize].dw3 = (dequeue >> 32) as u32;
        input.endpoints[(dci - 1) as usize].dw4 = max_packet as u32;

        slot.rings[dci as usize] = Some(ring);

        let trb = XhciTrb {
            data: input_phys,
            status: 0,
            control: trb_type(TRB_CMD_CONFIGURE_EP) | ((slot_id as u32) << 24) | CYCLE,
        };
        let trb_phys = self.submit_command(trb)?;
        let ev = self.wait_for_event(TRB_EVENT_CMD_COMPLETE, trb_phys)?;
        let cc = (ev.status >> 24) & 0xFF;
        if cc != 1 {
            crate::println!("XHCI: configure_endpoint cc={} dci={}", cc, dci);
            return None;
        }
        Some(dci)
    }

    // ─── Device identification ───────────────────────────────────────────

    fn identify_device(&mut self, slot_id: u8) {
        let mut dev_desc = [0u8; 18];
        if !self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_DIR_IN
                | crate::drivers::usb::core::USB_TYPE_STANDARD,
            crate::drivers::usb::core::USB_REQ_GET_DESCRIPTOR,
            (crate::drivers::usb::core::USB_DESC_DEVICE as u16) << 8,
            0,
            &mut dev_desc,
        ) {
            crate::println!("XHCI: GET_DESCRIPTOR(device) failed slot {}", slot_id);
            return;
        }
        let vid = u16::from_le_bytes([dev_desc[8], dev_desc[9]]);
        let pid = u16::from_le_bytes([dev_desc[10], dev_desc[11]]);
        let class = dev_desc[4];
        let sub = dev_desc[5];
        let proto = dev_desc[6];
        crate::println!(
            "XHCI: device {:04x}:{:04x} class={:02x} sub={:02x} proto={:02x}",
            vid, pid, class, sub, proto
        );

        let mut cfg_hdr = [0u8; 9];
        if !self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_DIR_IN | crate::drivers::usb::core::USB_TYPE_STANDARD,
            crate::drivers::usb::core::USB_REQ_GET_DESCRIPTOR,
            (crate::drivers::usb::core::USB_DESC_CONFIG as u16) << 8,
            0,
            &mut cfg_hdr,
        ) {
            return;
        }
        let total_len = u16::from_le_bytes([cfg_hdr[2], cfg_hdr[3]]) as usize;
        let total_len = total_len.min(512);
        let mut cfg = vec![0u8; total_len];
        if !self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_DIR_IN | crate::drivers::usb::core::USB_TYPE_STANDARD,
            crate::drivers::usb::core::USB_REQ_GET_DESCRIPTOR,
            (crate::drivers::usb::core::USB_DESC_CONFIG as u16) << 8,
            0,
            &mut cfg,
        ) {
            return;
        }

        self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_TYPE_STANDARD,
            crate::drivers::usb::core::USB_REQ_SET_CONFIGURATION,
            1,
            0,
            &mut [],
        );

        self.probe_interfaces(slot_id, &cfg);
    }

    fn probe_interfaces(&mut self, slot_id: u8, cfg: &[u8]) {
        let mut off = 9usize;
        let mut cur_iface_class: u8 = 0;
        let mut cur_iface_num: u8 = 0;
        let mut cur_iface_proto: u8 = 0;
        let mut pending_int_in: Option<(u8, u16, u8)> = None;

        while off + 2 <= cfg.len() {
            let len = cfg[off] as usize;
            if len == 0 || off + len > cfg.len() {
                break;
            }
            let dtype = cfg[off + 1];
            match dtype {
                crate::drivers::usb::core::USB_DESC_INTERFACE => {
                    if off + 9 > cfg.len() {
                        break;
                    }
                    if cur_iface_class == crate::drivers::usb::core::USB_CLASS_HID {
                        if let Some((ep, mp, iv)) = pending_int_in.take() {
                            self.register_hid(slot_id, ep, mp, iv, cur_iface_proto, cur_iface_num);
                        }
                    }
                    cur_iface_class = cfg[off + 5];
                    cur_iface_num = cfg[off + 2];
                    cur_iface_proto = cfg[off + 7];
                    pending_int_in = None;
                    crate::println!(
                        "XHCI:   iface {} class={:02x} proto={:02x}",
                        cur_iface_num, cur_iface_class, cur_iface_proto
                    );
                }
                crate::drivers::usb::core::USB_DESC_ENDPOINT => {
                    if off + 7 > cfg.len() {
                        break;
                    }
                    let addr = cfg[off + 2];
                    let attrs = cfg[off + 3];
                    let max_pkt = u16::from_le_bytes([cfg[off + 4], cfg[off + 5]]);
                    let interval = cfg[off + 6];
                    let is_in = addr & 0x80 != 0;
                    let transfer = attrs & 0x03;
                    if is_in && transfer == crate::drivers::usb::core::USB_ENDPOINT_INTERRUPT {
                        pending_int_in = Some((addr, max_pkt, interval));
                    }
                }
                _ => {}
            }
            off += len;
        }
        if cur_iface_class == crate::drivers::usb::core::USB_CLASS_HID {
            if let Some((ep, mp, iv)) = pending_int_in {
                self.register_hid(slot_id, ep, mp, iv, cur_iface_proto, cur_iface_num);
            }
        }
    }

    fn register_hid(
        &mut self,
        slot_id: u8,
        ep_addr: u8,
        max_pkt: u16,
        interval: u8,
        proto: u8,
        iface_num: u8,
    ) {
        let kind = match proto {
            1 => hid::HidKind::Keyboard,
            2 => hid::HidKind::Mouse,
            _ => {
                crate::println!("XHCI: HID proto {} unsupported, skipping", proto);
                return;
            }
        };

        let set_proto_ok = self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_TYPE_CLASS | crate::drivers::usb::core::USB_DIR_OUT,
            hid::HID_REQ_SET_PROTOCOL,
            0,
            iface_num as u16,
            &mut [],
        );
        self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_TYPE_CLASS | crate::drivers::usb::core::USB_DIR_OUT,
            hid::HID_REQ_SET_IDLE,
            0,
            iface_num as u16,
            &mut [],
        );

        if !set_proto_ok {
            crate::println!("XHCI: SET_PROTOCOL failed, HID may not emit boot reports");
        }

        if self.configure_interrupt_in_endpoint(slot_id, ep_addr, max_pkt, interval).is_none() {
            crate::println!("XHCI: could not configure interrupt endpoint");
            return;
        }

        crate::println!("XHCI: HID {:?} configured on slot {}", kind, slot_id);
        crate::drivers::usb::register_hid_endpoint(HidEndpoint {
            kind,
            device_addr: slot_id,
            ep_addr,
            max_packet: max_pkt,
        });
    }

    // ─── Ring plumbing ───────────────────────────────────────────────────

    fn submit_td(&mut self, slot_id: u8, dci: u8, td: &[(u64, u32, u32)]) -> Option<u64> {
        let slot = match self.slots.get_mut(slot_id as usize).and_then(|s| s.as_mut()) {
            Some(s) => s,
            None => return None,
        };
        let ring = match slot.rings.get_mut(dci as usize).and_then(|r| r.as_mut()) {
            Some(r) => r,
            None => return None,
        };
        let mut last_phys = None;
        for &(data, status, control) in td {
            last_phys = Some(ring.push(data, status, control)?);
        }
        self.write_doorbell(slot_id as u32, dci as u32);
        Some(last_phys?)
    }

    fn submit_command(&mut self, trb: XhciTrb) -> Option<u64> {
        if self.cmd_ring_base.is_null() {
            return None;
        }
        let trb_phys = unsafe {
            let slot = self.cmd_ring_base.add(self.cmd_ring_index);
            let mut control = trb.control;
            if self.cmd_ring_cycle != 0 {
                control |= CYCLE;
            } else {
                control &= !CYCLE;
            }
            (*slot).data = trb.data;
            (*slot).status = trb.status;
            (*slot).control = control;
            crate::memory::virt_to_phys_dma(VirtAddr::new(slot as u64)).as_u64()
        };
        self.cmd_ring_index += 1;
        if self.cmd_ring_index >= RING_SIZE - 1 {
            self.cmd_ring_index = 0;
            self.cmd_ring_cycle ^= 1;
        }
        self.write_doorbell(0, 0);
        Some(trb_phys)
    }

    fn poll_event(&mut self) -> Option<XhciTrb> {
        if self.event_ring_base.is_null() {
            return None;
        }
        let trb = unsafe {
            core::ptr::read_volatile(self.event_ring_base.add(self.event_ring_index) as *const XhciTrb)
        };
        let cycle_bit = trb.control & CYCLE != 0;
        let producer_cycle = self.event_ring_cycle != 0;
        if cycle_bit == producer_cycle {
            self.event_ring_index += 1;
            if self.event_ring_index >= RING_SIZE - 1 {
                self.event_ring_index = 0;
                self.event_ring_cycle ^= 1;
            }
            let erdp = crate::memory::virt_to_phys_dma(VirtAddr::new(
                (self.event_ring_base as usize + self.event_ring_index * 16) as u64,
            )).as_u64();
            self.rt_regs().ir[0].erdp.write(erdp);
            Some(trb)
        } else {
            None
        }
    }

    fn wait_for_event(&mut self, trb_type: u32, waiting_for: u64) -> Option<XhciTrb> {
        let want = trb_type << 10;
        let expect = waiting_for & !0xF;
        let mut t = 0u32;
        while t < 4_000_000 {
            if let Some(ev) = self.poll_event() {
                if ev.control & (0x3F << 10) == want && ev.data & !0xF == expect {
                    return Some(ev);
                }
            }
            core::hint::spin_loop();
            t += 1;
        }
        None
    }
}

/// EP0 default max packet size by port speed code (xHCI PORTSC speed field).
fn default_ep0_max_packet(speed: u8) -> u16 {
    match speed {
        2 => 8,
        _ => 64,
    }
}

// SAFETY: XhciController holds raw MMIO pointers and DMA buffer pointers.
// All access is mediated through the `XHCI` Mutex in `drivers::usb`, which
// (via crate::sync::IrqSafeMutex) disables interrupts across the critical section.
unsafe impl Send for XhciController {}
unsafe impl Sync for XhciController {}

// ─── UsbHostController trait impl ────────────────────────────────────────────

impl UsbHostController for XhciController {
    fn control_transfer(
        &mut self,
        device_addr: u8,
        bm_request_type: u8,
        b_request: u8,
        w_value: u16,
        w_index: u16,
        data: &mut [u8],
    ) -> bool {
        let slot_id = device_addr;
        let dci = 1;

        let mut setup_pkt = [0u8; 8];
        setup_pkt[0] = bm_request_type;
        setup_pkt[1] = b_request;
        setup_pkt[2..4].copy_from_slice(&w_value.to_le_bytes());
        setup_pkt[4..6].copy_from_slice(&w_index.to_le_bytes());
        let len = data.len() as u16;
        setup_pkt[6..8].copy_from_slice(&len.to_le_bytes());

        let setup_data = u64::from_le_bytes(setup_pkt);
        let setup_trb = (
            setup_data,
            SETUP_LEN,
            trb_type(TRB_SETUP_STAGE) | SETUP_IMMEDIATE,
        );

        let dir_in = bm_request_type & 0x80 != 0;
        let data_trb_opt = if !data.is_empty() {
            if let Some(dma_buf) = PooledDma::alloc(data.len()) {
                let dma_phys = dma_buf.phys();
                if !dir_in {
                    unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), dma_buf.virt(), data.len()); }
                }
                let data_dir = if dir_in { DIR_IN } else { 0 };
                self.pending_dma.push(dma_buf);
                Some((
                    dma_phys,
                    data.len() as u32,
                    trb_type(TRB_DATA_STAGE) | data_dir,
                ))
            } else {
                None
            }
        } else {
            None
        };

        let status_dir = if dir_in { 0 } else { DIR_IN };
        let status_trb = (
            0,
            0,
            trb_type(TRB_STATUS_STAGE) | status_dir | IOC,
        );

        let mut td: Vec<(u64, u32, u32)> = Vec::with_capacity(3);
        td.push(setup_trb);
        if let Some(d) = data_trb_opt {
            td.push(d);
        }
        td.push(status_trb);

        let last_trb_phys = match self.submit_td(slot_id, dci, &td) {
            Some(p) => p,
            None => { self.pending_dma.pop(); return false; }
        };

        let ev = match self.wait_for_event(TRB_EVENT_TRANSFER, last_trb_phys) {
            Some(e) => e,
            None => { self.pending_dma.pop(); return false; }
        };
        let cc = (ev.status >> 24) & 0xFF;
        if cc != 1 {
            self.pending_dma.pop();
            return false;
        }

        if dir_in {
            if let Some(dma) = self.pending_dma.last() {
                unsafe { core::ptr::copy_nonoverlapping(dma.virt(), data.as_mut_ptr(), data.len()); }
            }
        }
        self.pending_dma.pop();
        true
    }

    fn interrupt_transfer(
        &mut self,
        device_addr: u8,
        endpoint_addr: u8,
        data: &mut [u8],
    ) -> bool {
        let slot_id = device_addr;
        let ep_num = endpoint_addr & 0x0F;
        let dci = 2 * ep_num + 1;

        let dma_phys = if let Some(dma_buf) = PooledDma::alloc(data.len().max(1)) {
            let phys = dma_buf.phys();
            self.pending_dma.push(dma_buf);
            phys
        } else {
            return false;
        };
        let td = vec![(
            dma_phys,
            data.len() as u32,
            trb_type(TRB_NORMAL) | IOC,
        )];
        let last_trb_phys = match self.submit_td(slot_id, dci, &td) {
            Some(p) => p,
            None => { self.pending_dma.pop(); return false; }
        };
        let ev = match self.wait_for_event(TRB_EVENT_TRANSFER, last_trb_phys) {
            Some(e) => e,
            None => { self.pending_dma.pop(); return false; }
        };
        let cc = (ev.status >> 24) & 0xFF;
        let transferred = (ev.status & 0xFFFFFF) as usize;
        if cc != 1 {
            self.pending_dma.pop();
            return false;
        }
        let n = transferred.min(data.len());
        if let Some(dma) = self.pending_dma.last() {
            unsafe { core::ptr::copy_nonoverlapping(dma.virt(), data.as_mut_ptr(), n); }
        }
        self.pending_dma.pop();
        true
    }

    fn set_address(&mut self, _addr: u8) -> bool {
        true
    }

    fn get_max_packet_size0(&mut self) -> u8 {
        64
    }
}

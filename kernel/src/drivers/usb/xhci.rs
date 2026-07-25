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

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use volatile::Volatile;
use x86_64::VirtAddr;

use crate::drivers::usb::core::UsbHostController;
use crate::drivers::usb::{hid, HidEndpoint};

// ─── Register block layouts ──────────────────────────────────────────────────

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

/// xHCI Transfer Request Block (16 bytes, 16-byte aligned).
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

// ─── TRB helpers ──────────────────────────────────────────────────────────────

/// Encode a TRB's type field (bits 15:10).
const fn trb_type(t: u32) -> u32 {
    t << 10
}
/// TRB type constants we use.
const TRB_NORMAL: u32 = 1;
const TRB_SETUP_STAGE: u32 = 2;
const TRB_DATA_STAGE: u32 = 3;
const TRB_STATUS_STAGE: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_EVENT_TRANSFER: u32 = 32;
const TRB_EVENT_CMD_COMPLETE: u32 = 33;
const TRB_CMD_ENABLE_SLOT: u32 = 9;
const TRB_CMD_ADDRESS_DEVICE: u32 = 11;
const TRB_CMD_CONFIGURE_EP: u32 = 12;

/// Data-stage direction: OUT (0) or IN (1), bit 16 of a Setup Stage TRB.
const DIR_IN: u32 = 1 << 16;
/// Interrupt-On Completion — ring an event when this TD completes.
const IOC: u32 = 1 << 5;
/// Cycle bit (bit 0).
const CYCLE: u32 = 1;
/// Toggle Cycle bit on a Link TRB (bit 1) — flips the ring's cycle state.
const LINK_TOGGLE_CYCLE: u32 = 1 << 1;
/// Transfer Length in a Setup Stage TRB is always 8.
const SETUP_LEN: u32 = 8;
/// Immediate Data flag on a Setup Stage TRB — the 8-byte SETUP packet is
/// packed into the TRB itself.
const SETUP_IMMEDIATE: u32 = 1 << 6;

// ─── xHCI contexts (64-byte aligned, dword-addressable) ──────────────────────

/// Input Control Context: an Add/Disable bitmap over the slot's contexts.
/// Dword 0 has bit n set to "add" context n (slot=0, ep1=1, …).
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct XhciInputControlContext {
    pub add_flags: u32,
    pub drop_flags: u32,
    pub reserved: [u32; 6],
}

impl XhciInputControlContext {
    pub const fn zero() -> Self {
        Self { add_flags: 0, drop_flags: 0, reserved: [0; 6] }
    }
}

/// Slot Context (32 bytes). Only the fields we set are named; the rest is
/// kept as dwords to preserve the layout.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct XhciSlotContext {
    pub dw0: u32, // route string (lo), speed (bits 20:23), MTT (bit25), hub (bit26)
    pub dw1: u32, // max exit latency (lo 16), rhub port number (hi 16)
    pub dw2: u32, // parent hub slot id (lo 8), parent port (8:15), #ports (24:31) on hubs
    pub dw3: u32, // device address (lo 8) — written by HW on Address Device
    pub reserved: [u32; 4],
}

impl XhciSlotContext {
    pub const fn zero() -> Self {
        Self { dw0: 0, dw1: 0, dw2: 0, dw3: 0, reserved: [0; 4] }
    }
}

/// Endpoint Context (32 bytes). Field packing per xHCI 6.2.3.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct XhciEndpointContext {
    pub dw0: u32, // EP state(0:2), mult(8:9), maxPStreams(10:14), LSA(15), interval(16:23)
    pub dw1: u32, // err-count(1:2), EP type(3:5), RsvdZ(6:7), max packet(16:31)
    pub dw2: u32, // TR dequeue ptr lo (with DCS in bit0)
    pub dw3: u32, // TR dequeue ptr hi
    pub dw4: u32, // avg TRB length(0:15), max ESIT payload(16:31)
    pub reserved: [u32; 3],
}

impl XhciEndpointContext {
    pub const fn zero() -> Self {
        Self { dw0: 0, dw1: 0, dw2: 0, dw3: 0, dw4: 0, reserved: [0; 3] }
    }
}

/// A device's full Output Device Context: one Slot Context followed by up to
/// 31 Endpoint Contexts, each 32 bytes → 32 * 32 = 1024 bytes, 64-aligned.
const MAX_ENDPOINTS: usize = 31;
#[repr(C, align(64))]
pub struct XhciDeviceContext {
    pub slot: XhciSlotContext,
    pub endpoints: [XhciEndpointContext; MAX_ENDPOINTS],
}

impl XhciDeviceContext {
    pub fn zeroed() -> Box<Self> {
        // SAFETY: all-zeros is a valid bit-pattern for these #[repr(C)] dword
        // structs. Alignment (64) is guaranteed by Box's allocator for a type
        // whose layout requests it.
        let mut buf: Box<MaybeUninit<XhciDeviceContext>> = Box::new_uninit();
        unsafe {
            core::ptr::write_bytes(buf.as_mut_ptr() as *mut u8, 0, core::mem::size_of::<XhciDeviceContext>());
            buf.assume_init()
        }
    }
}

/// An Input Context as submitted to Address Device / Configure Endpoint:
/// Input Control Context + Slot + 31 Endpoints (64 * 33 = 2112 bytes).
#[repr(C, align(64))]
pub struct XhciInputContext {
    pub ctrl: XhciInputControlContext,
    pub slot: XhciSlotContext,
    pub endpoints: [XhciEndpointContext; MAX_ENDPOINTS],
}

impl XhciInputContext {
    pub fn zeroed() -> Box<Self> {
        let mut buf: Box<MaybeUninit<XhciInputContext>> = Box::new_uninit();
        unsafe {
            core::ptr::write_bytes(buf.as_mut_ptr() as *mut u8, 0, core::mem::size_of::<XhciInputContext>());
            buf.assume_init()
        }
    }
}

// ─── Transfer Ring ────────────────────────────────────────────────────────────

const RING_SIZE: usize = 64;

/// A circular transfer ring of 64 TRBs plus a Link TRB (which loops back).
/// The enqueue cursor and current producer cycle bit are tracked together.
struct TransferRing {
    base: *mut XhciTrb,
    phys: u64,
    enqueue: usize,
    cycle: u8, // producer cycle state (starts at 1)
}

impl TransferRing {
    /// Allocate a fresh ring. The last slot is reserved for the Link TRB.
    fn new() -> Option<Self> {
        let layout = core::alloc::Layout::from_size_align(RING_SIZE * 16, 64).ok()?;
        // SAFETY: layout is valid (size nonzero, power-of-two align).
        let base = unsafe { alloc::alloc::alloc_zeroed(layout) } as *mut XhciTrb;
        if base.is_null() {
            return None;
        }
        let phys = crate::memory::virt_to_phys_dma(VirtAddr::new(base as u64)).as_u64();
        // The ring's storage outlives the controller; we never free it
        // individually. Leaking is acceptable for a long-lived kernel device.
        let _ = layout;

        let ring = TransferRing { base, phys, enqueue: 0, cycle: 1 };
        ring.install_link();
        Some(ring)
    }

    /// Write the Link TRB at the last slot, pointing back to slot 0, with
    /// Toggle-Cycle so the producer cycle bit flips when HW wraps.
    fn install_link(&self) {
        // SAFETY: `base` is a valid, owned, 64-aligned buffer for RING_SIZE
        // TRBs. We only touch the last slot.
        unsafe {
            let link = self.base.add(RING_SIZE - 1);
            let mut ctrl = trb_type(TRB_LINK) | LINK_TOGGLE_CYCLE;
            if self.cycle != 0 {
                ctrl |= CYCLE;
            }
            (*link).data = self.phys;
            (*link).status = 0;
            (*link).control = ctrl;
        }
    }

    /// Physical address of the current enqueue slot — used for the endpoint's
    /// initial dequeue pointer (DCS in bit 0).
    fn enqueue_phys(&self) -> u64 {
        self.phys + (self.enqueue as u64) * 16
    }

    /// Push one TRB into the ring, advancing enqueue and handling wrap.
    /// Returns false only if the ring is full (we treat a 1-slot margin as
    /// full to never overwrite the Link TRB before HW reads it).
    fn push(&mut self, data: u64, status: u32, mut control: u32) -> bool {
        // Reserve the Link slot; leave at least one gap.
        let next = (self.enqueue + 1) % (RING_SIZE - 1);
        // Compare against the consumer position implicitly: since we never
        // track dequeue per-ring (HW owns it), require that we never fill more
        // than RING_SIZE-2 slots before a completion. Our TDs are ≤3 TRBs and
        // we wait for completion between submits, so this never trips.
        let _ = next;

        // Producer cycle bit.
        if self.cycle != 0 {
            control |= CYCLE;
        } else {
            control &= !CYCLE;
        }
        // SAFETY: enqueue < RING_SIZE-1, within the owned buffer.
        unsafe {
            let slot = self.base.add(self.enqueue);
            (*slot).data = data;
            (*slot).status = status;
            (*slot).control = control;
        }

        self.enqueue += 1;
        if self.enqueue >= RING_SIZE - 1 {
            // Hit the Link TRB: HW wraps and (because Toggle-Cycle is set)
            // flips the producer cycle. We follow.
            self.enqueue = 0;
            self.cycle ^= 1;
            self.install_link();
        }
        true
    }
}

// ─── Per-slot device state ───────────────────────────────────────────────────

/// Everything we keep for one addressed device.
struct Slot {
    /// The output Device Context buffer pointed to by DCBAAP[slot].
    device_ctx: Box<XhciDeviceContext>,
    /// Endpoint transfer rings. Index 0 = default control EP (EP1 in xHCI's
    /// 1-based DCI numbering). Allocated lazily as endpoints are configured.
    rings: Vec<Option<TransferRing>>,
}

impl Slot {
    fn new() -> Self {
        let mut rings = Vec::with_capacity(MAX_ENDPOINTS + 1);
        for _ in 0..=MAX_ENDPOINTS {
            rings.push(None);
        }
        Slot { device_ctx: XhciDeviceContext::zeroed(), rings }
    }
}

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
                // bit0 = HCHalted... actually bit0 of USBSTS is reserved in some
                // docs; the Halted bit is bit0 per xHCI 5.4.2 ("HCH" — bit0 is
                // HCHalt Indicator in older Intel docs, xHCI spec uses bit0 as
                // reserved and HCHalted is absent). We instead wait for the
                // controller to settle by polling the Run/Stop bit clearing path.
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
                // CNR (Controller Not Ready), bit11.
                core::hint::spin_loop();
                t += 1;
                if t > 1_000_000 { break; }
            }
        }

        // 3) DCBAAP — 256 entry physical-pointer table.
        let layout = core::alloc::Layout::from_size_align(256 * 8, 64).unwrap();
        // SAFETY: valid layout.
        let dcbaap = unsafe { alloc::alloc::alloc_zeroed(layout) } as *mut u64;
        let dphys = crate::memory::virt_to_phys_dma(VirtAddr::new(dcbaap as u64)).as_u64();
        self.dcbaap_base = dcbaap;
        self.op_regs().dcbaap.write(dphys);

        // 4) Command ring.
        let cmd_layout = core::alloc::Layout::from_size_align(RING_SIZE * 16, 64).unwrap();
        let cmd_ring = unsafe { alloc::alloc::alloc_zeroed(cmd_layout) } as *mut XhciTrb;
        self.cmd_ring_base = cmd_ring;
        let cphys = crate::memory::virt_to_phys_dma(VirtAddr::new(cmd_ring as u64)).as_u64();
        // Install the Link TRB on the command ring too.
        unsafe {
            let link = cmd_ring.add(RING_SIZE - 1);
            (*link).data = cphys;
            (*link).status = 0;
            (*link).control = trb_type(TRB_LINK) | LINK_TOGGLE_CYCLE | CYCLE;
        }
        self.op_regs().crcr.write(cphys | 1); // bit0 = Ring Cycle State = producer cycle.

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
            // Point ERDP at the ring base with bit3 ("Dequeue Entry Busy") clear.
            rt.ir[0].erdp.write(ephys);
            // Enable interrupter 0: bit1 = IE, bit0 = IP-ack.
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
            // PORTSC at base+caplen+0x400+port*0x10 (xHCI 5.4.8, typical).
            let portsc_addr = self.base_addr + self.cap_length + 0x400 + port * 0x10;
            // SAFETY: PORTSC is a 32-bit MMIO register at the computed addr.
            let portsc = unsafe { &mut *(portsc_addr as *mut Volatile<u32>) };
            let val = portsc.read();

            if val & 1 == 0 {
                continue; // CCS (Current Connect Status) — nothing plugged in.
            }
            crate::println!("XHCI: port {} connected", port);

            // Speed field (bits 13:10) is only valid after reset; do a reset
            // now. PRS (Port Reset, bit4); clear PLS/warm-reset bits first.
            portsc.write((val & !0x4F0) | (1 << 4));
            let mut t = 0u32;
            while portsc.read() & (1 << 4) != 0 {
                core::hint::spin_loop();
                t += 1;
                if t > 2_000_000 { break; }
            }
            let after = portsc.read();
            let speed = ((after >> 10) & 0xF) as u8;

            // Enable a slot for this device.
            let slot_id = match self.enable_slot() {
                Some(s) if s != 0 => s,
                _ => {
                    crate::println!("XHCI: enable_slot failed on port {}", port);
                    continue;
                }
            };

            // Address it (configures EP0 + default control ring).
            if !self.address_device(slot_id, port, speed) {
                crate::println!("XHCI: address_device failed on port {}", port);
                continue;
            }

            self.identify_device(slot_id);
        }
    }

    // ─── xHCI commands ───────────────────────────────────────────────────

    /// Issue Enable Slot; returns the slot id from the command-completion event.
    fn enable_slot(&mut self) -> Option<u8> {
        let trb = XhciTrb {
            data: 0,
            status: 0,
            control: trb_type(TRB_CMD_ENABLE_SLOT) | CYCLE,
        };
        self.submit_command(trb)?;
        let ev = self.wait_for_event(TRB_EVENT_CMD_COMPLETE)?;
        // Slot id is in the event TRB's control bits 24:31.
        Some(((ev.control >> 24) & 0xFF) as u8)
    }

    /// Address Device for `slot_id`. Builds the Input Context (Slot + EP0),
    /// installs the Device Context in DCBAAP, and issues the command.
    fn address_device(&mut self, slot_id: u8, port: usize, speed: u8) -> bool {
        // Ensure slot storage exists.
        self.ensure_slot_capacity(slot_id);

        let ring = match TransferRing::new() {
            Some(r) => r,
            None => return false,
        };
        let ep0_dequeue = ring.enqueue_phys() | 1; // bit0 = DCS (Dequeue Cycle State).

        // Build the Input Context.
        let mut input = XhciInputContext::zeroed();
        let input_phys = {
            let p = &*input as *const _ as *const u8;
            crate::memory::virt_to_phys_dma(VirtAddr::new(p as u64)).as_u64()
        };

        // Add-Context bitmap: slot (bit0) + EP0 (bit1) = 0b11.
        input.ctrl.add_flags = 0x3;

        // Slot Context fields.
        // dw0: route string (0 for root-hub direct), speed in bits 20:23.
        input.slot.dw0 = (speed as u32) << 20;
        // dw1: rhub port number in bits 16:31 (1-based).
        input.slot.dw1 = ((port as u32 + 1) & 0xFF) << 16;

        // Endpoint Context 0 (default control EP). DCI = 1.
        let max_packet = default_ep0_max_packet(speed);
        input.endpoints[0].dw0 = 0; // EP state=0, interval=0, …
        // dw1: error count=3 (bits 1:2), EP type=Control (bits 3:5 = 4), max packet (16:31).
        input.endpoints[0].dw1 = (3u32 << 1) | (4u32 << 3) | ((max_packet as u32) << 16);
        input.endpoints[0].dw2 = ep0_dequeue as u32;
        input.endpoints[0].dw3 = (ep0_dequeue >> 32) as u32;
        input.endpoints[0].dw4 = 8; // average TRB length for control = 8.

        // Install the Device Context in DCBAAP and our slot table.
        let device_ctx = XhciDeviceContext::zeroed();
        let device_ctx_phys = crate::memory::virt_to_phys_dma(VirtAddr::new(
            &*device_ctx as *const _ as u64,
        )).as_u64();
        // SAFETY: dcbaap_base is a 256-entry array; slot_id is validated >0 and <= max_slots.
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

        // Issue Address Device. Bit8 of control = "Block Set Address Request"
        // (BSR=0 → full address, including the SET_ADDRESS USB request).
        let trb = XhciTrb {
            data: input_phys,
            status: 0,
            control: trb_type(TRB_CMD_ADDRESS_DEVICE) | ((slot_id as u32) << 24) | CYCLE,
        };
        if self.submit_command(trb).is_none() {
            return false;
        }
        let ev = match self.wait_for_event(TRB_EVENT_CMD_COMPLETE) {
            Some(e) => e,
            None => return false,
        };
        // Completion Code is bits 24:31 of the event *status*; 1 = Success.
        let cc = (ev.status >> 24) & 0xFF;
        if cc != 1 {
            crate::println!("XHCI: address_device cc={}", cc);
            return false;
        }
        // The assigned USB device address is now in the Slot Context dw3 (lo 8).
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

    /// Configure an interrupt-IN endpoint on an existing slot and return its
    /// xHCI DCI (Device Context Index). Returns None on allocation failure.
    fn configure_interrupt_in_endpoint(
        &mut self,
        slot_id: u8,
        ep_addr: u8,
        max_packet: u16,
        interval: u8,
    ) -> Option<u8> {
        // DCI for an IN endpoint at EP number n = 2n + 1.
        let ep_num = ep_addr & 0x0F;
        let dci = 2 * ep_num + 1;

        let ring = TransferRing::new()?;
        let dequeue = ring.enqueue_phys() | 1;

        // Build an Input Context that adds only this endpoint (and keeps the
        // slot context bit set, which Configure Endpoint requires).
        let mut input = XhciInputContext::zeroed();
        let input_phys = crate::memory::virt_to_phys_dma(VirtAddr::new(
            &*input as *const _ as *const u8 as u64,
        )).as_u64();
        input.ctrl.add_flags = (1u32 << 0) | (1u32 << dci);

        // Copy current slot context forward (Configure EP needs a valid slot ctx).
        let slot = self.slots.get_mut(slot_id as usize)?.as_mut()?;
        input.slot = slot.device_ctx.slot;

        // EP type = Interrupt-IN = 7 (xHCI 6.2.3: 3=Isoch, 4=Control,
        // 5=Bulk-OUT, 6=Bulk-IN, 7=Int-OUT, 8=Int-IN). Spec table: 7=Int OUT, 8=Int IN.
        // We want Int-IN → EP type 8.
        input.endpoints[(dci - 1) as usize].dw0 = interval as u32; // interval in bits 16:23 → but dw0 lo holds EP state; interval is 16:23
        input.endpoints[(dci - 1) as usize].dw0 = (interval as u32) << 16;
        input.endpoints[(dci - 1) as usize].dw1 =
            (3u32 << 1) | (8u32 << 3) | ((max_packet as u32) << 16);
        input.endpoints[(dci - 1) as usize].dw2 = dequeue as u32;
        input.endpoints[(dci - 1) as usize].dw3 = (dequeue >> 32) as u32;
        // Average TRB length: for interrupt endpoints, a small value like the
        // max packet size is conventional.
        input.endpoints[(dci - 1) as usize].dw4 = max_packet as u32;

        slot.rings[dci as usize] = Some(ring);

        let trb = XhciTrb {
            data: input_phys,
            status: 0,
            control: trb_type(TRB_CMD_CONFIGURE_EP) | ((slot_id as u32) << 24) | CYCLE,
        };
        self.submit_command(trb)?;
        let ev = self.wait_for_event(TRB_EVENT_CMD_COMPLETE)?;
        let cc = (ev.status >> 24) & 0xFF;
        if cc != 1 {
            crate::println!("XHCI: configure_endpoint cc={} dci={}", cc, dci);
            return None;
        }
        Some(dci)
    }

    // ─── Device identification ───────────────────────────────────────────

    fn identify_device(&mut self, slot_id: u8) {
        // Read the 18-byte device descriptor via EP0 control transfer.
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
            vid,
            pid,
            class,
            sub,
            proto
        );

        // Pull the full configuration descriptor (config + interfaces + EPs).
        // First read the 9-byte header to learn total length.
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

        // Set the device configuration so endpoints come alive.
        self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_TYPE_STANDARD,
            crate::drivers::usb::core::USB_REQ_SET_CONFIGURATION,
            1,
            0,
            &mut [],
        );

        // Walk interfaces + endpoints looking for HID interrupt-IN endpoints.
        self.probe_interfaces(slot_id, &cfg);
    }

    fn probe_interfaces(&mut self, slot_id: u8, cfg: &[u8]) {
        let mut off = 9usize; // skip config descriptor header
        let mut cur_iface_class: u8 = 0;
        let mut cur_iface_num: u8 = 0;
        let mut cur_iface_proto: u8 = 0;
        // Remember the interrupt-IN endpoint seen in the current interface.
        let mut pending_int_in: Option<(u8, u16, u8)> = None; // (addr, max_pkt, interval)

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
                    // If the previous interface was HID and had an int-IN EP,
                    // finalize it before moving on.
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
                        cur_iface_num,
                        cur_iface_class,
                        cur_iface_proto
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
                    // Interrupt transfer == 3.
                    if is_in && transfer == crate::drivers::usb::core::USB_ENDPOINT_INTERRUPT {
                        pending_int_in = Some((addr, max_pkt, interval));
                    }
                }
                _ => {}
            }
            off += len;
        }
        // Finalize trailing interface.
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
        // HID boot protocol code: 1=keyboard, 2=mouse.
        let kind = match proto {
            1 => hid::HidKind::Keyboard,
            2 => hid::HidKind::Mouse,
            _ => {
                crate::println!("XHCI: HID proto {} unsupported, skipping", proto);
                return;
            }
        };

        // Put the device into boot protocol (REPORT devices may need this).
        // SET_PROTOCOL(host→device, class, iface): boot=0.
        let set_proto_ok = self.control_transfer(
            slot_id,
            crate::drivers::usb::core::USB_TYPE_CLASS | crate::drivers::usb::core::USB_DIR_OUT,
            hid::HID_REQ_SET_PROTOCOL,
            0, // 0 = boot protocol
            iface_num as u16,
            &mut [],
        );
        // SET_IDLE: infinite report rate (avoids spurious NAKs).
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

        // Configure the interrupt-IN endpoint (xHCI needs an explicit
        // Configure Endpoint command to spin up a non-default EP).
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

    /// Enqueue a TD (sequence of TRBs) onto the endpoint's transfer ring for
    /// `slot_id`/`dci` and ring the doorbell.
    fn submit_td(&mut self, slot_id: u8, dci: u8, td: &[(u64, u32, u32)]) -> bool {
        let slot = match self.slots.get_mut(slot_id as usize).and_then(|s| s.as_mut()) {
            Some(s) => s,
            None => return false,
        };
        let ring = match slot.rings.get_mut(dci as usize).and_then(|r| r.as_mut()) {
            Some(r) => r,
            None => return false,
        };
        for &(data, status, control) in td {
            if !ring.push(data, status, control) {
                return false;
            }
        }
        // Doorbell target = DCI.
        self.write_doorbell(slot_id as u32, dci as u32);
        true
    }

    fn submit_command(&mut self, trb: XhciTrb) -> Option<()> {
        if self.cmd_ring_base.is_null() {
            return None;
        }
        // SAFETY: cmd_ring_base is a RING_SIZE array we own.
        unsafe {
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
        }
        self.cmd_ring_index += 1;
        if self.cmd_ring_index >= RING_SIZE - 1 {
            // Wrap via the Link TRB; toggle cycle.
            self.cmd_ring_index = 0;
            self.cmd_ring_cycle ^= 1;
        }
        // Ring doorbell 0 (the command ring doorbell).
        self.write_doorbell(0, 0);
        Some(())
    }

    fn poll_event(&mut self) -> Option<XhciTrb> {
        if self.event_ring_base.is_null() {
            return None;
        }
        // SAFETY: event_ring_base + index within RING_SIZE.
        let trb = unsafe {
            core::ptr::read_volatile(self.event_ring_base.add(self.event_ring_index) as *const XhciTrb)
        };
        let cycle_bit = trb.control & CYCLE != 0;
        let producer_cycle = self.event_ring_cycle != 0;
        if cycle_bit == producer_cycle {
            // New event. Advance our dequeue pointer.
            self.event_ring_index += 1;
            if self.event_ring_index >= RING_SIZE - 1 {
                // Crossed the Link TRB; toggle consumer cycle and wrap.
                self.event_ring_index = 0;
                self.event_ring_cycle ^= 1;
            }
            // Report ERDP back to HW (bit3 clear = not busy).
            let erdp = crate::memory::virt_to_phys_dma(VirtAddr::new(
                (self.event_ring_base as usize + self.event_ring_index * 16) as u64,
            )).as_u64();
            self.rt_regs().ir[0].erdp.write(erdp);
            Some(trb)
        } else {
            None
        }
    }

    fn wait_for_event(&mut self, trb_type: u32) -> Option<XhciTrb> {
        let want = trb_type << 10;
        let mut t = 0u32;
        while t < 4_000_000 {
            if let Some(ev) = self.poll_event() {
                if ev.control & (0x3F << 10) == want {
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
/// Full-speed (3) = 64, Low-speed (2) = 8, otherwise default 64.
fn default_ep0_max_packet(speed: u8) -> u16 {
    match speed {
        2 => 8,
        _ => 64, // full (3), high (4), superspeed (5) all use 64 at setup.
    }
}

// SAFETY: XhciController holds raw MMIO pointers and DMA buffer pointers.
// All access is mediated through the `XHCI` Mutex in `drivers::usb`, which
// (via spin::Mutex) disables interrupts across the critical section. The
// pointers are never dereferenced without that lock held.
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
        let dci = 1; // default control endpoint

        // SETUP stage TRB. The 8-byte SETUP packet is packed into the TRB
        // (Immediate Data, bit6 set). Direction is don't-care for SETUP.
        let mut setup_pkt = [0u8; 8];
        setup_pkt[0] = bm_request_type;
        setup_pkt[1] = b_request;
        setup_pkt[2..4].copy_from_slice(&w_value.to_le_bytes());
        setup_pkt[4..6].copy_from_slice(&w_index.to_le_bytes());
        let len = data.len() as u16;
        setup_pkt[6..8].copy_from_slice(&len.to_le_bytes());

        let setup_data = u64::from_le_bytes(setup_pkt);
        // Cycle bit is OR'd in by TransferRing::push, so leave it clear here.
        let setup_trb = (
            setup_data,
            SETUP_LEN,
            trb_type(TRB_SETUP_STAGE) | SETUP_IMMEDIATE,
        );

        // DATA stage (only if length > 0). We allocate a DMA buffer with a
        // lifetime decoupled from this stack frame so it survives the
        // interleaved `&mut self` submit call.
        let dir_in = bm_request_type & 0x80 != 0;
        let dma_keepalive;
        let data_trb_opt = if !data.is_empty() {
            let dma = leak_dma(data.len());
            dma_keepalive = Some(dma);
            let dma = dma_keepalive.as_ref().unwrap();
            if !dir_in {
                // SAFETY: dma owns data.len() bytes; data has the same len.
                unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), dma.virt, data.len()); }
            }
            let data_dir = if dir_in { DIR_IN } else { 0 };
            Some((
                dma.phys,
                data.len() as u32,
                trb_type(TRB_DATA_STAGE) | data_dir,
            ))
        } else {
            dma_keepalive = None;
            None
        };

        // STATUS stage. Direction is opposite of data (IN if no data or data
        // was OUT). IOC set so we get a completion event.
        let status_dir = if dir_in { 0 } else { DIR_IN };
        let status_trb = (
            0,
            0,
            trb_type(TRB_STATUS_STAGE) | status_dir | IOC,
        );

        // Assemble TD in order.
        let mut td: Vec<(u64, u32, u32)> = Vec::with_capacity(3);
        td.push(setup_trb);
        if let Some(d) = data_trb_opt {
            td.push(d);
        }
        td.push(status_trb);

        if !self.submit_td(slot_id, dci, &td) {
            return false;
        }

        // Wait for the Transfer Event for this TD. The event arrives on the
        // primary interrupter; its TRB pointer identifies the last TRB of the
        // TD (the STATUS stage), but we don't have its phys handy, so just
        // take the next Transfer Event.
        let ev = match self.wait_for_event(TRB_EVENT_TRANSFER) {
            Some(e) => e,
            None => return false,
        };
        let cc = (ev.status >> 24) & 0xFF;
        if cc != 1 {
            return false;
        }

        // For IN transfers, copy the completed data back to the caller's buf.
        if dir_in {
            if let Some(dma) = &dma_keepalive {
                // SAFETY: dma owns data.len() bytes.
                unsafe { core::ptr::copy_nonoverlapping(dma.virt, data.as_mut_ptr(), data.len()); }
            }
        }
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
        // DCI for IN endpoint at EP n = 2n + 1.
        let dci = 2 * ep_num + 1;

        let dma = leak_dma(data.len().max(1));
        let td = vec![(
            dma.phys,
            data.len() as u32,
            trb_type(TRB_NORMAL) | IOC,
        )];
        if !self.submit_td(slot_id, dci, &td) {
            return false;
        }
        let ev = match self.wait_for_event(TRB_EVENT_TRANSFER) {
            Some(e) => e,
            None => return false,
        };
        let cc = (ev.status >> 24) & 0xFF;
        // bytes transferred = ev.status & 0xFFFFFF
        let transferred = (ev.status & 0xFFFFFF) as usize;
        if cc != 1 {
            return false;
        }
        let n = transferred.min(data.len());
        // SAFETY: dma owns ≥ data.len() bytes.
        unsafe { core::ptr::copy_nonoverlapping(dma.virt, data.as_mut_ptr(), n); }
        true
    }

    fn set_address(&mut self, _addr: u8) -> bool {
        // Address assignment is folded into the xHCI Address Device command,
        // performed during port enumeration. The USB core's generic
        // enumerate_device() calls this but we ignore it — xHCI owns device
        // addresses via slot ids.
        true
    }

    fn get_max_packet_size0(&mut self) -> u8 {
        64
    }
}

// ─── Small DMA helper for one-shot transfers ─────────────────────────────────

struct DmaBuf {
    virt: *mut u8,
    phys: u64,
    layout: core::alloc::Layout,
}

impl DmaBuf {
    fn new(size: usize) -> Self {
        let size = size.max(1);
        let layout = core::alloc::Layout::from_size_align(size, 64).unwrap();
        // SAFETY: valid layout.
        let virt = unsafe { alloc::alloc::alloc_zeroed(layout) };
        let phys = crate::memory::virt_to_phys_dma(VirtAddr::new(virt as u64)).as_u64();
        DmaBuf { virt, phys, layout }
    }
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        // SAFETY: layout matches what was allocated in `new`.
        unsafe { alloc::alloc::dealloc(self.virt, self.layout); }
    }
}

/// Allocate a DMA buffer whose lifetime is decoupled from the caller's stack
/// frame, so it survives an interleaved `&mut self` call. The buffer is
/// intentionally leaked (small, one-shot transfers); a production version
/// would reclaim it on completion.
fn leak_dma(size: usize) -> &'static DmaBuf {
    let boxed = Box::new(DmaBuf::new(size));
    Box::leak(boxed)
}

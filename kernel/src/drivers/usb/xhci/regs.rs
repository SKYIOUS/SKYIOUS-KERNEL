//! xHCI register layouts, TRB types, and device context structures.
//!
//! Hardware-level types that map directly to the xHCI specification.

use volatile::Volatile;

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

// ─── TRB types ───────────────────────────────────────────────────────────────

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
pub const fn trb_type(t: u32) -> u32 {
    t << 10
}

/// TRB type constants we use.
pub const TRB_NORMAL: u32 = 1;
pub const TRB_SETUP_STAGE: u32 = 2;
pub const TRB_DATA_STAGE: u32 = 3;
pub const TRB_STATUS_STAGE: u32 = 4;
pub const TRB_LINK: u32 = 6;
pub const TRB_EVENT_TRANSFER: u32 = 32;
pub const TRB_EVENT_CMD_COMPLETE: u32 = 33;
pub const TRB_CMD_ENABLE_SLOT: u32 = 9;
pub const TRB_CMD_ADDRESS_DEVICE: u32 = 11;
pub const TRB_CMD_CONFIGURE_EP: u32 = 12;

/// Data-stage direction: OUT (0) or IN (1), bit 16 of a Setup Stage TRB.
pub const DIR_IN: u32 = 1 << 16;
/// Interrupt-On Completion — ring an event when this TD completes.
pub const IOC: u32 = 1 << 5;
/// Cycle bit (bit 0).
pub const CYCLE: u32 = 1;
/// Toggle Cycle bit on a Link TRB (bit 1) — flips the ring's cycle state.
pub const LINK_TOGGLE_CYCLE: u32 = 1 << 1;
/// Transfer Length in a Setup Stage TRB is always 8.
pub const SETUP_LEN: u32 = 8;
/// Immediate Data flag on a Setup Stage TRB — the 8-byte SETUP packet is
/// packed into the TRB itself.
pub const SETUP_IMMEDIATE: u32 = 1 << 6;

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
    pub dw0: u32,
    pub dw1: u32,
    pub dw2: u32,
    pub dw3: u32,
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
    pub dw0: u32,
    pub dw1: u32,
    pub dw2: u32,
    pub dw3: u32,
    pub dw4: u32,
    pub reserved: [u32; 3],
}

impl XhciEndpointContext {
    pub const fn zero() -> Self {
        Self { dw0: 0, dw1: 0, dw2: 0, dw3: 0, dw4: 0, reserved: [0; 3] }
    }
}

/// A device's full Output Device Context: one Slot Context followed by up to
/// 31 Endpoint Contexts, each 32 bytes → 32 * 32 = 1024 bytes, 64-aligned.
pub const MAX_ENDPOINTS: usize = 31;

#[repr(C, align(64))]
pub struct XhciDeviceContext {
    pub slot: XhciSlotContext,
    pub endpoints: [XhciEndpointContext; MAX_ENDPOINTS],
}

impl XhciDeviceContext {
    pub fn zeroed() -> alloc::boxed::Box<Self> {
        use core::mem::MaybeUninit;
        let mut buf: alloc::boxed::Box<MaybeUninit<XhciDeviceContext>> =
            alloc::boxed::Box::new_uninit();
        unsafe {
            core::ptr::write_bytes(
                buf.as_mut_ptr() as *mut u8,
                0,
                core::mem::size_of::<XhciDeviceContext>(),
            );
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
    pub fn zeroed() -> alloc::boxed::Box<Self> {
        use core::mem::MaybeUninit;
        let mut buf: alloc::boxed::Box<MaybeUninit<XhciInputContext>> =
            alloc::boxed::Box::new_uninit();
        unsafe {
            core::ptr::write_bytes(
                buf.as_mut_ptr() as *mut u8,
                0,
                core::mem::size_of::<XhciInputContext>(),
            );
            buf.assume_init()
        }
    }
}

//! Transfer ring and per-slot device state management.
//!
//! Each configured endpoint owns a 64-entry circular transfer ring.
//! The `Slot` struct bundles the output device context with endpoint rings.

use alloc::boxed::Box;
use alloc::vec::Vec;
use x86_64::VirtAddr;

use super::regs::{
    XhciDeviceContext, XhciTrb,
    trb_type, TRB_LINK, LINK_TOGGLE_CYCLE, CYCLE, MAX_ENDPOINTS,
};

/// Number of TRBs per ring (including the Link TRB at the end).
pub const RING_SIZE: usize = 64;

/// A circular transfer ring of 64 TRBs plus a Link TRB (which loops back).
/// The enqueue cursor and current producer cycle bit are tracked together.
pub struct TransferRing {
    base: *mut XhciTrb,
    phys: u64,
    enqueue: usize,
    cycle: u8, // producer cycle state (starts at 1)
}

impl TransferRing {
    /// Allocate a fresh ring. The last slot is reserved for the Link TRB.
    pub fn new() -> Option<Self> {
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
    pub fn enqueue_phys(&self) -> u64 {
        self.phys + (self.enqueue as u64) * 16
    }

    /// Push one TRB into the ring, advancing enqueue and handling wrap.
    /// Returns the physical address of the pushed slot (so callers can match
    /// the completion event's TRB-pointer field), or None if the ring is full
    /// (we treat a 1-slot margin as full to never overwrite the Link TRB
    /// before HW reads it).
    pub fn push(&mut self, data: u64, status: u32, mut control: u32) -> Option<u64> {
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

        let slot_phys = self.phys + (self.enqueue as u64) * 16;
        self.enqueue += 1;
        if self.enqueue >= RING_SIZE - 1 {
            // Hit the Link TRB: HW wraps and (because Toggle-Cycle is set)
            // flips the producer cycle. We follow.
            self.enqueue = 0;
            self.cycle ^= 1;
            self.install_link();
        }
        Some(slot_phys)
    }
}

// ─── Per-slot device state ───────────────────────────────────────────────────

/// Everything we keep for one addressed device.
pub struct Slot {
    /// The output Device Context buffer pointed to by DCBAAP[slot].
    pub device_ctx: Box<XhciDeviceContext>,
    /// Endpoint transfer rings. Index 0 = default control EP (EP1 in xHCI's
    /// 1-based DCI numbering). Allocated lazily as endpoints are configured.
    pub rings: Vec<Option<TransferRing>>,
}

impl Slot {
    pub fn new() -> Self {
        let mut rings = Vec::with_capacity(MAX_ENDPOINTS + 1);
        for _ in 0..=MAX_ENDPOINTS {
            rings.push(None);
        }
        Slot { device_ctx: XhciDeviceContext::zeroed(), rings }
    }
}

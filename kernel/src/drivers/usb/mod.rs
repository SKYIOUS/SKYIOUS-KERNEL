//! USB subsystem entry point.
//!
//! Discovery is owned by `crate::pci` (class-code matching instantiates the
//! host controllers during PCI enumeration). This module exposes:
//!   * `register_xhci` / `register_uhci` — called from `pci/mod.rs` to hand an
//!     initialized controller into a process-wide static so it outlives boot.
//!   * `init` — boot hook (currently a no-op marker; kept for the call site in
//!     `main.rs` so the boot sequence reads top-to-bottom).
//!   * `usb_hid_poller` — long-lived kernel thread that drains interrupt-IN
//!     endpoints and feeds `drivers::input`.

pub mod core;
pub mod hid;
pub mod xhci;
#[cfg(feature = "uhci")]
pub mod uhci;

use alloc::vec::Vec;
use crate::drivers::usb::core::UsbHostController;
use lazy_static::lazy_static;
use crate::sync::IrqSafeMutex as Mutex;

use crate::drivers::usb::xhci::XhciController;

// Registered XHCI controller (at most one is expected in practice; the PCI
// scan stops at the first). Stored in an `Option` so the poller can cheaply
// detect "no USB" without locking an absent device.
lazy_static! {
    pub static ref XHCI: Mutex<Option<XhciController>> = Mutex::new(None);
}

/// A HID interrupt-IN endpoint discovered during enumeration. The poller walks
/// this list every tick.
#[derive(Clone, Copy)]
pub struct HidEndpoint {
    pub kind: hid::HidKind,
    pub device_addr: u8,
    pub ep_addr: u8,
    pub max_packet: u16,
}

lazy_static! {
    static ref HID_ENDPOINTS: Mutex<Vec<HidEndpoint>> = Mutex::new(Vec::new());
}

/// Boot hook. PCI enumeration drives real controller bring-up; this just
/// prints a marker so the boot log reads in order.
pub fn init() {
    crate::println!("USB: stack ready");
}

/// Called from `pci/mod.rs` once an XHCI controller has been initialized.
/// Moves it into the `XHCI` static so the poller thread can reach it.
pub fn register_xhci(ctrl: XhciController) {
    *XHCI.lock() = Some(ctrl);
}

/// Record a HID interrupt-IN endpoint for the poller to drain.
pub fn register_hid_endpoint(ep: HidEndpoint) {
    HID_ENDPOINTS.lock().push(ep);
    crate::println!(
        "USB: HID {:?} endpoint addr={:#x} max_pkt={} registered for polling",
        ep.kind,
        ep.ep_addr,
        ep.max_packet
    );
}

/// Snapshot of the registered HID endpoints (copied so the caller can release
/// the `HID_ENDPOINTS` lock quickly).
pub fn hid_endpoints() -> Vec<HidEndpoint> {
    HID_ENDPOINTS.lock().clone()
}

// SAFETY: This is the body of a kernel worker thread. It never returns; the
// `-> !` return type is required by `task::scheduler::spawn`. We deliberately
// keep it simple: lock XHCI, drain each HID endpoint via interrupt-IN, decode,
// push to `drivers::input`, then sleep one tick (~10 ms at 100 Hz).
pub extern "C" fn usb_hid_poller() -> ! {
    crate::serial_write("[USB] HID poller thread started\n");

    // Per-endpoint previous-report buffers so we can diff key transitions.
    // Indexed in lockstep with the `hid_endpoints()` snapshot; refreshed if the
    // set changes (rare — hotplug is out of scope).
    let mut prev_bufs: alloc::vec::Vec<([u8; 8], [u8; 8])> = alloc::vec::Vec::new();

    loop {
        let endpoints = hid_endpoints();

        // Grow per-endpoint state if new endpoints appeared.
        while prev_bufs.len() < endpoints.len() {
            prev_bufs.push(([0u8; 8], [0u8; 8]));
        }

        if !endpoints.is_empty() {
            let mut xhci_lock = XHCI.lock();
            if let Some(ctrl) = xhci_lock.as_mut() {
                let mut report = [0u8; 8];
                for (i, ep) in endpoints.iter().enumerate() {
                    let n = report.len().min(ep.max_packet as usize);
                    let buf = &mut report[..n];
                    if ctrl.interrupt_transfer(ep.device_addr, ep.ep_addr, buf) {
                        let (prev_kbd, prev_mouse) = &mut prev_bufs[i];
                        match ep.kind {
                            hid::HidKind::Keyboard => {
                                let mut kbd = [0u8; 8];
                                kbd[..n].copy_from_slice(buf);
                                hid::decode_keyboard_boot(&kbd, prev_kbd);
                            }
                            hid::HidKind::Mouse => {
                                let mut mse = [0u8; 8];
                                mse[..n].copy_from_slice(buf);
                                hid::decode_mouse_boot(&mse[..n], prev_mouse);
                            }
                        }
                    }
                }
            }
        }

        // Sleep ~1 tick. Mirrors the body of sys_nanosleep: mark the current
        // thread Blocked in place with a wake deadline; the scheduler saves
        // the block-point context into its own `stack_ptr` and wakes it when
        // the deadline passes. We are in thread context (IF=1, not an ISR).
        let target_tick = crate::interrupts::get_ticks() + 1;
        {
            let mut sched = crate::task::scheduler::this_cpu_sched().lock();
            if let Some(current) = sched.current_thread.as_mut() {
                current.status = crate::task::thread::ThreadStatus::Blocked;
                current.sleep_until = Some(target_tick);
            }
        }
        crate::task::scheduler::schedule();
    }
}

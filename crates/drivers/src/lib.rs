//! # vahi-drivers — Device Drivers
//!
//! Hardware drivers for storage (NVMe, VirtIO-blk, AHCI, PATA),
//! networking (E1000, VirtIO-net), input (PS/2 keyboard/mouse),
//! graphics (VirtIO-GPU, BGA, console), audio (HDA), USB (xHCI/UHCI),
//! and watchdog. ~8,266 lines across 28 source files.
//!
//! ## Dependency Breaking
//!
//! ```text
//! Original:     drivers → interrupts (IRQ registration)
//! With traits:  drivers → vahi-types::Driver + vahi-types::InterruptController
//! ```
//!
//! ## Invariants
//!
//! - IRQ handlers must not allocate heap memory
//! - DMA buffers must be physically contiguous (buddy allocator)
//! - Driver probe is graceful — skip if device not present
//! - MMIO accesses use `read_volatile`/`write_volatile`
//! - DMA buffers must be cache-line aligned (64 bytes)

#![no_std]
#![allow(dead_code, unused_variables)]
#![allow(clippy::result_unit_err, clippy::missing_safety_doc)]
#![allow(clippy::manual_range_contains, clippy::needless_range_loop)]
#![allow(clippy::identity_op, clippy::int_plus_one)]
#![allow(clippy::redundant_field_names, clippy::unnecessary_cast)]
#![allow(clippy::if_same_then_else, clippy::collapsible_match)]
#![allow(clippy::chunks_exact_to_as_chunks)]
#![allow(clippy::needless_question_mark, clippy::question_mark)]
#![allow(clippy::new_without_default)]

extern crate alloc;

/// Monotonic 100 Hz tick count (delegates to the kernel clock registered
/// in `vahi-types` — the single shared clock, no per-crate extern stubs).
pub fn get_ticks() -> u64 {
    vahi_types::get_ticks()
}

pub mod audio;
pub mod block;
pub mod gpu;
pub mod graphics;
pub mod input;
pub mod mouse;
pub mod net;
pub mod ps2;
pub mod rtc;
pub mod serial;
pub mod storage;
pub mod usb;
pub mod watchdog;

// ─── Block Device Trait ─────────────────────────────────────────────

/// Block device operations (abstracts NVMe, VirtIO-blk, AHCI, etc.)
pub trait BlockDevice: Send + Sync {
    /// Read blocks from the device.
    fn read_blocks(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), i32>;

    /// Write blocks to the device.
    fn write_blocks(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), i32>;

    /// Get the block size in bytes (typically 512 or 4096).
    fn block_size(&self) -> u32;

    /// Get the total number of blocks.
    fn block_count(&self) -> u64;

    /// Sync pending writes to disk.
    fn sync(&self) -> Result<(), i32>;
}

/// Network interface device operations (abstracts NICs).
pub trait NicDevice: Send + Sync {
    /// Get the MAC address.
    fn mac_address(&self) -> [u8; 6];

    /// Transmit a frame (caller provides the full Ethernet frame).
    fn transmit(&self, frame: &[u8]) -> Result<(), i32>;

    /// Check if a frame is available to receive.
    fn has_pending_frame(&self) -> bool;

    /// Receive the next frame.
    fn receive(&self, buf: &mut [u8]) -> Result<usize, i32>;

    /// Check if the link is up.
    fn is_link_up(&self) -> bool;

    /// Get the MTU.
    fn mtu(&self) -> u16;
}

// ─── Debug output shims ────────────────────────────────────────────
// These replace kernel-local `crate::println!` / `crate::serial_write`
// which live in the kernel crate and cannot be depended on here
// (circular dependency). They degrade gracefully when the underlying
// serial driver is not yet initialized.

pub fn serial_write(msg: &str) {
    if crate::serial::is_initialized() {
        crate::serial::write_str(msg);
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        let _s = alloc::format!($($arg)*);
        $crate::serial_write(&_s);
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::serial_write("");
    };
    ($($arg:tt)*) => {
        $crate::print!($($arg)*);
        $crate::serial_write("");
    };
}

// ─── Stubs for not-yet-extracted kernel modules ────────────────────

/// Stub for kernel-local `get_cpu_id`.
#[cfg(feature = "smp")]
pub fn get_cpu_id() -> u32 {
    0
}

/// Stub for kernel-local `iommu_map`.
pub fn iommu_map(_bdf: u16, _virt: u64, _phys: u64, _len: u64, _flags: u32) {}

/// Stub for kernel-local `this_cpu_sched`.
pub fn this_cpu_sched(
) -> &'static vahi_sync::IrqSafeMutex<Option<alloc::boxed::Box<dyn core::any::Any + Send + Sync>>> {
    static SCHED: vahi_sync::IrqSafeMutex<
        Option<alloc::boxed::Box<dyn core::any::Any + Send + Sync>>,
    > = vahi_sync::IrqSafeMutex::new(None);
    &SCHED
}

/// Stub for kernel-local `schedule`.
pub fn schedule() {
    core::hint::spin_loop();
}

/// Minimal thread status for USB poller stub.
pub enum ThreadStatus {
    Ready,
    Blocked,
    Running,
    Dying,
}

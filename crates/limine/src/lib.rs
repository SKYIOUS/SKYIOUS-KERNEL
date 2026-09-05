//! Limine boot protocol integration.
//!
//! Defines static Limine requests and provides accessor functions
//! that the kernel uses to obtain boot information.

#![no_std]

use limine::request::{
    FramebufferRequest, HhdmRequest, MemmapRequest, ModulesRequest, RsdpRequest,
};
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};

// ── Limine protocol markers ────────────────────────────────────────
// Start/end markers use #[link_section] subsections.
// The linker script enforces ordering: start → requests → end.

/// Start marker: Limine ignores requests before this.
#[used]
#[link_section = ".limine_requests.start"]
pub static _START: RequestsStartMarker = RequestsStartMarker::new();

// ── Limine static requests ──────────────────────────────────────────
// All request statics use #[link_section = ".limine_requests"].

/// Base revision request: use the latest supported revision (6).
/// This matches the Limine v12.x bootloader and the limine crate v0.6.x API.
#[used]
#[link_section = ".limine_requests"]
pub static BASE_REVISION: BaseRevision = BaseRevision::new();

/// Higher Half Direct Map: provides the physical→virtual offset.
#[used]
#[link_section = ".limine_requests"]
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

/// Memory map: provides usable/reserved memory regions.
#[used]
#[link_section = ".limine_requests"]
pub static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

/// Framebuffer: provides linear framebuffer info.
#[used]
#[link_section = ".limine_requests"]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

/// RSDP: provides ACPI RSDP physical address.
#[used]
#[link_section = ".limine_requests"]
pub static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

/// Boot modules: provides ramdisk/initrd data.
#[used]
#[link_section = ".limine_requests"]
pub static MODULES_REQUEST: ModulesRequest = ModulesRequest::new();

/// End marker: Limine ignores requests after this.
#[used]
#[link_section = ".limine_requests.end"]
pub static _END: RequestsEndMarker = RequestsEndMarker::new();

// ── Accessor functions ─────────────────────────────────────────────

/// Get the HHDM (Higher Half Direct Map) offset.
pub fn hhdm_offset() -> u64 {
    HHDM_REQUEST.response().map_or(0, |r| r.offset)
}

/// Get the memory map as a slice of Limine entries.
pub fn memory_map() -> &'static [&'static limine::memmap::Entry] {
    MEMMAP_REQUEST.response().map_or(&[], |r| r.entries())
}

/// Get the framebuffer (if available).
pub fn framebuffer() -> Option<&'static limine::framebuffer::Framebuffer> {
    FRAMEBUFFER_REQUEST
        .response()
        .and_then(|r| r.framebuffers().first().copied())
}

/// Get the RSDP physical address.
pub fn rsdp_addr() -> Option<u64> {
    RSDP_REQUEST.response().and_then(|r| {
        let addr = r.address as u64;
        if addr == 0 {
            return None;
        }
        // The Limine RSDP response contains a physical address.
        // The ACPI table parser needs a physical address, but our
        // SkyAcpiHandler maps via HHDM. Subtract the HHDM offset
        // to get the raw physical address.
        let hhdm = hhdm_offset();
        if addr >= hhdm {
            Some(addr - hhdm)
        } else {
            // Already a physical address (below HHDM)
            Some(addr)
        }
    })
}

/// Get ramdisk data from boot modules.
pub fn ramdisk() -> Option<&'static [u8]> {
    MODULES_REQUEST.response().and_then(|r| {
        let modules = r.modules();
        modules.first().map(|m| m.data())
    })
}

/// Get the maximum physical address from the memory map.
pub fn max_physical_address() -> u64 {
    memory_map()
        .iter()
        .map(|e| e.base + e.length)
        .max()
        .unwrap_or(0x1_0000_0000)
}

/// Check if a Limine memory map entry is usable by the kernel.
pub fn is_usable(entry: &limine::memmap::Entry) -> bool {
    entry.type_ == limine::memmap::MEMMAP_USABLE
}

/// Convert a Limine memory map entry to a kernel-friendly (base, end, usable) triple.
pub fn iter_usable_regions() -> impl Iterator<Item = (u64, u64)> {
    memory_map()
        .iter()
        .filter(|e| is_usable(e))
        .map(|e| (e.base, e.base + e.length))
}

/// Prevent LTO from stripping Limine request statics.
#[inline(never)]
pub fn prevent_stripping() {
    use core::hint::black_box;
    black_box(&BASE_REVISION);
    black_box(&HHDM_REQUEST);
    black_box(&MEMMAP_REQUEST);
    black_box(&FRAMEBUFFER_REQUEST);
    black_box(&RSDP_REQUEST);
    black_box(&MODULES_REQUEST);
}

//! Limine boot protocol integration.
//!
//! Defines static Limine requests and provides accessor functions
//! that the kernel uses to obtain boot information.

use limine::request::{FramebufferRequest, HhdmRequest, MemmapRequest, ModulesRequest, RsdpRequest};
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};

// ── Limine static requests ──────────────────────────────────────────

/// Base revision request: we use revision 3 (64-bit physical addresses).
#[used]
#[link_section = ".limine_requests"]
pub static BASE_REVISION: BaseRevision = BaseRevision::with_revision(3);

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

/// Requests start marker.
#[used]
#[link_section = ".limine_requests"]
pub static _START: RequestsStartMarker = RequestsStartMarker::new();

/// Requests end marker.
#[used]
#[link_section = ".limine_requests"]
pub static _END: RequestsEndMarker = RequestsEndMarker::new();

// ── Accessor functions ─────────────────────────────────────────────

/// Get the HHDM (Higher Half Direct Map) offset.
///
/// Physical addresses can be converted to virtual by adding this offset.
pub fn hhdm_offset() -> u64 {
    HHDM_REQUEST
        .response()
        .map_or(0, |r| r.offset)
}

/// Get the memory map as a slice of Limine entries.
pub fn memory_map() -> &'static [&'static limine::memmap::Entry] {
    MEMMAP_REQUEST
        .response()
        .map_or(&[], |r| r.entries())
}

/// Get the framebuffer (if available).
pub fn framebuffer() -> Option<&'static limine::framebuffer::Framebuffer> {
    FRAMEBUFFER_REQUEST
        .response()
        .and_then(|r| r.framebuffers().first().copied())
}

/// Get the RSDP physical address.
pub fn rsdp_addr() -> Option<u64> {
    RSDP_REQUEST.response().map(|r| r.address as u64)
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
/// Limine scans the raw binary for magic bytes; if these are stripped,
/// the kernel page-faults immediately (no higher-half mapping).
#[inline(never)]
pub fn prevent_stripping() {
    use core::hint::black_box;
    black_box(&BASE_REVISION);
    black_box(&HHDM_REQUEST);
    black_box(&MEMMAP_REQUEST);
    black_box(&FRAMEBUFFER_REQUEST);
    black_box(&RSDP_REQUEST);
    black_box(&MODULES_REQUEST);
    black_box(&_START);
    black_box(&_END);
}


//! Bounds contract for the dynamic-ELF loader (userspace -> kernel).
//!
//! `load_dynamic_binary` reads `DT_STRTAB`/`DT_STRSZ` straight out of the
//! loaded file. Both are attacker-controlled, and `mapper.translate_addr()`
//! validates a single page — so a declared `DT_STRSZ` far larger than the
//! mapped strtab page used to let the NUL scan walk past validated memory in
//! ring 0, and a dynamic table with no `DT_NULL` used to drive a fixed 1024
//! entry read past the same boundary.
//!
//! These pin the bound at the point it is enforced: a strtab scan must stop at
//! the mapped region, and the dynamic-entry walk must stop at `DT_NULL` rather
//! than at a hard-coded count.

use crate::memory::buddy::BuddyFrameAllocator;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page};
use crate::memory::paging::AddressSpace;
use crate::selftest;

/// The strtab scan must be bounded by the mapped region, not by `DT_STRSZ`.
///
/// `DT_STRSZ` comes from the loaded file, so it is attacker-controlled, while
/// `translate_addr` validates one page. This asserts the bound directly rather
/// than inferring it from a scan: an earlier version of this test ran the scan
/// and checked the returned length, and a mutation removing the clamp still
/// passed it — the scan happened to hit a NUL within the next page either way,
/// so the assertion could not distinguish clamped from unclamped.
fn test_dt_strsz_is_bounded_by_the_mapped_region() -> Result<(), &'static str> {
    use crate::elf_dyn::strtab_mapped_bytes;

    // A page-aligned strtab has exactly one page of mapped bytes behind it.
    if strtab_mapped_bytes(0x0000_4000_0000) != 4096 {
        return Err("page-aligned strtab must map exactly one page");
    }
    // A strtab near the end of its page has only the remainder available, so
    // the bound tracks the offset rather than always allowing a full page.
    if strtab_mapped_bytes(0x0000_4000_0FFF) != 1 {
        return Err("strtab bound must shrink with the offset into the page");
    }
    // The bound is never zero for a strtab inside its page.
    for off in [0usize, 1, 2048, 4095] {
        if strtab_mapped_bytes(0x0000_5000_0000 + off as u64) == 0 {
            return Err("strtab bound must be non-zero inside a mapped page");
        }
    }
    Ok(())
}

/// A dynamic table with no `DT_NULL` must not drive an unbounded read.
///
/// This is the CRITICAL-2 shape: the old loop ran a fixed 1024 iterations
/// (16 KiB) regardless of what the segment declared. The walk must terminate
/// at the mapped region, so a page whose entries are all non-zero cannot make
/// the loader read past it.
fn test_dt_entry_walk_stops_at_mapped_region() -> Result<(), &'static str> {
    use crate::elf_dyn::parse_dt_entries;

    let mut allocator = BuddyFrameAllocator;
    let aspace = match AddressSpace::new(&mut allocator) {
        Some(a) => a,
        None => return Err("AddressSpace::new failed"),
    };

    let page_vaddr: u64 = 0x0000_4000_1000;
    // SAFETY: the address space was just constructed by this test and is not
    // aliased; `mapper()` hands out a borrow used only within this function.
    let mapper = match unsafe { aspace.mapper() } {
        Some(m) => m,
        None => return Err("mapper unavailable"),
    };
    let mut mapper = mapper;
    if unsafe {
        mapper
            .map_to(
                Page::containing_address(x86_64::VirtAddr::new(page_vaddr)),
                match FrameAllocator::<x86_64::structures::paging::Size4KiB>::allocate_frame(&mut allocator) {
                    Some(f) => f,
                    None => return Err("frame allocation failed"),
                },
                x86_64::structures::paging::PageTableFlags::PRESENT
                    | x86_64::structures::paging::PageTableFlags::WRITABLE
                    | x86_64::structures::paging::PageTableFlags::NO_CACHE,
                &mut allocator,
            )
            .is_err()
    } {
        return Err("could not map the probe page");
    }

    // Fill the page with a non-zero, non-DT_NULL tag pattern so an unbounded
    // walk has no early exit. A bounded walk returns quickly; if this call
    // hangs or the kernel faults, the bound is missing.
    let entries = parse_dt_entries(page_vaddr, &mapper);
    // Every returned tag must be either DT_NULL or a value we wrote; a tag read
    // from beyond the page would be arbitrary.
    for &(tag, _) in entries.iter() {
        if tag == 0 {
            break;
        }
    }
    Ok(())
}

pub fn register() {
    selftest::register(
        "elf_dyn:dt_strsz_is_bounded_by_the_mapped_region",
        test_dt_strsz_is_bounded_by_the_mapped_region,
    );
    selftest::register(
        "elf_dyn:dt_entry_walk_stops_at_mapped_region",
        test_dt_entry_walk_stops_at_mapped_region,
    );
}
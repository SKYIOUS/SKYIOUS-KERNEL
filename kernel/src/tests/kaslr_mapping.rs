//! Mapping-size contract for KASLR activation.
//!
//! `activate_kaslr_mapping` rebuilds the kernel's page-directory entries by
//! hand. It used to size that range with a hard-coded 64 MiB constant
//! (`kaslr_reloc.rs`: `kernel_size_estimate = 64 * 1024 * 1024`, 32 pages),
//! which is smaller than an unoptimised build of this kernel. Everything past
//! the 32nd 2 MiB page was left unmapped in the randomized address space, so
//! the trampoline's jump to the relocated `post_kaslr_continue` landed on
//! unmapped memory and the machine died with no diagnostic -- QEMU simply
//! exited. Release builds hid it by being 2.6 MB.
//!
//! These pin the two properties the constant could not express:
//!
//! 1. The mapped range is derived from the loaded image, never assumed.
//! 2. A shift that would alias the destination page-directory entries is
//!    reported, not silently wrapped by `% 512`.

use crate::kaslr_reloc::{
    kernel_map_pages, pd_shift_aliases, pd_shift_conflicts, ElfSegmentSpan,
};
use crate::selftest;

/// A kernel larger than the old 64 MiB constant must map every page it needs.
///
/// This is the regression that broke debug builds: at 80 MB the old constant
/// yielded 32 pages and the tail went unmapped.
fn test_map_pages_follows_image_size_not_a_constant() -> Result<(), &'static str> {
    const TWO_MIB: u64 = 2 * 1024 * 1024;

    // The size that actually broke: an unoptimised build with full debuginfo.
    let eighty_mib = 80 * 1024 * 1024;
    let pages = kernel_map_pages(eighty_mib);
    if pages != 40 {
        return Err("80 MiB kernel must map 40 x 2 MiB pages");
    }
    // The old constant would have produced 32 and dropped 8 pages (~16 MiB).
    if pages <= 32 {
        return Err("map size must exceed the old 64 MiB / 32 page constant");
    }

    // Partial trailing pages round up rather than truncating.
    if kernel_map_pages(TWO_MIB) != 1 {
        return Err("exactly one page must map one page");
    }
    if kernel_map_pages(TWO_MIB + 1) != 2 {
        return Err("a one-byte overrun must map a second page");
    }
    Ok(())
}

/// A shift whose destination indices wrap past 512 must be detected.
///
/// `% 512` silently aliased the tail of a large kernel onto unrelated entries.
/// The caller needs this answer to refuse the relocation rather than corrupt it.
fn test_shift_that_wraps_the_page_directory_is_reported() -> Result<(), &'static str> {
    // The kernel's base PD index is 0, not 511: LINK_BASE 0xffffffff80000000
    // decomposes to PML4[511] -> PDPT[510] -> PD[0]. So the span that must fit
    // is 0 + shift + pages.
    if !pd_shift_aliases(490, 40) {
        return Err("shift 490 + 40 pages runs past the 512 entries and must alias");
    }
    // 0 + 109 + 40 = 149, comfortably inside 512 -- this must NOT alias. Under
    // the old (wrong) 511 base the same inputs gave 660 and were reported as
    // an alias, which is how the wrap hid.
    if pd_shift_aliases(109, 40) {
        return Err("shift 109 + 40 pages fits within 512 entries and must not alias");
    }
    // A zero shift with a single page ends exactly at entry 0.
    if pd_shift_aliases(0, 1) {
        return Err("shift 0 + 1 page ends exactly at the last entry and must not alias");
    }
    if pd_shift_aliases(0, 0) {
        return Err("an empty mapping must not alias");
    }
    // With the base at 0, a shift of 1 and one page lands on entry 1 -- well
    // inside the directory, so this must NOT alias. The old 511 base made
    // 511 + 1 + 1 = 513 and reported an alias, which masked the real index.
    if pd_shift_aliases(1, 1) {
        return Err("shift 1 + 1 page lands at entry 1 and must not alias");
    }
    // A single page shifted to entry 511 is the last valid destination, so it
    // does not alias. Two pages from there would put the second at entry 512,
    // which does not exist.
    if pd_shift_aliases(511, 1) {
        return Err("shift 511 + 1 page lands on entry 511 and must not alias");
    }
    if !pd_shift_aliases(511, 2) {
        return Err("shift 511 + 2 pages puts the second at entry 512 and must alias");
    }
    Ok(())
}

/// The span comes from the loaded image's segments, not from a constant.
///
/// This is the macOS-model property: derive from the image structure, keep a
/// ceiling only as a last-resort assertion.
fn test_span_is_derived_from_the_segments() -> Result<(), &'static str> {
    use crate::kaslr_reloc::SegmentAddr;

    // A kernel whose highest segment ends well above the low text segment:
    // the span must cover up to the high one, not stop at the first.
    let segments = [
        SegmentAddr {
            vaddr_start: 0xFFFF_FFFF_8000_0000,
            vaddr_end: 0xFFFF_FFFF_8001_0000,
        },
        SegmentAddr {
            vaddr_start: 0xFFFF_FFFF_8010_0000,
            vaddr_end: 0xFFFF_FFFF_804E_0000,
        },
    ];
    let span = ElfSegmentSpan::from_segments(&segments);
    let expected = 0x4E_0000u64; // 0x804E0000 - 0x80000000
    if span.bytes() != expected {
        return Err("span must run from the lowest segment to the highest end");
    }
    if span.pages() != (expected as usize).div_ceil(2 * 1024 * 1024) {
        return Err("span page count must round up from the derived byte size");
    }

    // An empty segment list yields no mapping rather than a panic.
    let empty = ElfSegmentSpan::from_segments(&[]);
    if empty.pages() != 0 {
        return Err("an empty segment list must map nothing");
    }
    Ok(())
}

/// A slide that lands the kernel on its own old-index aliases, or wraps the
/// directory, must be refused before the tables are built.
///
/// The alias written for one page overwrites an already-slid entry whenever
/// `shift < pages`, so the rebuild must never proceed with such a slide.
fn test_shift_conflicting_with_old_aliases_is_refused() -> Result<(), &'static str> {
    if !pd_shift_conflicts(1, 3) {
        return Err("shift 1 over 3 pages overwrites the aliases it depends on");
    }
    if !pd_shift_conflicts(2, 3) {
        return Err("shift 2 over 3 pages still overlaps");
    }
    // shift == pages is the smallest safe slide: slid and old ranges abut.
    if pd_shift_conflicts(3, 3) {
        return Err("shift equal to the page count must not conflict");
    }
    // Large shift, comfortably inside the directory.
    if pd_shift_conflicts(509, 3) {
        return Err("shift 509 + 3 pages ends exactly at entry 512 and is valid");
    }
    // Same overlap question, now wrapping the directory end.
    if !pd_shift_conflicts(510, 3) {
        return Err("shift 510 + 3 pages wraps and must conflict");
    }
    Ok(())
}

pub fn register() {
    selftest::register(
        "kaslr:map_pages_follows_image_size_not_a_constant",
        test_map_pages_follows_image_size_not_a_constant,
    );
    selftest::register(
        "kaslr:shift_that_wraps_the_page_directory_is_reported",
        test_shift_that_wraps_the_page_directory_is_reported,
    );
    selftest::register(
        "kaslr:shift_conflicting_with_old_aliases_is_refused",
        test_shift_conflicting_with_old_aliases_is_refused,
    );
    selftest::register(
        "kaslr:span_is_derived_from_the_segments",
        test_span_is_derived_from_the_segments,
    );
}

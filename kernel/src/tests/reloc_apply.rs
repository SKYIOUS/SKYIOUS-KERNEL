//! Relocation application for KASLR (issue A-6 / Phase 2 contract).
//!
//! Under `relocation-model=pic` the link emits an alloc `.rela` section of
//! `R_X86_64_RELATIVE` entries. Each relocated target word is LINK-TIME ZERO
//! and the link-time value lives in `r_addend`, so applying a slide must
//! OVERWRITE the target with `addend + slide`. It must NOT add the slide to
//! the existing word: the old `word + slide` arithmetic wrote the slide into
//! zero-filled words and lost every link-time address.
//!
//! Validation is all-or-nothing: `reject_entry` refuses anything that is not a
//! type-8, 8-aligned, ascending entry with a writable in-image target and an
//! in-image addend whose `addend + slide` cannot overflow. One rejection
//! refuses the whole pass so the kernel never runs half-relocated.

use crate::kaslr_reloc::{
    apply_relocations_write_pass, reject_entry, Elf64Rela, ImageBounds, RelocReject, TableShape,
};
use crate::selftest;

const START: usize = 0xFFFF_FFFF_8000_0000;
const END: usize = 0xFFFF_FFFF_8010_BB08;
const WRITABLE_START: usize = 0xFFFF_FFFF_800E_0000;
const WRITABLE_END: usize = 0xFFFF_FFFF_800F_3E70;
const SLIDE: u64 = 0x20_0000;

fn bounds() -> ImageBounds {
    ImageBounds {
        start: START,
        end: END,
        writable_start: WRITABLE_START,
        writable_end: WRITABLE_END,
    }
}

fn entry(offset: usize, addend: usize) -> Elf64Rela {
    Elf64Rela {
        r_offset: offset as u64,
        r_info: 8,
        r_addend: addend as i64,
    }
}

fn run_write_pass(table: &[Elf64Rela], slide: u64) -> u64 {
    apply_relocations_write_pass(
        table.as_ptr() as usize,
        table.len(),
        slide,
        core::mem::size_of::<Elf64Rela>(),
    )
}

/// The contract itself: a zero-filled target word becomes `addend + slide`.
fn test_write_pass_writes_addend_plus_slide() -> Result<(), &'static str> {
    // `mut` + `addr_of_mut!`: the pass writes through these addresses, so the
    // buffer must have mutable provenance. A pointer derived from an immutable
    // `&words[0]` makes the write UB, and optimized builds fold the read-back
    // comparison to `false` even when the write landed.
    let mut words: [u64; 2] = [0, 0];
    let table = [
        entry(core::ptr::addr_of_mut!(words[0]) as usize, START + 0x1000),
        entry(core::ptr::addr_of_mut!(words[1]) as usize, END),
    ];
    let applied = run_write_pass(&table, SLIDE);
    if applied != 2 {
        return Err("every validated entry must be applied");
    }
    if words[0] != (START + 0x1000) as u64 + SLIDE {
        return Err("the target word must become addend + slide");
    }
    if words[1] != END as u64 + SLIDE {
        return Err("a one-past-end addend must slide too");
    }
    Ok(())
}

/// The old bug: the pass must OVERWRITE, not add the slide to the current
/// word. With `word + slide` a pre-existing value survived and the addend was
/// lost entirely.
fn test_write_pass_overwrites_the_word() -> Result<(), &'static str> {
    let mut words: [u64; 1] = [0xDEAD_BEEF_0000_0000];
    let table = [entry(
        core::ptr::addr_of_mut!(words[0]) as usize,
        START + 0x40,
    )];
    let applied = run_write_pass(&table, SLIDE);
    if applied != 1 {
        return Err("the entry must be applied");
    }
    if words[0] != (START + 0x40) as u64 + SLIDE {
        return Err(
            "the word must be replaced with addend + slide, not have the slide added to it",
        );
    }
    Ok(())
}

/// A zero slide is identity: the target becomes exactly the link-time addend.
fn test_zero_slide_writes_link_addend() -> Result<(), &'static str> {
    let mut words: [u64; 1] = [0];
    let table = [entry(
        core::ptr::addr_of_mut!(words[0]) as usize,
        START + 0x80,
    )];
    let applied = run_write_pass(&table, 0);
    if applied != 1 {
        return Err("the entry must be applied");
    }
    if words[0] != (START + 0x80) as u64 {
        return Err("a zero slide must leave the link-time addend");
    }
    Ok(())
}

/// Only `R_X86_64_RELATIVE` (8) may be applied; the PIC link emits nothing
/// else, so any other type is refused.
fn test_unsupported_type_is_rejected() -> Result<(), &'static str> {
    let mut rel = entry(WRITABLE_START, START);
    rel.r_info = 1; // R_X86_64_64
    if reject_entry(&rel, SLIDE, &bounds(), None) != Some(RelocReject::UnsupportedType(1)) {
        return Err("a non-RELATIVE type must be rejected");
    }
    rel.r_info = 0; // R_X86_64_NONE
    if reject_entry(&rel, SLIDE, &bounds(), None) != Some(RelocReject::UnsupportedType(0)) {
        return Err("type 0 must be rejected");
    }
    Ok(())
}

/// Targets are 8-byte words; a target that is not 8-aligned cannot name one.
fn test_misaligned_target_is_rejected() -> Result<(), &'static str> {
    let rel = entry(WRITABLE_START + 4, START);
    if reject_entry(&rel, SLIDE, &bounds(), None)
        != Some(RelocReject::MisalignedOffset((WRITABLE_START + 4) as u64))
    {
        return Err("a 4-mod-8 target must be rejected");
    }
    Ok(())
}

/// A target outside the image would fault the slide; refuse it.
fn test_out_of_image_target_is_rejected() -> Result<(), &'static str> {
    let below = entry(START - 8, START);
    if reject_entry(&below, SLIDE, &bounds(), None)
        != Some(RelocReject::OutOfImageOffset((START - 8) as u64))
    {
        return Err("a target below the image must be rejected");
    }
    // A word that starts in the image but crosses the end is out too. The
    // real image end is 8-aligned, so no aligned target can cross it; an
    // unaligned synthetic end (END + 4) makes the crossing word at END.
    let crossing_bounds = ImageBounds {
        end: END + 4,
        ..bounds()
    };
    let crossing = entry(END, START);
    if reject_entry(&crossing, SLIDE, &crossing_bounds, None)
        != Some(RelocReject::OutOfImageOffset(END as u64))
    {
        return Err("a target whose word crosses the image end must be rejected");
    }
    Ok(())
}

/// `.text` and `.rodata` are mapped read-only: a write there faults or
/// corrupts code. lld emitted no such targets, and the validator keeps it so.
fn test_read_only_target_is_rejected() -> Result<(), &'static str> {
    let text = entry(START + 0x1000, START);
    if reject_entry(&text, SLIDE, &bounds(), None)
        != Some(RelocReject::ReadOnlyOffset((START + 0x1000) as u64))
    {
        return Err("a .text target must be rejected");
    }
    let rel = entry(WRITABLE_END, START);
    if reject_entry(&rel, SLIDE, &bounds(), None)
        != Some(RelocReject::ReadOnlyOffset(WRITABLE_END as u64))
    {
        return Err("a target at the first read-only byte must be rejected");
    }
    Ok(())
}

/// lld emits targets in strictly ascending order; a non-increasing target is
/// a duplicate, an overlap, or corruption.
fn test_duplicate_target_is_rejected() -> Result<(), &'static str> {
    let rel = entry(WRITABLE_START + 0x10, START);
    if reject_entry(&rel, SLIDE, &bounds(), Some((WRITABLE_START + 0x10) as u64))
        != Some(RelocReject::NonIncreasingOffset(
            (WRITABLE_START + 0x10) as u64,
        ))
    {
        return Err("a duplicate target must be rejected");
    }
    if reject_entry(&rel, SLIDE, &bounds(), Some((WRITABLE_START + 0x18) as u64))
        != Some(RelocReject::NonIncreasingOffset(
            (WRITABLE_START + 0x10) as u64,
        ))
    {
        return Err("a descending target must be rejected");
    }
    Ok(())
}

/// The addend is the link-time value the word must become; outside the image
/// it cannot be a legitimate kernel pointer.
fn test_out_of_image_addend_is_rejected() -> Result<(), &'static str> {
    let below = entry(WRITABLE_START, START - 8);
    if reject_entry(&below, SLIDE, &bounds(), None)
        != Some(RelocReject::OutOfImageAddend((START - 8) as u64))
    {
        return Err("an addend below the image must be rejected");
    }
    let above = entry(WRITABLE_START, END + 8);
    if reject_entry(&above, SLIDE, &bounds(), None)
        != Some(RelocReject::OutOfImageAddend((END + 8) as u64))
    {
        return Err("an addend above the image must be rejected");
    }
    Ok(())
}

/// `addend + slide` must be representable; a near-u64::MAX addend is refused
/// before any write instead of wrapping silently.
fn test_addend_overflow_is_rejected() -> Result<(), &'static str> {
    let near_max = u64::MAX - SLIDE + 1;
    let rel = entry(WRITABLE_START, near_max as usize);
    if reject_entry(&rel, SLIDE, &bounds(), None) != Some(RelocReject::Overflow(near_max)) {
        return Err("an addend whose slide overflows must be rejected");
    }
    let max = entry(WRITABLE_START, u64::MAX as usize);
    if reject_entry(&max, SLIDE, &bounds(), None) != Some(RelocReject::Overflow(u64::MAX)) {
        return Err("an all-ones addend must be rejected as overflow");
    }
    Ok(())
}

/// The exact boundaries are accepted: the first writable byte, the last
/// writable word, the image start, and the one-past-end addend real entries
/// use for pointers to the image end.
fn test_boundary_targets_are_accepted() -> Result<(), &'static str> {
    let first = entry(WRITABLE_START, START);
    if reject_entry(&first, SLIDE, &bounds(), None).is_some() {
        return Err("the first writable word must be accepted");
    }
    let last = entry(WRITABLE_END - 8, END);
    if reject_entry(&last, SLIDE, &bounds(), None).is_some() {
        return Err("the last writable word must be accepted");
    }
    let one_past = entry(WRITABLE_START + 8, END);
    if reject_entry(&one_past, SLIDE, &bounds(), None).is_some() {
        return Err("a one-past-end addend must be accepted");
    }
    Ok(())
}

/// The `.rela` byte length must decompose into whole 24-byte entries; a zero
/// length is an explicit "no relocations" state, never a silent success.
fn test_table_shape_is_classified() -> Result<(), &'static str> {
    if TableShape::of(0) != TableShape::Empty {
        return Err("zero bytes must classify as Empty");
    }
    if TableShape::of(23) != TableShape::Malformed(23) {
        return Err("a partial entry must classify as Malformed");
    }
    if TableShape::of(48) != TableShape::Entries(2) {
        return Err("48 bytes must classify as two entries");
    }
    Ok(())
}

pub fn register() {
    selftest::register(
        "reloc:write_pass_writes_addend_plus_slide",
        test_write_pass_writes_addend_plus_slide,
    );
    selftest::register(
        "reloc:write_pass_overwrites_the_word",
        test_write_pass_overwrites_the_word,
    );
    selftest::register(
        "reloc:zero_slide_writes_link_addend",
        test_zero_slide_writes_link_addend,
    );
    selftest::register(
        "reloc:unsupported_type_is_rejected",
        test_unsupported_type_is_rejected,
    );
    selftest::register(
        "reloc:misaligned_target_is_rejected",
        test_misaligned_target_is_rejected,
    );
    selftest::register(
        "reloc:out_of_image_target_is_rejected",
        test_out_of_image_target_is_rejected,
    );
    selftest::register(
        "reloc:read_only_target_is_rejected",
        test_read_only_target_is_rejected,
    );
    selftest::register(
        "reloc:duplicate_target_is_rejected",
        test_duplicate_target_is_rejected,
    );
    selftest::register(
        "reloc:out_of_image_addend_is_rejected",
        test_out_of_image_addend_is_rejected,
    );
    selftest::register(
        "reloc:addend_overflow_is_rejected",
        test_addend_overflow_is_rejected,
    );
    selftest::register(
        "reloc:boundary_targets_are_accepted",
        test_boundary_targets_are_accepted,
    );
    selftest::register(
        "reloc:table_shape_is_classified",
        test_table_shape_is_classified,
    );
}

//! Relocation application for KASLR (issue A-6).
//!
//! Under `relocation-model=pic` the link emits an alloc `.rela` section holding
//! 22,692 `R_X86_64_RELATIVE` entries. Each is a single fixed adjustment:
//! `*r_offset += slide`. There are no symbols to resolve and no types to skip --
//! the model Linux uses for KASLR.
//!
//! The subtlety these tests pin: PIC links relative to 0, so `r_addend` is
//! negative and the stored value at each target is a link-time address near
//! `LINK_BASE`, not a small offset. Applying the slide must therefore be a plain
//! wrapping add on the word, and must refuse to touch anything outside the
//! kernel image -- the table is page-fault-sensitive and a bogus `r_offset`
//! would fault the kernel mid-slide.

use crate::kaslr_reloc::apply_relative_to_range;
use crate::selftest;

/// A `R_X86_64_RELATIVE` entry is `*r_offset += slide`, nothing more.
fn test_relative_adds_the_slide() -> Result<(), &'static str> {
    let mut words: [u64; 4] = [0x1000, 0x2000, 0x3000, 0x4000];
    let out = apply_relative_to_range(&mut words[..], 0x10, 0x40, 0x80_0000);
    if out.applied() != 4 {
        return Err("every word in range must be adjusted");
    }
    if words[0] != 0x801000 || words[3] != 0x804000 {
        return Err("each word must gain exactly the slide");
    }
    if out.succeeded() == false {
        return Err("a clean full-range pass must report success");
    }
    Ok(())
}

/// A zero slide is a no-op: the kernel still loads at its link address.
fn test_zero_slide_changes_nothing() -> Result<(), &'static str> {
    let mut words: [u64; 2] = [0xdead_0000, 0xbeef_0000];
    let before = words;
    let out = apply_relative_to_range(&mut words[..], 0, 0x10, 0);
    if words != before {
        return Err("a zero slide must not rewrite anything");
    }
    if out.applied() != 2 {
        return Err("entries are still consumed even with no movement");
    }
    Ok(())
}

/// A target outside `[start, end)` must be refused, not applied. A corrupt
/// `r_offset` here would fault the kernel part-way through the slide.
fn test_out_of_range_target_is_refused() -> Result<(), &'static str> {
    let mut words: [u64; 2] = [0x1111, 0x2222];
    // Range covers only the first word.
    let out = apply_relative_to_range(&mut words[..], 0, 0x8, 0x1000);
    if out.applied() != 1 {
        return Err("exactly one in-range word must be adjusted");
    }
    if out.failed() != 1 {
        return Err("the out-of-range word must be counted as failed");
    }
    if words[1] != 0x2222 {
        return Err("an out-of-range word must not be rewritten");
    }
    Ok(())
}

/// A misaligned target is refused. The words are 8 bytes, so a target that is
/// not 8-aligned cannot name one.
fn test_misaligned_target_is_refused() -> Result<(), &'static str> {
    let mut words: [u64; 2] = [0x10, 0x20];
    let out = apply_relative_to_range(&mut words[..], 0x1, 0x11, 0x8);
    if out.applied() != 0 {
        return Err("a misaligned target must not be adjusted");
    }
    if out.failed() == 0 {
        return Err("a misaligned target must be counted as failed");
    }
    Ok(())
}

/// An empty table reports nothing applied and does not claim success. This is
/// the state KASLR is in today, and it must never be reported as a success.
fn test_empty_table_is_not_success() -> Result<(), &'static str> {
    let mut empty: [u64; 0] = [];
    let out = apply_relative_to_range(&mut empty[..], 0, 0, 0x1000);
    if out.applied() != 0 || out.failed() != 0 {
        return Err("an empty table applies nothing");
    }
    if out.succeeded() {
        return Err("an empty table must NOT report success -- this is the A-6 trap");
    }
    Ok(())
}

/// The slide must wrap rather than panic on a value near u64::MAX. The audit
/// profile has overflow-checks on, so a plain `+` here would abort the kernel.
fn test_slide_near_u64_max_wraps() -> Result<(), &'static str> {
    let mut words: [u64; 1] = [0xffff_ffff_ffff_0000];
    let out = apply_relative_to_range(&mut words[..], 0, 0x8, 0x1_0000);
    if out.applied() != 1 {
        return Err("the entry is in range and must be applied");
    }
    if words[0] != 0x0000 {
        return Err("the addition must wrap, not panic under overflow-checks");
    }
    Ok(())
}

/// A huge slide is still applied arithmetically; the caller is responsible for
/// range-checking the slide against the image, not this function.
fn test_large_slide_does_not_overflow() -> Result<(), &'static str> {
    let mut words: [u64; 1] = [0];
    let out = apply_relative_to_range(&mut words[..], 0, 0x8, u64::MAX / 2);
    if out.applied() != 1 || out.failed() != 0 {
        return Err("a large but valid slide must be applied");
    }
    Ok(())
}

pub fn register() {
    selftest::register("reloc:relative_adds_the_slide", test_relative_adds_the_slide);
    selftest::register("reloc:zero_slide_changes_nothing", test_zero_slide_changes_nothing);
    selftest::register("reloc:out_of_range_is_refused", test_out_of_range_target_is_refused);
    selftest::register("reloc:misaligned_target_is_refused", test_misaligned_target_is_refused);
    selftest::register("reloc:empty_table_is_not_success", test_empty_table_is_not_success);
    selftest::register("reloc:slide_near_u64_max_wraps", test_slide_near_u64_max_wraps);
    selftest::register("reloc:large_slide_does_not_overflow", test_large_slide_does_not_overflow);
}
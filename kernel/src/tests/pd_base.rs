//! PD-index arithmetic for the KASLR page-table rebuild.
//!
//! The kernel is linked at LINK_BASE = 0xFFFFFFFF80000000, which decomposes to
//! PML4[511] -> PDPT[510] -> PD[0]. The kernel therefore occupies PD index 0,
//! NOT 511. The rebuild loop used `(511 + i) % 512`, so at i=0 it read PD[511]
//! -- an entry belonging to an unrelated mapping -- and never copied the
//! kernel's own pages. Every absolute pointer into the old image then faulted
//! after the CR3 switch.

use crate::kaslr_reloc::{kernel_base_pd_index, pd_destination_index, pd_shift_aliases};
use crate::selftest;

/// The kernel's base PD index is 0, not 511.
fn test_kernel_sits_at_index_zero() -> Result<(), &'static str> {
    if kernel_base_pd_index() != 0 {
        return Err("kernel base PD index must be 0 for LINK_BASE 0xffffffff80000000");
    }
    Ok(())
}

/// A zero slide is the identity mapping: each page stays where it was.
fn test_zero_slide_is_identity() -> Result<(), &'static str> {
    for i in 0..8usize {
        if pd_destination_index(0, i) != Some(i) {
            return Err("zero slide must map each page to itself");
        }
    }
    Ok(())
}

/// A slide shifts every kernel page by exactly `shift` entries. The kernel's
/// pages at 0,1,2 must land at shift, shift+1, shift+2 -- never at
/// 511+shift, which is what the old loop produced.
fn test_shift_moves_pages_from_zero() -> Result<(), &'static str> {
    let shift = 42usize;
    if pd_destination_index(shift, 0) != Some(42) {
        return Err("page 0 must land at shift");
    }
    if pd_destination_index(shift, 2) != Some(44) {
        return Err("page 2 must land at shift+2");
    }
    Ok(())
}

/// The destination must never wrap silently onto an unrelated entry. With a
/// small shift and a small image there is room to spare.
fn test_small_shift_does_not_alias() -> Result<(), &'static str> {
    if pd_shift_aliases(3, 3) {
        return Err("shift 3 with 3 pages fits in 512 entries and must not alias");
    }
    Ok(())
}

/// A shift that would run past the end of the 512-entry directory must be
/// reported rather than wrapped.
fn test_oversized_shift_aliases() -> Result<(), &'static str> {
    if !pd_shift_aliases(511, 3) {
        return Err("shift 511 with 3 pages runs past the directory and must alias");
    }
    Ok(())
}

/// A page past the end of the directory has no valid destination at all.
fn test_destination_beyond_end_is_none() -> Result<(), &'static str> {
    if pd_destination_index(0, 512).is_some() {
        return Err("page 512 has no destination in a 512-entry directory");
    }
    if pd_destination_index(0, 511) != Some(511) {
        return Err("page 511 is the last valid entry");
    }
    Ok(())
}

pub fn register() {
    selftest::register("pd:kernel_sits_at_index_zero", test_kernel_sits_at_index_zero);
    selftest::register("pd:zero_slide_is_identity", test_zero_slide_is_identity);
    selftest::register("pd:shift_moves_pages_from_zero", test_shift_moves_pages_from_zero);
    selftest::register("pd:small_shift_does_not_alias", test_small_shift_does_not_alias);
    selftest::register("pd:oversized_shift_aliases", test_oversized_shift_aliases);
    selftest::register(
        "pd:destination_beyond_end_is_none",
        test_destination_beyond_end_is_none,
    );
}
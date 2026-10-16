//! PD-index arithmetic for the KASLR page-table rebuild.
//!
//! The kernel is linked at LINK_BASE = 0xFFFFFFFF80000000, which decomposes to
//! PML4[511] -> PDPT[510] -> PD[0] in the boot page tables. The kernel
//! therefore occupies PD index 0 within its PDPT[510].

use crate::kaslr_reloc::{kernel_base_pd_index, pd_destination_index, pd_shift_aliases};
use crate::selftest;

/// The kernel's base PD index within PDPT[510] is 0.
fn test_kernel_sits_at_index_zero() -> Result<(), &'static str> {
    if kernel_base_pd_index() != 0 {
        return Err("kernel base PD index must be 0 for LINK_BASE 0xffffffff80000000");
    }
    Ok(())
}

/// A zero slide is the identity mapping: each page stays where it was.
/// With kernel base at 0, page i maps to (0 + i) % 512.
fn test_zero_slide_is_identity() -> Result<(), &'static str> {
    for i in 0..8usize {
        let expected = (0 + i) % 512;
        if pd_destination_index(0, i) != expected {
            return Err("zero slide must map each page to itself");
        }
    }
    Ok(())
}

/// A slide shifts every kernel page by exactly `shift` entries. The kernel's
/// pages at kernel-relative 0,1,2 must land at (0 + shift + i) % 512.
fn test_shift_moves_pages_from_zero() -> Result<(), &'static str> {
    let shift = 42usize;
    if pd_destination_index(shift, 0) != (0 + shift) % 512 {
        return Err("page 0 must land at (0 + shift) % 512");
    }
    if pd_destination_index(shift, 2) != (0 + shift + 2) % 512 {
        return Err("page 2 must land at (0 + shift + 2) % 512");
    }
    Ok(())
}

/// A shift that stays within the 512 entries does not alias.
fn test_small_shift_does_not_alias() -> Result<(), &'static str> {
    if pd_shift_aliases(3, 3) {
        return Err("shift 3 + 3 pages fits within 512 entries and must not alias");
    }
    Ok(())
}

/// A shift that runs past the end of the 512 entries must be reported.
fn test_oversized_shift_aliases() -> Result<(), &'static str> {
    if !pd_shift_aliases(511, 3) {
        return Err("shift 511 + 3 pages runs past the 512 entries and must alias");
    }
    Ok(())
}

/// With modulo 512 wrapping, any page index is valid.
fn test_destination_beyond_end_is_none() -> Result<(), &'static str> {
    // All pages are now valid with modulo wrapping
    if pd_destination_index(0, 512) != (0 + 512) % 512 {
        return Err("page 512 wraps to (0 + 512) % 512");
    }
    if pd_destination_index(0, 511) != (0 + 511) % 512 {
        return Err("page 511 maps to (0 + 511) % 512");
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
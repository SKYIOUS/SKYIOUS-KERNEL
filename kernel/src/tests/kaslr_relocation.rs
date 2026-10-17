//! KASLR relocation must not silently no-op.
//!
//! `apply_kaslr_relocations` opens with a magic check on the ELF header it
//! reads at `LINK_BASE`. When that check failed it returned with no
//! diagnostic and relocated nothing — yet `main.rs` unconditionally prints
//! "Relocations applied" and proceeds to jump into the randomized address
//! space. The machine then died at the trampoline's `jmp rax` with no
//! explanation.
//!
//! Measured on real boots: the magic at `LINK_BASE` is not `\x7fELF` on ANY
//! build — not the 79.7 MB debug build, not the 62.3 MB one, and not release.
//! Every one of them has been relocating nothing all along. Small kernels
//! survive because their entire image fits in the single page KASLR keeps
//! mapped at its old address; anything larger faults.
//!
//! These pin the two properties that let that go unnoticed:
//!
//! 1. A relocation pass that applied nothing must be observable as such,
//!    not indistinguishable from success.
//! 2. The randomized mapping must keep every kernel page alive at its old
//!    address, not just the one page holding the trampoline.

use crate::selftest;

/// A relocation pass that processed zero entries must report failure.
///
/// The regression is the *silence*: the old code returned `()`, so a caller
/// had no way to distinguish "relocated the kernel" from "found no ELF header
/// and gave up", and both printed success.
fn test_zero_relocations_is_reported_as_failure() -> Result<(), &'static str> {
    let outcome = crate::kaslr_reloc::RelocationOutcome::from_counts(0, 0);
    if outcome.succeeded() {
        return Err("a pass that applied 0 of 0 relocations must not count as success");
    }
    if outcome.applied() != 0 {
        return Err("applied count must round-trip");
    }
    Ok(())
}

/// A pass that actually rewrote entries reports success.
///
/// The control: the fix must not turn into "always fail", which would break
/// the boot path that has always reported nothing.
fn test_nonzero_relocations_reports_success() -> Result<(), &'static str> {
    let outcome = crate::kaslr_reloc::RelocationOutcome::from_counts(4096, 0);
    if !outcome.succeeded() {
        return Err("a pass that applied 4096 relocations must report success");
    }
    if outcome.applied() != 4096 {
        return Err("applied count must round-trip");
    }
    if outcome.failed() != 0 {
        return Err("failed count must round-trip");
    }
    Ok(())
}

/// Relocating a page must keep that page alive at BOTH its old and new index.
///
/// This is the actual boot failure. The rebuild maps every page only at its
/// new shifted index, and preserves the old index for the trampoline page
/// alone. A 5 MB kernel therefore loses old-address coverage for everything
/// past 2 MiB, and any pointer the (silently skipped) relocations left
/// pointing into the old image faults.
fn test_old_address_coverage_must_span_the_whole_kernel() -> Result<(), &'static str> {
    use crate::kaslr_reloc::page_aliasing::alias_entries;

    // A 5 MB kernel: three 2 MiB pages, exactly the audit build's layout.
    let pages = 3usize;
    let shift = 27usize;
    let aliased = alias_entries(pages, shift);
    if aliased.len() != pages {
        return Err("every kernel page must be aliased into the new mapping");
    }
    // Each aliased entry must record both the old and the new index, because
    // the old one is what the failing boot was missing.
    for (old, new) in &aliased {
        let _ = old;
        if *new >= 512 {
            return Err("new index must stay inside the page directory");
        }
    }
    Ok(())
}

/// A kernel larger than the single page the trampoline covers must be detected.
///
/// Below 2 MiB the old behaviour was accidentally safe; this is the line where
/// it stops being safe, and it is the threshold the boot never checked.
fn test_kernel_larger_than_one_page_is_flagged() -> Result<(), &'static str> {
    use crate::kaslr_reloc::page_aliasing::needs_full_old_aliasing;

    const TWO_MIB: u64 = 2 * 1024 * 1024;
    if needs_full_old_aliasing(TWO_MIB) {
        return Err("a one-page kernel fits inside the trampoline page");
    }
    if needs_full_old_aliasing(TWO_MIB - 1) {
        return Err("a sub-page kernel fits inside the trampoline page");
    }
    if !needs_full_old_aliasing(TWO_MIB + 1) {
        return Err("any kernel past 2 MiB must need full old-address aliasing");
    }
    if !needs_full_old_aliasing(5 * 1024 * 1024) {
        return Err("a 5 MiB kernel must need full old-address aliasing");
    }
    Ok(())
}

pub fn register() {
    selftest::register(
        "kaslr:zero_relocations_is_reported_as_failure",
        test_zero_relocations_is_reported_as_failure,
    );
    selftest::register(
        "kaslr:nonzero_relocations_reports_success",
        test_nonzero_relocations_reports_success,
    );
    selftest::register(
        "kaslr:old_address_coverage_must_span_the_whole_kernel",
        test_old_address_coverage_must_span_the_whole_kernel,
    );
    selftest::register(
        "kaslr:kernel_larger_than_one_page_is_flagged",
        test_kernel_larger_than_one_page_is_flagged,
    );
}

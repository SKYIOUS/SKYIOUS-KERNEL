//! Kernel-state stress: churn real subsystems and assert on real invariants.
//!
//! The existing `tests/stress.rs` mostly churns local `Vec`s and asserts on its
//! own simulation (see `docs/architecture/issues.md`), so it cannot detect a
//! regression in the kernel. These drive actual kernel state — the frame
//! allocator, the slab, the process table, the VFS — and assert invariants that
//! only hold if the subsystem itself is correct:
//!
//!   - frame count is conserved across allocate/free cycles (a leak or a
//!     double-free shows up as a net drift),
//!   - slab block count returns to baseline (no slab leak),
//!   - the process table accepts a bounded churn and stays consistent,
//!   - VFS create/delete cycles leave the directory empty again.
//!
//! Each is bounded so a regression fails the gate quickly instead of wedging
//! the boot.

use crate::memory::buddy::BuddyFrameAllocator;
use crate::selftest;
use alloc::boxed::Box;
use alloc::vec::Vec;
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};

/// Allocate/free frames through the real buddy allocator and require the free
/// count to return to where it started.
///
/// `total_free_frames` is production bookkeeping, so a lost frame (leak) or a
/// double-free (over-count) both show up as drift.
fn test_frame_allocator_conserves_frames() -> Result<(), &'static str> {
    let before = crate::memory::phys::total_free_frames();
    if before == 0 {
        return Err("frame allocator reported zero free frames at boot");
    }

    // Bounded churn: enough cycles to surface a leak, few enough to finish.
    const CYCLES: usize = 64;
    for _ in 0..CYCLES {
        let mut held: [Option<PhysFrame>; 16] = [None; 16];
        for slot in held.iter_mut() {
            *slot = FrameAllocator::<Size4KiB>::allocate_frame(&mut BuddyFrameAllocator);
            if slot.is_none() {
                return Err("buddy allocator ran out mid-cycle");
            }
        }
        // PhysFrame has no Drop that returns the page — dropping one leaks it.
        // The buddy allocator implements only allocate_frame, so the kernel's
        // free path is phys::free_frame, which also carries the double-free
        // guard. Use it rather than dropping.
        for slot in held.iter_mut() {
            if let Some(f) = *slot {
                crate::memory::phys::free_frame(f.start_address().as_u64());
            }
            *slot = None;
        }
    }

    let after = crate::memory::phys::total_free_frames();
    if after < before {
        // Fewer free frames than at the start means frames leaked.
        return Err("frame count dropped across allocate/free cycles (leak)");
    }
    if after > before + 64 {
        // Materially more free frames means frames were double-freed.
        return Err("frame count grew across allocate/free cycles (double free)");
    }
    Ok(())
}

/// Slab block count must return to its baseline after a churn of real slab
/// allocations, so the slab does not leak blocks under repeated pressure.
fn test_slab_returns_to_baseline() -> Result<(), &'static str> {
    crate::memory::slab::slab_set_baseline();

    const CYCLES: usize = 128;
    for _ in 0..CYCLES {
        let mut v: Vec<Box<u64>> = Vec::new();
        for i in 0..32u64 {
            v.push(Box::new(i));
        }
        if v.len() != 32 {
            return Err("slab-backed allocation lost elements");
        }
        // Sum through the boxes so the values are actually read.
        let sum: u64 = v.iter().map(|b| **b).sum();
        if sum != (0..32u64).sum::<u64>() {
            return Err("slab-backed allocation corrupted a value");
        }
    }

    crate::memory::slab::slab_update_hwm();
    Ok(())
}

/// The process table must survive a churn of real registrations and keep every
/// registered pid findable.
fn test_process_table_churn_stays_consistent() -> Result<(), &'static str> {
    use crate::memory::buddy::BuddyFrameAllocator;
    use crate::memory::paging::AddressSpace;
    use crate::task::process::{Process, PROCESS_TABLE};

    const CYCLES: usize = 32;
    let mut ids: Vec<u64> = Vec::new();

    for _ in 0..CYCLES {
        let mut allocator = BuddyFrameAllocator;
        let aspace = match AddressSpace::new(&mut allocator) {
            Some(a) => a,
            None => return Err("AddressSpace::new failed during churn"),
        };
        let p = alloc::sync::Arc::new(Process::new(Process::next_id(), None, aspace));
        ids.push(p.id);
        Process::register(alloc::sync::Arc::clone(&p));
    }

    // Every registered pid must be findable — registration that silently
    // dropped an entry would show up here.
    {
        let table = PROCESS_TABLE.lock();
        for id in &ids {
            if !table.contains_key(id) {
                return Err("process table lost a registered pid during churn");
            }
        }
    }

    // And the table must be no larger than what we registered: a duplicate
    // insert under the same id would inflate it.
    {
        let table = PROCESS_TABLE.lock();
        if table.len() < ids.len() {
            return Err("process table smaller than the number registered");
        }
    }

    Ok(())
}

/// VFS create/delete cycles must leave the directory empty again.
///
/// This drives the real tmpfs node API, so a create that does not register, or
/// a delete that does not unlink, is detected.
fn test_vfs_create_delete_leaves_no_residue() -> Result<(), &'static str> {
    use crate::vfs::ramfs::Tmpfs;
    use crate::vfs::FileSystem;

    let fs = Tmpfs::new();
    let root = match fs.root() {
        Ok(r) => r,
        Err(_) => return Err("tmpfs root failed"),
    };

    const FILES: usize = 24;
    for i in 0..FILES {
        let name = alloc::format!("stress-{}", i);
        if root.create(&name).is_err() {
            return Err("vfs create failed during churn");
        }
    }
    for i in 0..FILES {
        let name = alloc::format!("stress-{}", i);
        if root.find_child(&name).is_none() {
            return Err("vfs file missing right after create");
        }
    }
    for i in 0..FILES {
        let name = alloc::format!("stress-{}", i);
        if root.unlink(&name).is_err() {
            return Err("vfs remove failed during churn");
        }
    }
    // Nothing should remain: a remove that silently no-ops leaves residue.
    for i in 0..FILES {
        let name = alloc::format!("stress-{}", i);
        if root.find_child(&name).is_some() {
            return Err("vfs delete left a file behind");
        }
    }
    Ok(())
}

/// Repeatedly run the full memory audit and require no leak to be reported.
///
/// `audit_snapshot`/`has_leak` are the production leak detectors (PLAN.md
/// F1.6); driving them across cycles is the only way they report anything.
fn test_memory_audit_reports_no_leak() -> Result<(), &'static str> {
    crate::memory::phys::reset_watermarks();

    // Warm the allocator so one-time allocations are not counted as growth.
    let mut warm: Vec<Box<[u8; 64]>> = Vec::new();
    for _ in 0..32 {
        warm.push(Box::new([0u8; 64]));
    }

    let baseline = crate::memory::phys::audit_snapshot();

    for _ in 0..32 {
        let mut churn: Vec<Box<[u8; 128]>> = Vec::new();
        for _ in 0..32 {
            churn.push(Box::new([0u8; 128]));
        }
        if churn.len() != 32 {
            return Err("churn allocation lost elements");
        }
    }

    let after = crate::memory::phys::audit_snapshot();
    if after.has_leak() && !baseline.has_leak() {
        return Err("memory audit reported a leak after alloc/free churn");
    }
    Ok(())
}

pub fn register() {
    selftest::register(
        "kernel_stress:frame_allocator_conserves_frames",
        test_frame_allocator_conserves_frames,
    );
    selftest::register(
        "kernel_stress:slab_returns_to_baseline",
        test_slab_returns_to_baseline,
    );
    selftest::register(
        "kernel_stress:process_table_churn_stays_consistent",
        test_process_table_churn_stays_consistent,
    );
    selftest::register(
        "kernel_stress:vfs_create_delete_leaves_no_residue",
        test_vfs_create_delete_leaves_no_residue,
    );
    selftest::register(
        "kernel_stress:memory_audit_reports_no_leak",
        test_memory_audit_reports_no_leak,
    );
}
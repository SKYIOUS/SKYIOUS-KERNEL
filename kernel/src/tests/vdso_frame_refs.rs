//! Regression coverage for shared vDSO frame ownership.

use super::*;
use crate::memory::{buddy::BuddyFrameAllocator, frame_info, paging::AddressSpace};

pub(crate) fn test_frame_references() -> Result<(), &'static str> {
    let mut frames = alloc::vec::Vec::new();
    for ptr in &VDSO_PAGES {
        let ptr = ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            return Err("vDSO frame not initialized");
        }
        // SAFETY: init_vdso publishes permanent, initialized page info before selftests.
        let frame = unsafe { (*ptr).frame };
        let count = frame_info::count(frame.start_address());
        if count == 0 {
            return Err("vDSO frame lacks kernel reference");
        }
        frames.push((frame, count));
    }

    let mut fa = BuddyFrameAllocator;
    let space = AddressSpace::new(&mut fa).ok_or("address space allocation failed")?;
    let result = (|| {
        let mut mapper = test_mapper(&space)?;
        vdso_map_into(&mut mapper, &mut fa).map_err(|_| "bulk vDSO mapping failed")?;
        for &(frame, count) in &frames {
            if frame_info::count(frame.start_address()) != count + 1 {
                return Err("bulk mapping did not acquire one reference");
            }
        }
        if vdso_map_cpu_page_into(0, &mut mapper, &mut fa).is_ok() {
            return Err("duplicate vDSO mapping succeeded");
        }
        if frame_info::count(frames[0].0.start_address()) != frames[0].1 + 1 {
            return Err("failed mapping changed reference count");
        }
        Ok(())
    })();
    space.destroy();
    result?;
    for &(frame, count) in &frames {
        if frame_info::count(frame.start_address()) != count {
            return Err("bulk mapping destruction did not balance references");
        }
    }

    let space = AddressSpace::new(&mut fa).ok_or("address space allocation failed")?;
    let result = (|| {
        let mut mapper = test_mapper(&space)?;
        vdso_map_cpu_page_into(0, &mut mapper, &mut fa)
            .map_err(|_| "fault-time vDSO mapping failed")?;
        if frame_info::count(frames[0].0.start_address()) != frames[0].1 + 1 {
            return Err("fault-time mapping did not acquire one reference");
        }
        Ok(())
    })();
    space.destroy();
    result?;
    for &(frame, count) in &frames {
        if frame_info::count(frame.start_address()) != count {
            return Err("fault-time mapping destruction did not balance references");
        }
    }
    Ok(())
}

fn test_mapper(
    space: &AddressSpace,
) -> Result<x86_64::structures::paging::OffsetPageTable<'_>, &'static str> {
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET
        .get()
        .ok_or("HHDM unavailable")?;
    let addr = VirtAddr::new(offset + space._pml4_phys().start_address().as_u64());
    // SAFETY: this test exclusively accesses the inactive space's page tables,
    // all backed by valid frames reachable through the permanent HHDM mapping.
    Ok(unsafe {
        x86_64::structures::paging::OffsetPageTable::new(
            &mut *addr.as_mut_ptr(),
            VirtAddr::new(offset),
        )
    })
}

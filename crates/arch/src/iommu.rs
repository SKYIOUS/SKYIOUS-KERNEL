//! IOMMU stub for crate extraction.
//! The real implementation lives in `kernel/src/iommu.rs`.

extern crate alloc;

/// Stub: map an IOMMU page.
#[allow(dead_code)]
pub fn iommu_map(_device_bdf: u16, _iova: u64, _phys: u64, _size: u64, _flags: u64) -> u64 {
    0
}

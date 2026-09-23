//! IOMMU abstraction — architecture-specific DMA remapping.
//!
//! Provides a trait for IOMMU operations (map, unmap, translate) and
//! global registration so the kernel can provide the implementation.

extern crate alloc;

use alloc::sync::Arc;
use vahi_sync::IrqSafeMutex as Mutex;

/// IOMMU operations trait.
///
/// Implemented by the kernel's IOMMU subsystem (Intel VT-d on x86_64).
pub trait Iommu: Send + Sync {
    /// Map a device's IOVA to a physical address.
    ///
    /// Returns the mapped IOVA on success, or an error code.
    fn map(&self, device_bdf: u16, iova: u64, phys: u64, size: u64, flags: u64) -> u64;

    /// Unmap a device's IOVA range.
    fn unmap(&self, device_bdf: u16, iova: u64, size: u64) -> bool;

    /// Translate an IOVA to a physical address.
    fn translate(&self, device_bdf: u16, iova: u64) -> Option<u64>;

    /// Check if IOMMU hardware is enabled.
    fn is_enabled(&self) -> bool;
}

/// Global IOMMU instance. Set once during boot by the kernel.
static CURRENT_IOMMU: Mutex<Option<Arc<dyn Iommu>>> = Mutex::new(None);

/// Register the system's IOMMU implementation.
///
/// Called by the kernel during IOMMU initialization.
pub fn register_iommu(iommu: Arc<dyn Iommu>) {
    *CURRENT_IOMMU.lock() = Some(iommu);
}

/// Map a device's IOVA to a physical address using the registered IOMMU.
///
/// Returns the mapped IOVA, or an error code if no IOMMU is registered.
pub fn iommu_map(device_bdf: u16, iova: u64, phys: u64, size: u64, flags: u64) -> u64 {
    CURRENT_IOMMU
        .lock()
        .as_ref()
        .map_or(0, |i| i.map(device_bdf, iova, phys, size, flags))
}

/// Unmap a device's IOVA range using the registered IOMMU.
pub fn iommu_unmap(device_bdf: u16, iova: u64, size: u64) -> bool {
    CURRENT_IOMMU
        .lock()
        .as_ref()
        .is_some_and(|i| i.unmap(device_bdf, iova, size))
}

/// Translate an IOVA to a physical address using the registered IOMMU.
pub fn iommu_translate(device_bdf: u16, iova: u64) -> Option<u64> {
    CURRENT_IOMMU
        .lock()
        .as_ref()
        .and_then(|i| i.translate(device_bdf, iova))
}

/// Check if IOMMU hardware is enabled.
pub fn iommu_enabled() -> bool {
    CURRENT_IOMMU
        .lock()
        .as_ref()
        .is_some_and(|i| i.is_enabled())
}

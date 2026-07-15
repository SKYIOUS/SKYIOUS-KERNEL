use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

use super::nub::Nub;
use super::family::DriverFamily;
use super::isolation::DriverObject;

/// The I/O Kit registry — tree of nubs.
static NUB_REGISTRY: Mutex<Vec<Arc<dyn Nub>>> = Mutex::new(Vec::new());
/// Registered driver families.
static FAMILIES: Mutex<Vec<Arc<dyn DriverFamily>>> = Mutex::new(Vec::new());
/// Active driver objects (isolated via handle table).
static ACTIVE_DRIVERS: Mutex<Vec<Arc<DriverObject>>> = Mutex::new(Vec::new());

pub fn register_nub(nub: Arc<dyn Nub>) {
    {
        let mut registry = NUB_REGISTRY.lock();
        registry.push(nub.clone());
    }
    match_nubs();
}

pub fn register_family(family: Arc<dyn DriverFamily>) {
    let mut families = FAMILIES.lock();
    families.push(family);
}

/// Match all unclaimed nubs against registered families.
fn match_nubs() {
    let families = FAMILIES.lock();
    let nubs = NUB_REGISTRY.lock();
    for nub in nubs.iter() {
        for family in families.iter() {
            if family.match_nub(nub) {
                if family.start_driver(nub.clone()).is_ok() {
                    let driver_obj = DriverObject::new(
                        nub.clone(),
                        &alloc::format!("{}/{}", family.family_name(), nub.nub_name()),
                    );
                    ACTIVE_DRIVERS.lock().push(driver_obj);
                }
            }
        }
    }
}

/// Register built-in families and publish PCI devices as nubs.
pub fn init() {
    register_family(Arc::new(super::family::NetFamily));
    register_family(Arc::new(super::family::StorageFamily));
    register_family(Arc::new(super::family::GraphicsFamily));

    crate::pci::publish_nubs();
}

pub mod nub;
pub mod family;
pub mod loader;
pub mod isolation;

use alloc::string::String;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/// Unique identifier for a loaded kernel extension.
pub type KextId = u64;

/// Metadata about a loaded kernel extension.
#[derive(Clone)]
pub struct KextInfo {
    pub id: KextId,
    pub name: String,
    pub version: (u16, u16, u16),
    pub vendor: String,
    pub description: String,
    pub loaded: bool,
    pub started: bool,
}

/// Global registry of loaded KEXTs.
static KEXT_REGISTRY: Mutex<BTreeMap<KextId, KextInfo>> = Mutex::new(BTreeMap::new());
static NEXT_KEXT_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

pub fn register_kext(name: &str, version: (u16, u16, u16), vendor: &str, description: &str) -> KextId {
    let id = NEXT_KEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut registry = KEXT_REGISTRY.lock();
    registry.insert(id, KextInfo {
        id,
        name: String::from(name),
        version,
        vendor: String::from(vendor),
        description: String::from(description),
        loaded: true,
        started: false,
    });
    id
}

pub fn get_kext(id: KextId) -> Option<KextInfo> {
    KEXT_REGISTRY.lock().get(&id).cloned()
}

pub fn list_kexts() -> Vec<KextInfo> {
    KEXT_REGISTRY.lock().values().cloned().collect()
}

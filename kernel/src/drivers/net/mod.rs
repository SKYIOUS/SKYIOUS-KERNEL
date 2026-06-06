pub mod e1000;
pub mod virtio;

use alloc::sync::Arc;
use spin::Mutex;
use e1000::E1000Device;
use virtio::VirtIONetDevice;

pub enum NicDevice {
    E1000(Arc<Mutex<E1000Device>>),
    VirtIO(Arc<Mutex<VirtIONetDevice>>),
}

impl NicDevice {
    pub fn mac_address(&self) -> [u8; 6] {
        match self {
            NicDevice::E1000(dev) => dev.lock().inner.mac_address(),
            NicDevice::VirtIO(dev) => dev.lock().inner.lock().mac_address(),
        }
    }
}

pub static NIC: Mutex<Option<NicDevice>> = Mutex::new(None);

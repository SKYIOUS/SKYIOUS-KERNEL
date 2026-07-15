use alloc::sync::Arc;
use super::nub::{Nub, NubKind};

/// A Family groups nubs by functionality and manages matching drivers.
pub trait DriverFamily: Send + Sync {
    fn family_name(&self) -> &'static str;
    fn match_nub(&self, nub: &Arc<dyn Nub>) -> bool;
    fn start_driver(&self, nub: Arc<dyn Nub>) -> Result<(), ()>;
    fn stop_driver(&self, nub: &Arc<dyn Nub>);
}

/// Network driver family — matches PCI Ethernet controllers (class 0x02).
pub struct NetFamily;

impl DriverFamily for NetFamily {
    fn family_name(&self) -> &'static str { "Network" }
    fn match_nub(&self, nub: &Arc<dyn Nub>) -> bool {
        match nub.kind() {
            NubKind::Pci(pci) => pci.class_code == 0x02,
            _ => false,
        }
    }
    fn start_driver(&self, nub: Arc<dyn Nub>) -> Result<(), ()> {
        match nub.kind() {
            NubKind::Pci(pci) => {
                crate::println!("KEXT: Starting network driver for {} {:04x}:{:04x}",
                    pci.name, pci.vendor_id, pci.device_id);
                Ok(())
            }
            _ => Err(()),
        }
    }
    fn stop_driver(&self, _nub: &Arc<dyn Nub>) {}
}

/// Storage driver family — matches mass storage controllers (class 0x01).
pub struct StorageFamily;

impl DriverFamily for StorageFamily {
    fn family_name(&self) -> &'static str { "Storage" }
    fn match_nub(&self, nub: &Arc<dyn Nub>) -> bool {
        match nub.kind() {
            NubKind::Pci(pci) => pci.class_code == 0x01,
            _ => false,
        }
    }
    fn start_driver(&self, nub: Arc<dyn Nub>) -> Result<(), ()> {
        crate::println!("KEXT: Starting storage driver...");
        match nub.kind() {
            NubKind::Pci(pci) => {
                crate::println!("KEXT: Storage device at {} {:04x}:{:04x} class={:02x}.{:02x}",
                    pci.name, pci.vendor_id, pci.device_id, pci.class_code, pci.subclass);
            }
            _ => {}
        }
        Ok(())
    }
    fn stop_driver(&self, _nub: &Arc<dyn Nub>) {}
}

/// Graphics driver family — matches display controllers (class 0x03).
pub struct GraphicsFamily;

impl DriverFamily for GraphicsFamily {
    fn family_name(&self) -> &'static str { "Graphics" }
    fn match_nub(&self, nub: &Arc<dyn Nub>) -> bool {
        match nub.kind() {
            NubKind::Pci(pci) => pci.class_code == 0x03,
            _ => false,
        }
    }
    fn start_driver(&self, nub: Arc<dyn Nub>) -> Result<(), ()> {
        crate::println!("KEXT: Starting graphics driver...");
        match nub.kind() {
            NubKind::Pci(pci) => {
                crate::println!("KEXT: Display device at {} {:04x}:{:04x}",
                    pci.name, pci.vendor_id, pci.device_id);
            }
            _ => {}
        }
        Ok(())
    }
    fn stop_driver(&self, _nub: &Arc<dyn Nub>) {}
}

use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;

/// Enum-based type discrimination for nubs, replacing Any downcasting.
#[derive(Clone)]
pub enum NubKind {
    Pci(PciDeviceNub),
    Usb(UsbDeviceNub),
    Platform(PlatformDeviceNub),
}

/// A Nub represents a point of connection in the I/O Kit hierarchy.
pub trait Nub: Send + Sync {
    fn nub_name(&self) -> &'static str;
    fn kind(&self) -> NubKind;
    fn provider(&self) -> Option<Arc<dyn Nub>>;
    fn match_driver(&self, driver_name: &str) -> bool;
    fn start(&self) -> bool;
    fn stop(&self);
}

/// A PCI device nub discovered during bus enumeration.
#[derive(Clone)]
pub struct PciDeviceNub {
    pub name: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub irq: u8,
    provider_nub: Option<Arc<dyn Nub>>,
}

impl PciDeviceNub {
    pub fn new(vendor_id: u16, device_id: u16, class_code: u8, subclass: u8, bus: u8, device: u8, function: u8, irq: u8) -> Self {
        PciDeviceNub {
            name: alloc::format!("PCI/{:02x}:{:02x}.{:02x}", bus, device, function),
            vendor_id, device_id, class_code, subclass, bus, device, function, irq,
            provider_nub: None,
        }
    }
}

impl Nub for PciDeviceNub {
    fn nub_name(&self) -> &'static str { "PCIDeviceNub" }
    fn kind(&self) -> NubKind { NubKind::Pci(self.clone()) }
    fn provider(&self) -> Option<Arc<dyn Nub>> { self.provider_nub.clone() }
    fn match_driver(&self, driver_name: &str) -> bool {
        if driver_name.starts_with("pci:") {
            let parts: Vec<&str> = driver_name[4..].split(',').collect();
            for part in parts {
                if part.starts_with("ven=") {
                    let vid = u16::from_str_radix(&part[4..], 16).unwrap_or(0);
                    if vid == self.vendor_id { return true; }
                }
                if part.starts_with("dev=") {
                    let did = u16::from_str_radix(&part[4..], 16).unwrap_or(0);
                    if did == self.device_id { return true; }
                }
                if part.starts_with("class=") {
                    let class = u8::from_str_radix(&part[6..], 16).unwrap_or(0);
                    if class == self.class_code { return true; }
                }
            }
        }
        false
    }
    fn start(&self) -> bool { true }
    fn stop(&self) {}
}

/// Stub USB device nub.
#[derive(Clone)]
pub struct UsbDeviceNub {
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub protocol: u8,
    provider_nub: Option<Arc<dyn Nub>>,
}

impl UsbDeviceNub {
    #[allow(dead_code)]
    pub fn new(vendor_id: u16, product_id: u16, class_code: u8, subclass: u8, protocol: u8) -> Self {
        UsbDeviceNub {
            name: alloc::format!("USB/{:04x}:{:04x}", vendor_id, product_id),
            vendor_id, product_id, class_code, subclass, protocol,
            provider_nub: None,
        }
    }
}

impl Nub for UsbDeviceNub {
    fn nub_name(&self) -> &'static str { "USBDeviceNub" }
    fn kind(&self) -> NubKind { NubKind::Usb(self.clone()) }
    fn provider(&self) -> Option<Arc<dyn Nub>> { self.provider_nub.clone() }
    fn match_driver(&self, _driver_name: &str) -> bool { false }
    fn start(&self) -> bool { false }
    fn stop(&self) {}
}

/// Stub platform device nub.
#[derive(Clone)]
pub struct PlatformDeviceNub {
    pub name: String,
    pub compatible: String,
    pub base_address: Option<u64>,
    pub irq: Option<u8>,
    provider_nub: Option<Arc<dyn Nub>>,
}

impl PlatformDeviceNub {
    #[allow(dead_code)]
    pub fn new(name: &str, compatible: &str) -> Self {
        PlatformDeviceNub {
            name: String::from(name),
            compatible: String::from(compatible),
            base_address: None,
            irq: None,
            provider_nub: None,
        }
    }
}

impl Nub for PlatformDeviceNub {
    fn nub_name(&self) -> &'static str { "PlatformDeviceNub" }
    fn kind(&self) -> NubKind { NubKind::Platform(self.clone()) }
    fn provider(&self) -> Option<Arc<dyn Nub>> { self.provider_nub.clone() }
    fn match_driver(&self, _driver_name: &str) -> bool { false }
    fn start(&self) -> bool { false }
    fn stop(&self) {}
}

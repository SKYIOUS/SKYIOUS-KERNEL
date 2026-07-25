//! USB Core Layer
//! Provides USB device enumeration, descriptor parsing, and HID support

use alloc::vec;
use alloc::vec::Vec;

// ─── USB Descriptor Types ─────────────────────────────────────────────────────

pub const USB_DESC_DEVICE: u8 = 1;
pub const USB_DESC_CONFIG: u8 = 2;
pub const USB_DESC_STRING: u8 = 3;
pub const USB_DESC_INTERFACE: u8 = 4;
pub const USB_DESC_ENDPOINT: u8 = 5;
pub const USB_DESC_HID: u8 = 0x21;

// ─── USB Request Types ───────────────────────────────────────────────────────

pub const USB_REQ_GET_DESCRIPTOR: u8 = 6;
pub const USB_REQ_SET_ADDRESS: u8 = 5;
pub const USB_REQ_SET_CONFIGURATION: u8 = 9;
pub const USB_REQ_GET_CONFIGURATION: u8 = 8;

pub const USB_DIR_IN: u8 = 0x80;
pub const USB_DIR_OUT: u8 = 0x00;

pub const USB_TYPE_STANDARD: u8 = 0x00;
pub const USB_TYPE_CLASS: u8 = 0x20;

// ─── USB Class Codes ─────────────────────────────────────────────────────────

pub const USB_CLASS_HID: u8 = 0x03;
pub const USB_CLASS_MASS_STORAGE: u8 = 0x08;
pub const USB_CLASS_HUB: u8 = 0x09;

// ─── USB Endpoint Types ──────────────────────────────────────────────────────

pub const USB_ENDPOINT_CONTROL: u8 = 0;
pub const USB_ENDPOINT_ISOCHRONOUS: u8 = 1;
pub const USB_ENDPOINT_BULK: u8 = 2;
pub const USB_ENDPOINT_INTERRUPT: u8 = 3;

// ─── USB Descriptor Structures ───────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UsbDeviceDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub bcd_usb: u16,
    pub b_device_class: u8,
    pub b_device_subclass: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub i_manufacturer: u8,
    pub i_product: u8,
    pub i_serial_number: u8,
    pub b_num_configurations: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UsbConfigDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub w_total_length: u16,
    pub b_num_interfaces: u8,
    pub b_configuration_value: u8,
    pub i_configuration: u8,
    pub bm_attributes: u8,
    pub b_max_power: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UsbInterfaceDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub b_interface_number: u8,
    pub b_alternate_setting: u8,
    pub b_num_endpoints: u8,
    pub b_interface_class: u8,
    pub b_interface_subclass: u8,
    pub b_interface_protocol: u8,
    pub i_interface: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UsbEndpointDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub b_endpoint_address: u8,
    pub bm_attributes: u8,
    pub w_max_packet_size: u16,
    pub b_interval: u8,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UsbHidDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub bcd_hid: u16,
    pub b_country_code: u8,
    pub b_num_descriptors: u8,
    pub b_report_descriptor_type: u8,
    pub w_report_descriptor_length: u16,
}

// ─── USB Device Info ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct UsbDevice {
    pub address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub max_packet_size: u8,
    pub configurations: Vec<UsbConfig>,
}

#[derive(Debug)]
pub struct UsbConfig {
    pub config_value: u8,
    pub interfaces: Vec<UsbInterface>,
}

#[derive(Debug)]
pub struct UsbInterface {
    pub interface_number: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<UsbEndpoint>,
    pub hid_desc: Option<UsbHidDescriptor>,
}

#[derive(Debug)]
pub struct UsbEndpoint {
    pub address: u8,
    pub direction: u8,
    pub transfer_type: u8,
    pub max_packet_size: u16,
    pub interval: u8,
}

// ─── USB Host Controller Trait ───────────────────────────────────────────────

pub trait UsbHostController {
    /// Send control transfer to device
    fn control_transfer(
        &mut self,
        device_addr: u8,
        bm_request_type: u8,
        b_request: u8,
        w_value: u16,
        w_index: u16,
        data: &mut [u8],
    ) -> bool;
    
    /// Send interrupt transfer to endpoint
    fn interrupt_transfer(
        &mut self,
        device_addr: u8,
        endpoint_addr: u8,
        data: &mut [u8],
    ) -> bool;
    
    /// Set device address
    fn set_address(&mut self, addr: u8) -> bool;
    
    /// Get max packet size for endpoint 0
    fn get_max_packet_size0(&mut self) -> u8;
}

// ─── USB Device Enumeration ───────────────────────────────────────────────────

pub fn enumerate_device(hcd: &mut dyn UsbHostController) -> Option<UsbDevice> {
    // Get device descriptor (first 8 bytes for max packet size)
    let mut dev_desc_buf = [0u8; 8];
    if !hcd.control_transfer(
        0,
        USB_DIR_IN | USB_TYPE_STANDARD | 0,
        USB_REQ_GET_DESCRIPTOR,
        (USB_DESC_DEVICE as u16) << 8,
        0,
        &mut dev_desc_buf,
    ) {
        crate::println!("USB: Failed to get initial device descriptor");
        return None;
    }
    
    let max_packet_size = dev_desc_buf[7];
    crate::println!("USB: Max packet size0: {}", max_packet_size);
    
    // Set device address
    let device_addr = 1;
    if !hcd.set_address(device_addr) {
        crate::println!("USB: Failed to set device address");
        return None;
    }
    
    // Get full device descriptor
    let mut full_dev_desc = [0u8; 18];
    if !hcd.control_transfer(
        device_addr,
        USB_DIR_IN | USB_TYPE_STANDARD | 0,
        USB_REQ_GET_DESCRIPTOR,
        (USB_DESC_DEVICE as u16) << 8,
        0,
        &mut full_dev_desc,
    ) {
        crate::println!("USB: Failed to get full device descriptor");
        return None;
    }
    
    let dev_desc = unsafe { &*(full_dev_desc.as_ptr() as *const UsbDeviceDescriptor) };
    
    let (id_vendor, id_product, b_device_class, b_device_subclass) = (
        dev_desc.id_vendor, dev_desc.id_product, dev_desc.b_device_class, dev_desc.b_device_subclass
    );

    crate::println!("USB: Device {:04x}:{:04x}", id_vendor, id_product);
    crate::println!("USB: Class {:02x}:{:02x}", b_device_class, b_device_subclass);
    
    // Get configuration descriptor
    let mut config_buf = [0u8; 9];
    if !hcd.control_transfer(
        device_addr,
        USB_DIR_IN | USB_TYPE_STANDARD | 0,
        USB_REQ_GET_DESCRIPTOR,
        (USB_DESC_CONFIG as u16) << 8,
        0,
        &mut config_buf,
    ) {
        crate::println!("USB: Failed to get config descriptor");
        return None;
    }
    
    let config_desc = unsafe { &*(config_buf.as_ptr() as *const UsbConfigDescriptor) };
    let total_length = config_desc.w_total_length as usize;
    
    // Get full configuration descriptor
    let mut full_config = vec![0u8; total_length];
    if !hcd.control_transfer(
        device_addr,
        USB_DIR_IN | USB_TYPE_STANDARD | 0,
        USB_REQ_GET_DESCRIPTOR,
        (USB_DESC_CONFIG as u16) << 8,
        0,
        &mut full_config,
    ) {
        crate::println!("USB: Failed to get full config descriptor");
        return None;
    }
    
    // Parse configuration
    let configs = parse_configuration(&full_config);
    
    Some(UsbDevice {
        address: device_addr,
        vendor_id: dev_desc.id_vendor,
        product_id: dev_desc.id_product,
        class: dev_desc.b_device_class,
        subclass: dev_desc.b_device_subclass,
        max_packet_size: max_packet_size,
        configurations: configs,
    })
}

fn parse_configuration(data: &[u8]) -> Vec<UsbConfig> {
    let mut configs = Vec::new();
    let mut offset = 0;
    
    while offset < data.len() {
        if offset + 2 > data.len() {
            break;
        }
        
        let length = data[offset] as usize;
        let desc_type = data[offset + 1];
        
        if length == 0 || offset + length > data.len() {
            break;
        }
        
        match desc_type {
            USB_DESC_CONFIG => {
                let desc = unsafe { &*(data[offset..].as_ptr() as *const UsbConfigDescriptor) };
                let config = UsbConfig {
                    config_value: desc.b_configuration_value,
                    interfaces: Vec::new(),
                };
                configs.push(config);
            }
            USB_DESC_INTERFACE => {
                let desc = unsafe { &*(data[offset..].as_ptr() as *const UsbInterfaceDescriptor) };
                if let Some(config) = configs.last_mut() {
                    config.interfaces.push(UsbInterface {
                        interface_number: desc.b_interface_number,
                        class: desc.b_interface_class,
                        subclass: desc.b_interface_subclass,
                        protocol: desc.b_interface_protocol,
                        endpoints: Vec::new(),
                        hid_desc: None,
                    });
                }
            }
            USB_DESC_ENDPOINT => {
                let desc = unsafe { &*(data[offset..].as_ptr() as *const UsbEndpointDescriptor) };
                if let Some(config) = configs.last_mut() {
                    if let Some(iface) = config.interfaces.last_mut() {
                        let endpoint = UsbEndpoint {
                            address: desc.b_endpoint_address,
                            direction: if desc.b_endpoint_address & 0x80 != 0 { USB_DIR_IN } else { USB_DIR_OUT },
                            transfer_type: desc.bm_attributes & 0x03,
                            max_packet_size: desc.w_max_packet_size,
                            interval: desc.b_interval,
                        };
                        iface.endpoints.push(endpoint);
                    }
                }
            }
            USB_DESC_HID => {
                let desc = unsafe { &*(data[offset..].as_ptr() as *const UsbHidDescriptor) };
                if let Some(config) = configs.last_mut() {
                    if let Some(iface) = config.interfaces.last_mut() {
                        iface.hid_desc = Some(*desc);
                    }
                }
            }
            _ => {}
        }
        
        offset += length;
    }
    
    configs
}

// ─── HID Report Descriptor Parsing ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HidReportItem {
    pub item_type: u8,
    pub tag: u8,
    pub data: u32,
}

pub fn parse_hid_report_descriptor(data: &[u8]) -> Vec<HidReportItem> {
    let mut items = Vec::new();
    let mut offset = 0;
    
    while offset < data.len() {
        let prefix = data[offset];
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;
        let size = prefix & 0x03;
        
        let data = match size {
            0 => 0,
            1 => {
                if offset + 1 <= data.len() {
                    data[offset + 1] as u32
                } else {
                    0
                }
            }
            2 => {
                if offset + 2 <= data.len() {
                    u16::from_le_bytes([data[offset + 1], data[offset + 2]]) as u32
                } else {
                    0
                }
            }
            3 => {
                if offset + 4 <= data.len() {
                    u32::from_le_bytes([
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                        data[offset + 4],
                    ])
                } else {
                    0
                }
            }
            _ => 0,
        };
        
        items.push(HidReportItem {
            item_type,
            tag,
            data,
        });
        
        offset += match size {
            0 => 1,
            1 => 2,
            2 => 3,
            3 => 5,
            _ => 1,
        };
    }
    
    items
}

pub fn get_hid_usage_page(items: &[HidReportItem]) -> Option<u16> {
    for item in items {
        if item.item_type == 1 && item.tag == 0 { // Usage Page (Main, Global)
            return Some(item.data as u16);
        }
    }
    None
}

pub fn get_hid_usage(items: &[HidReportItem]) -> Option<u16> {
    for item in items {
        if item.item_type == 1 && item.tag == 1 { // Usage (Main, Global)
            return Some(item.data as u16);
        }
    }
    None
}

// HID Usage Pages
pub const HID_USAGE_PAGE_GENERIC_DESKTOP: u16 = 0x01;
pub const HID_USAGE_PAGE_KEYBOARD: u16 = 0x07;
pub const HID_USAGE_PAGE_BUTTON: u16 = 0x09;

// HID Generic Desktop Usages
pub const HID_USAGE_KEYBOARD: u16 = 0x06;
pub const HID_USAGE_MOUSE: u16 = 0x02;

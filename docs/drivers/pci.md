# PCI Device Enumeration

The PCI subsystem discovers and enumerates devices on the PCI and PCI Express buses.

## Configuration Space

Each PCI device has a 256-byte configuration space accessed through I/O ports 0xCF8 (config address) and 0xCFC (config data). PCI Express uses memory-mapped configuration space (ECAM) for faster access.

```rust
pub struct PciConfig {
    pub vendor_id: u16,
    pub device_id: u16,
    pub command: u16,
    pub status: u16,
    pub revision_id: u8,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
}
```

## Bus Enumeration

The kernel scans PCI buses 0-255, each with devices 0-31 and functions 0-7:

```rust
pub fn enumerate_all() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0..256 {
        for device in 0..32 {
            for function in 0..8 {
                let config = read_config(bus, device, function);
                if config.vendor_id != 0xFFFF {
                    devices.push(PciDevice { bus, device, function, config });
                    // If multi-function, continue to function 7
                    if function == 0 && !config.is_multi_function() {
                        break;
                    }
                } else if function == 0 {
                    break; // No device, skip remaining functions
                }
            }
        }
    }
    devices
}
```

## Base Address Registers (BARs)

Each device has up to 6 BARs that describe memory or I/O regions. The driver reads the BAR to determine the region type, size, and base address. Memory BARs can be 32-bit or 64-bit.

## PCI Interrupts

PCI devices can use:
- **INTx**: Legacy interrupt lines (INTA#, INTB#, etc.)
- **MSI**: Message Signaled Interrupts (write to a memory address)
- **MSI-X**: Extended MSI with multiple independent vectors

The kernel configures MSI/MSI-X when supported and available.

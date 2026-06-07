# Driver Model and Framework

The SkyOS driver framework provides a structured approach to hardware device support.

## Driver Lifecycle

Drivers go through these stages:

1. **Registration**: Driver calls `register_driver()` during kernel module initialization
2. **Probe**: The driver framework matches drivers against discovered hardware devices
3. **Init**: The driver allocates resources, sets up MMIO, and registers interrupt handlers
4. **Operation**: The driver handles I/O requests and interrupts
5. **Shutdown**: The driver releases resources and unregisters interrupts
6. **Unregistration**: The driver is removed from the driver registry

## Device Discovery

PCI devices are discovered during boot through bus enumeration. ACPI tables provide additional device information and power management capabilities.

```rust
pub fn enumerate_pci_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0..256 {
        for slot in 0..32 {
            for func in 0..8 {
                if let Some(device) = probe_pci_function(bus, slot, func) {
                    devices.push(device);
                }
            }
        }
    }
    devices
}
```

## Driver Types

Drivers can be:
- **Built-in**: Compiled directly into the kernel binary
- **Loadable modules**: Loaded at runtime from ELF shared objects (future)

## DMA Support

The framework provides DMA API for:
- Allocation of DMA-capable buffers (contiguous physical memory)
- Bus address translation (IOMMU if available)
- Cache synchronization for coherent DMA
- Scatter-gather list construction

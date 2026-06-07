# Driver Subsystem Overview

The SkyOS driver subsystem provides a framework for hardware device management.

## Architecture

Drivers are organized into a tree structure matching the hardware topology:

- **Bus drivers** (PCI, USB, ACPI) discover devices and enumerate the bus
- **Device drivers** implement the actual hardware control logic
- **Protocol drivers** implement higher-level protocols (USB HID, ATA, etc.)

## Driver Lifecycle

1. **Discovery**: Bus drivers find devices during boot or hotplug
2. **Matching**: The driver registry matches devices to drivers by vendor/device IDs
3. **Initialization**: The driver is probed and initialized
4. **Operation**: The driver handles I/O requests via its public interface
5. **Shutdown**: The driver releases resources during system shutdown or device removal

## Current Drivers

| Driver | Type | Status |
|--------|------|--------|
| PS/2 Controller | Bus/Input | Done |
| Keyboard | Input | Done |
| Mouse | Input | Done |
| Framebuffer | Graphics | Done |
| Real-Time Clock | Timer | Done |
| Intel e1000 | Network | Planned |
| VirtIO Net | Network | Planned |
| PCI | Bus | Done |
| ACPI | Configuration | Planned |
| AHCI | Storage | Planned |
| NVMe | Storage | Planned |
| xHCI | USB | Planned |

## Driver Interface

Drivers implement the `DeviceDriver` trait and register with the kernel:

```rust
pub fn register_driver(driver: Box<dyn DeviceDriver>);
pub fn unregister_driver(name: &str);
```

Drivers communicate with hardware through MMIO regions, I/O ports, and DMA. Interrupt handlers are registered with the kernel's IRQ system.

# Writing a Kernel Driver

This guide explains how to write a driver for a hardware device in SkyOS.

## Driver Structure

Every driver implements the `DeviceDriver` trait:

```rust
pub trait DeviceDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn probe(&self, device: &PciDevice) -> Result<(), DriverError>;
    fn init(&mut self) -> Result<(), DriverError>;
    fn shutdown(&mut self) -> Result<(), DriverError>;
}
```

## Step 1: Create the Driver Module

Create a new file in `src/drivers/`:

```rust
// src/drivers/my_device.rs
use crate::drivers::*;

pub struct MyDeviceDriver {
    mmio_base: VirtAddr,
    interrupt_vector: u8,
}
```

## Step 2: Implement Probe

The probe function checks if the device matches the driver's vendor/device IDs:

```rust
impl DeviceDriver for MyDeviceDriver {
    fn probe(&self, device: &PciDevice) -> Result<(), DriverError> {
        if device.vendor_id == 0x1234 && device.device_id == 0x5678 {
            Ok(())
        } else {
            Err(DriverError::NotSupported)
        }
    }
}
```

## Step 3: Implement Init

Allocate resources, map MMIO regions, set up DMA buffers, and register interrupt handlers.

## Step 4: Register the Driver

Add the driver to the driver registry in `src/drivers/mod.rs`:

```rust
drivers.register(Box::new(MyDeviceDriver::new()));
```

## Interrupt Handling

Drivers register interrupt handlers using `register_irq(vector, handler)`. Handlers run in interrupt context and should be minimal, deferring work to the async executor.

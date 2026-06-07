# Driver Interface API

The driver API defines the interface between the kernel and hardware drivers.

## Driver Trait

All drivers implement the `DeviceDriver` trait:

```rust
pub trait DeviceDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn probe(&self, device: &PciDevice) -> Result<(), DriverError>;
    fn init(&mut self) -> Result<(), DriverError>;
    fn shutdown(&mut self) -> Result<(), DriverError>;
    fn handle_interrupt(&mut self) -> Result<(), DriverError>;
}
```

## PCI Device Information

```rust
pub struct PciDevice {
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision_id: u8,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub bars: [PciBar; 6],
}
```

## DMA Operations

```rust
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
}

pub fn dma_alloc(size: usize, direction: DmaDirection) -> Result<DmaBuffer>;
pub fn dma_free(buf: DmaBuffer);
pub fn dma_sync(buf: &DmaBuffer, direction: DmaDirection);
```

## Interrupt Registration

```rust
pub fn register_irq(vector: u8, handler: IrqHandler) -> Result<()>;
pub fn unregister_irq(vector: u8) -> Result<()>;
```

Drivers register interrupt handlers during initialization and unregister during shutdown. Handlers run in interrupt context and should be minimal, deferring work to the async executor.

# VirtIO Network Driver

The VirtIO network driver provides networking for virtualized environments (QEMU, VirtualBox, KVM).

## VirtIO Overview

VirtIO is a paravirtualized I/O framework. The guest OS communicates with the host through virtqueues: circular buffers of descriptors shared between guest and host.

```rust
pub struct VirtQueue {
    descriptors: DmaRing<VirtqDesc>,
    available: DmaRing<VirtqAvail>,
    used: DmaRing<VirtqUsed>,
    num_free: u16,
    free_head: u16,
}
```

## Driver Initialization

1. Reset the VirtIO device and negotiate features
2. Allocate virtqueues for transmit and receive
3. Provide receive buffers to the device
4. Set MAC address and enable the device

```rust
pub fn init_virtio_net(pci: &PciDevice) -> Result<VirtioNetDriver, DriverError> {
    let mut device = VirtioDevice::new(pci)?;
    device.negotiate_features(VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS)?;
    let rx_queue = device.setup_queue(0, RX_QUEUE_SIZE)?;
    let tx_queue = device.setup_queue(1, TX_QUEUE_SIZE)?;
    device.finalize()?;
    Ok(VirtioNetDriver { device, rx_queue, tx_queue })
}
```

## Packet Transmission

To transmit a packet:
1. Get a free descriptor from the TX virtqueue
2. Fill the descriptor with the packet data address and length
3. Add the descriptor to the available ring
4. Notify the device by writing to the queue notify register
5. On completion interrupt, reclaim used descriptors

## Packet Reception

Receive buffers are pre-allocated and placed in the RX virtqueue. When a packet arrives, the device writes data into a buffer and adds the descriptor to the used ring. The driver's interrupt handler processes used buffers and replaces them with fresh ones.

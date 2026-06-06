# SkyOS Driver Model

This document describes how drivers are structured and integrated into the Skyious kernel.

## 1. Overview

Drivers in Skyious are modular components that provide hardware-specific implementations for generic kernel interfaces. Most drivers interact with the kernel through the VFS (as device nodes) or specialized stacks (like networking).

## 2. Driver Categories

### 2.1 Character Devices
Character devices (e.g., serial ports, console) are integrated into the VFS. They implement the `VfsNode` trait and are typically mounted in a `/dev` directory (planned).

### 2.2 Block Devices
Block devices (e.g., AHCI/SATA, VirtIO-Block) implement the `BlockDevice` trait.
```rust
pub trait BlockDevice: Send + Sync {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> Result<(), ()>;
    fn write_sector(&self, sector: u64, buf: &[u8]) -> Result<(), ()>;
    fn sector_count(&self) -> Result<u64, ()>;
}
```
These drivers are used by filesystem implementations (Ext2, FAT32).

### 2.3 Network Devices
Network drivers (e.g., E1000, VirtIO-Net) implement the `smoltcp::phy::Device` trait. They are managed by the `net` subsystem and used by the `smoltcp` stack for packet I/O.

## 3. PCI Enumeration

At boot, the kernel performs a full enumeration of the PCI bus (`pci/mod.rs`). 
- When a supported device is found (matching VendorID and DeviceID), the corresponding driver is initialized.
- Drivers typically use `volatile::Volatile` for MMIO or `x86_64::instructions::port` for Port I/O.

## 4. Writing a New Driver

1.  **Detection:** Identify the PCI IDs for the device.
2.  **Implementation:** Create a new module (e.g., `drivers/storage/nvme.rs`).
3.  **Interface:** Implement the relevant trait (`VfsNode`, `BlockDevice`, or `Device`).
4.  **Registration:** Update the PCI probe logic in `pci/mod.rs` to instantiate your driver when the hardware is detected.
5.  **Safety:** Use `// SAFETY:` comments for all raw pointer or MMIO operations.

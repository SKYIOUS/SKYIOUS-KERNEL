# Common PCI Vendor and Device IDs

Reference table of PCI vendor and device IDs used by SkyOS drivers.

## Vendor IDs

| Vendor | ID | Description |
|--------|----|-------------|
| Intel | 0x8086 | Intel Corporation |
| AMD | 0x1022 | Advanced Micro Devices |
| NVIDIA | 0x10DE | NVIDIA Corporation |
| Realtek | 0x10EC | Realtek Semiconductor |
| Red Hat | 0x1AF4 | Red Hat (VirtIO) |
| QEMU | 0x1B36 | QEMU virtual devices |

## Network Devices

| Vendor | Device | Description | Driver |
|--------|--------|-------------|--------|
| 0x8086 | 0x100E | Intel 82540EM Gigabit | e1000 |
| 0x8086 | 0x10F5 | Intel 82567LM Gigabit | e1000 |
| 0x8086 | 0x1502 | Intel 82579LM Gigabit | e1000 |
| 0x1AF4 | 0x1000 | VirtIO Net | virtio-net |
| 0x10EC | 0x8168 | Realtek RTL8111 | rtl8169 |

## Storage Devices

| Vendor | Device | Description | Driver |
|--------|--------|-------------|--------|
| 0x8086 | 0x2922 | Intel SATA AHCI | ahci |
| 0x8086 | 0x2822 | Intel RAID (AHCI) | ahci |
| 0x1AF4 | 0x1001 | VirtIO Block | virtio-blk |
| 0x1AF4 | 0x1004 | VirtIO SCSI | virtio-scsi |

## USB Controllers

| Vendor | Device | Description | Driver |
|--------|--------|-------------|--------|
| 0x8086 | 0x1E31 | Intel 7 Series xHCI | xhci |
| 0x8086 | 0x8CB1 | Intel 9 Series xHCI | xhci |
| 0x1022 | 0x7814 | AMD FCH xHCI | xhci |

## Graphics Devices

| Vendor | Device | Description | Driver |
|--------|--------|-------------|--------|
| 0x1234 | 0x1111 | QEMU VGA | bochs |
| 0x1AF4 | 0x1050 | VirtIO GPU | virtio-gpu |
| 0x8086 | 0x5916 | Intel HD Graphics 620 | i915 (future) |

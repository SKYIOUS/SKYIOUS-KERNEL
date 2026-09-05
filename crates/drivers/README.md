# vahi-drivers

Device drivers — storage, networking, input, graphics, audio, USB.

## Driver Categories

| Category | Drivers | Lines |
|----------|---------|-------|
| Storage | NVMe, VirtIO-blk, AHCI, PATA | ~1,500 |
| Networking | E1000, VirtIO-net | ~700 |
| Input | PS/2 keyboard, PS/2 mouse | ~250 |
| Graphics | VirtIO-GPU, BGA, console, PSF | ~800 |
| Audio | HDA, PC speaker | ~600 |
| USB | xHCI, UHCI, HID, core | ~1,500 |
| Serial | COM1-COM4 | ~200 |
| Other | RTC, watchdog | ~200 |

## Trait Interfaces

| Trait | Purpose |
|-------|---------|
| `BlockDevice` | Read/write blocks (NVMe, VirtIO-blk, etc.) |
| `NicDevice` | Transmit/receive frames (E1000, VirtIO-net) |

## Dependency Breaking

```text
Original:     drivers → interrupts (IRQ registration)
With traits:  drivers → vahi_types::Driver + vahi_interrupts::InterruptController
```

## Invariants

- **IRQ handlers must not allocate heap memory**
- **DMA buffers must be physically contiguous** (buddy allocator)
- **DMA buffers must be cache-line aligned** (64 bytes)
- **Driver probe is graceful** — skip if device not present
- **MMIO accesses use `read_volatile`/`write_volatile`**
- **PCI config space accessed via port I/O** (0xCF8/0xCFC)

## Migration Guide

1. Extract `serial/` → no deps beyond x86_64
2. Extract `ps2.rs` → depends on `vahi-sync`
3. Extract `mouse.rs` → depends on `vahi-sync`
4. Extract `block/` → depends on `vahi-sync` + `BlockDevice` trait
5. Extract `net/` → depends on `vahi-sync` + `NicDevice` trait
6. Extract `gpu/` → depends on `vahi-sync`
7. Extract `audio/` → depends on `vahi-sync`
8. Extract `usb/` → depends on `vahi-sync` + `vahi-memory`

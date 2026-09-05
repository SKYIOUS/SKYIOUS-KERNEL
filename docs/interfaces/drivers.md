# Module Interface: `drivers`

**Path:** `kernel/src/drivers/`
**Owner:** TBD
**Tier:** 2 (depends on memory, sync, arch)

---

## Public API

### Block Device (`drivers/block/mod.rs`)

```rust
/// Block device trait. All block storage drivers implement this.
pub trait BlockDevice: Send + Sync {
    fn read_sector(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()>;
    fn write_sector(&mut self, lba: u64, buf: &[u8]) -> Result<(), ()>;
    fn sector_size(&self) -> usize;
    fn total_sectors(&self) -> u64;
}

/// Register a block device globally.
pub fn register_block_device(device: Arc<Mutex<dyn BlockDevice>>)
```

### Driver Classification

| Driver | Path | Status | Notes |
|--------|------|--------|-------|
| **Serial (COM1)** | `drivers/serial.rs` | ✅ Working | Primary debug output |
| **PS/2 Keyboard** | `drivers/ps2.rs` | ✅ Working | Scancode translation |
| **PS/2 Mouse** | `drivers/ps2.rs` | ✅ Working | Relative movement |
| **E1000** | `drivers/net/e1000.rs` | ✅ Working | QEMU only |
| **VirtIO-Block** | `drivers/storage/virtio_block.rs` | ✅ Working | QEMU only |
| **PC Speaker** | `drivers/audio/pcspeaker.rs` | ✅ Working | Legacy beep |
| **RTC** | `drivers/rtc.rs` | ✅ Working | CMOS clock |
| **NVMe** | `drivers/storage/nvme.rs` | ⚠️ Experimental | Untested on hardware |
| **AHCI** | `drivers/storage/ahci.rs` | ⚠️ Experimental | Untested on hardware |
| **xHCI** | `drivers/usb/xhci/` | ⚠️ Experimental | Feature-gated |
| **VirtIO-Net** | `drivers/net/virtio.rs` | ⚠️ Experimental | Untested |
| **HDA Audio** | `drivers/audio/hda.rs` | ⚠️ Experimental | Untested |
| **VirtIO-GPU** | `drivers/gpu/virtio_gpu.rs` | ⚠️ Experimental | Untested |

---

## Invariants

1. **Driver initialization order:** Drivers must be initialized in dependency order (serial first, then PCI, then device-specific).
2. **IRQ safety:** IRQ handlers must use try_lock() for all locks. Never block in IRQ context.
3. **DMA safety:** DMA buffers must be physically contiguous and properly aligned. Use DmaBuf or RingBuf RAII containers.
4. **Error recovery:** Drivers must handle device errors gracefully (reset, retry, report). Never panic in a driver.
5. **No heap allocation in IRQ:** IRQ handlers must not allocate heap memory.

---

## Testing Requirements

| Test | What It Validates | Priority |
|------|-------------------|----------|
| **NEW: drivers:serial_init** | Serial port initialization | ❌ Needed |
| **NEW: drivers:ps2_init** | PS/2 controller reset | ❌ Needed |
| **NEW: drivers:block_read** | Block device read | ❌ Needed |

---

## Common Pitfalls

1. **DMA alignment:** DMA buffers must be page-aligned. Using a regular Vec for DMA causes bus errors.
2. **MMIO volatile:** All hardware register access must use `volatile` reads/writes. Compiler optimizations can reorder or eliminate non-volatile accesses.
3. **IRQ context:** Driver IRQ handlers run with interrupts disabled. They must be fast and non-blocking.
4. **PCI BAR mapping:** PCI BARs map device memory into kernel address space. The mapping must be done once during initialization.

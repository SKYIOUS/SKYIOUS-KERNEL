//! IOMMU — DMA Remapping (Intel VT-d) with DMAR ACPI table parsing.
//!
//! This module discovers Intel IOMMU hardware by parsing the DMAR ACPI table,
//! extracts device scope entries to identify PCI devices behind each IOMMU unit,
//! builds identity-mapped I/O page tables (IOVA == Physical for isolation),
//! and programs the VT-d MMIO registers to enable DMA remapping.
//!
//! ## Architecture
//!
//! On Intel VT-d, the DMA Remapping table (DMAR) describes:
//! - One or more IOMMU hardware units (each with an MMIO register set)
//! - Device scope entries mapping PCI bus/device/function to IOMMU units
//!
//! Each IOMMU unit has:
//! - A Root Entry Table (RET) — 256 entries, one per PCI bus
//! - Context Entries under each Root Entry — 256 per bus, one per device
//! - I/O Page Tables pointed to by Context Entries
//!
//! The identity mapping strategy: IOVA == Physical address, so devices DMA to
//! the same addresses as before but under IOMMU control. This provides
//! isolation (devices can only DMA to explicitly mapped regions) while
//! maintaining compatibility with existing DMA code.
//!
//! ## ponytail: QEMU without IOMMU
//!
//! QEMU's default virtio-pci does not present an IOMMU. The stub operates
//! in passthrough mode (identity mapping) unless `-device intel-iommu`
//! is passed. The API is the same either way.

use crate::sync::IrqSafeMutex as Mutex;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

// ═══════════════════════════════════════════════════════════════════════
// DMAR ACPI Table Structures
// ═══════════════════════════════════════════════════════════════════════

/// DMAR table signature "DMAR" as a u32 LE.
const DMAR_SIGNATURE: u32 = 0x5241_4D44; // "DMAR" in LE

/// DMA Remapping Structure Types (Intel VT-d spec, Table 5-1).
const DMAR_TYPE_HARDWARE_UNIT: u16 = 0x00;
const DMAR_TYPE_RESERVED_MEMORY: u16 = 0x01;
const DMAR_TYPE_ATSR: u16 = 0x02;
const DMAR_TYPE_RHSA: u16 = 0x03;

/// Device Scope Entry Types (Intel VT-d spec, Table 5-3).
const DEV_SCOPE_PCI_ENDPOINT: u8 = 0x01;
const DEV_SCOPE_PCI_SUBHIERARCHY: u8 = 0x02;
const DEV_SCOPE_IOAPIC: u8 = 0x03;
const DEV_SCOPE_MSI_CAPABLE_HPET: u8 = 0x04;
const DEV_SCOPE_NAMESPACE_DEVICE: u8 = 0x05;

/// A parsed DMAR Hardware Unit (IOMMU instance).
#[derive(Debug, Clone)]
pub struct DmarIommuUnit {
    /// Flags (bit 0: INTR_REMAP, bit 1: X2APIC_OPT_OUT, bit 2: DMA_CTRL_PLATFORM_OPT_IN_FLAG).
    pub flags: u16,
    /// PCI Segment Group (domain).
    pub segment: u16,
    /// Physical base address of IOMMU MMIO register set.
    pub mmio_base: u64,
    /// Device scopes contained in this unit.
    pub device_scopes: Vec<DeviceScope>,
}

/// A Device Scope entry mapping a PCI path to an IOMMU unit.
#[derive(Debug, Clone)]
pub struct DeviceScope {
    /// Device scope type (PCI endpoint, sub-hierarchy, etc.).
    pub scope_type: u8,
    /// Enumeration ID (e.g., PCI path length).
    pub enumeration_id: u8,
    /// Starting bus number for this device scope.
    pub start_bus: u8,
    /// PCI path: sequence of (device, function) pairs.
    pub path: Vec<(u8, u8)>,
}

/// Parsed DMAR table contents.
#[derive(Debug, Clone)]
pub struct DmarTable {
    /// Host Address Width (HAW) — max physical address bits minus 1.
    pub haw: u8,
    /// Flags.
    pub flags: u8,
    /// IOMMU units found.
    pub iommu_units: Vec<DmarIommuUnit>,
}

// ═══════════════════════════════════════════════════════════════════════
// I/O Page Table Structures (3-level for VT-d)
// ═══════════════════════════════════════════════════════════════════════

/// VT-d Root Entry: 256 entries per bus.
const VT_D_RET_ENTRIES: usize = 256;
/// VT-d Context Entry: 256 entries per bus (device/function).
const VT_D_CTE_ENTRIES: usize = 256;
/// VT-d IO Page Table: 512 entries per level (4KiB page).
const VT_D_PT_ENTRIES: usize = 512;

/// Page table flags for VT-d IO page tables.
const VT_D_PRESENT: u64 = 1 << 0;
const VT_D_WR: u64 = 1 << 1;
const VT_D_USER: u64 = 1 << 2;
const VT_D_CACHE_DISABLE: u64 = 1 << 3;
/// Bit 6: Read permission (for superpage).
const VT_D_READ: u64 = 1 << 6;

/// A single Root Entry (RE).
#[repr(C)]
#[derive(Clone, Copy)]
struct RootEntry {
    /// Bits [63:12] = context table physical address, bit 0 = present.
    low: u64,
    /// Reserved.
    high: u64,
}

impl RootEntry {
    const fn empty() -> Self {
        RootEntry { low: 0, high: 0 }
    }

    fn is_present(&self) -> bool {
        self.low & VT_D_PRESENT != 0
    }

    fn set_context_table(&mut self, phys: u64) {
        // Context table must be 4KiB aligned; present bit = 1.
        self.low = (phys & 0x000F_FFFF_FFFF_F000) | VT_D_PRESENT;
        self.high = 0;
    }
}

/// A single Context Entry (CE).
#[repr(C)]
#[derive(Clone, Copy)]
struct ContextEntry {
    /// Bits [63:12] = IO page table physical address (or PASID root).
    /// Bit 0: Present. Bits [3:2] = Translation Type (0=NO牙, 1=Translated, 2=Pass-through).
    /// Bit 7: Fault Processing Disable.
    low: u64,
    /// Bits [31:0] = Source ID (BDF), bits [63:32] = AM (Address Mask for SID).
    high: u64,
}

impl ContextEntry {
    const fn empty() -> Self {
        ContextEntry { low: 0, high: 0 }
    }

    fn is_present(&self) -> bool {
        self.low & VT_D_PRESENT != 0
    }

    /// Set as translated mode pointing to an IO page table.
    fn set_translated(&mut self, iotlb_pt_phys: u64, source_id: u16) {
        // Translation type = 0b01 (translated mode)
        // Bit 0: present, bits [3:2]: type = 01
        self.low = (iotlb_pt_phys & 0x000F_FFFF_FFFF_F000) | VT_D_PRESENT | (0b01 << 2);
        // Source ID = BDF (bus[15:8] | device[7:3] | func[2:0])
        self.high = source_id as u64;
    }
}

/// IO Page Table Entry (level 1, 2, or 3).
#[repr(C)]
#[derive(Clone, Copy)]
struct IoPtEntry {
    val: u64,
}

impl IoPtEntry {
    const fn empty() -> Self {
        IoPtEntry { val: 0 }
    }

    fn set_leaf(&mut self, phys: u64, flags: u64) {
        self.val = (phys & 0x000F_FFFF_FFFF_F000) | flags | VT_D_PRESENT;
    }

    fn set_directory(&mut self, phys: u64, flags: u64) {
        // Directory entry: bits [63:12] = next level table phys, bit 0 = present.
        self.val = (phys & 0x000F_FFFF_FFFF_F000) | flags | VT_D_PRESENT;
    }
}

// ═══════════════════════════════════════════════════════════════════════
// VT-d MMIO Register Offsets
// ═══════════════════════════════════════════════════════════════════════

/// VT-d register offsets (Intel VT-d spec, Table 2-1).
mod regs {
    /// Global Status Register (offset 0x001C for 32-bit read).
    pub const GLOBAL_STATUS: u64 = 0x001C;
    /// Global Command Register (offset 0x0018).
    pub const GLOBAL_CMD: u64 = 0x0018;
    /// Capability Register.
    pub const CAP: u64 = 0x0008;
    /// Extended Capability Register.
    pub const ECAP: u64 = 0x0010;
    /// Root Entry Table Address (low 32 bits, offset 0x0068).
    pub const ROOT_TABLE_LOW: u64 = 0x0068;
    /// Root Entry Table Address (high 32 bits, offset 0x006C).
    pub const ROOT_TABLE_HIGH: u64 = 0x006C;
    /// Context Command Register (offset 0x0028).
    pub const CONTEXT_CMD: u64 = 0x0028;
    /// IOTLB Invalidation Register (offset 0x0008 in Invalidation Queue).
    pub const IOTLB_INV: u64 = 0x0008;
    /// Invalidation Queue Head (offset 0x0080).
    pub const INV_QUEUE_HEAD: u64 = 0x0080;
    /// Invalidation Queue Tail (offset 0x0088).
    pub const INV_QUEUE_TAIL: u64 = 0x0088;
    /// Invalidation Queue Address (low).
    pub const INV_QUEUE_ADDR_LOW: u64 = 0x0090;
    /// Invalidation Queue Address (high).
    pub const INV_QUEUE_ADDR_HIGH: u64 = 0x0094;

    // Global Command bits
    /// SRTP — Set Root Table Pointer.
    pub const GC_SRTP: u32 = 1 << 24;
    /// SIRTLB — Scalable Invalidation.
    pub const GC_SIRTLB: u32 = 1 << 26;
    /// IR — Interrupt Remapping Enable.
    pub const GC_IR: u32 = 1 << 25;
    /// TE — Translation Enable.
    pub const GC_TE: u32 = 1 << 31;
    /// QIE — Queue Invalidation Enable.
    pub const GC_QIE: u32 = 1 << 26;

    // Global Status bits
    /// RTPS — Root Table Pointer Status.
    pub const GS_RTPS: u32 = 1 << 24;
    /// TES — Translation Enable Status.
    pub const GS_TES: u32 = 1 << 31;
    /// IRGS — Invalidation Request/Status Global Status (bit 4).
    pub const GS_IRGS: u32 = 1 << 4;

    // IOTLB Invalidation Register (offset 0x0058)
    /// IOTLB Invalidation Descriptor register.
    pub const IOTLB_INVD: u64 = 0x0058;

    // Context Command bits
    /// Context Invalidation — global.
    pub const CC_GLOBAL: u32 = 1 << 30;
    /// Context Invalidation — device.
    pub const CC_DEVICE: u32 = 1 << 29;
}

// ═══════════════════════════════════════════════════════════════════════
// DMAR ACPI Table Parser
// ═══════════════════════════════════════════════════════════════════════

/// Read a u8 from a physical address via HHDM.
unsafe fn read_phys_u8(phys: u64) -> u8 {
    let virt = crate::memory::physical_memory_offset() + phys;
    core::ptr::read_volatile(virt as *const u8)
}

/// Read a u16 LE from a physical address via HHDM.
/// Uses read_unaligned because DMAR table addresses may not be naturally aligned.
/// x86 hardware supports unaligned MMIO loads.
unsafe fn read_phys_u16(phys: u64) -> u16 {
    let virt = crate::memory::physical_memory_offset() + phys;
    core::ptr::read_unaligned(virt as *const u16)
}

/// Read a u32 LE from a physical address via HHDM.
unsafe fn read_phys_u32(phys: u64) -> u32 {
    let virt = crate::memory::physical_memory_offset() + phys;
    core::ptr::read_unaligned(virt as *const u32)
}

/// Read a u64 LE from a physical address via HHDM.
unsafe fn read_phys_u64(phys: u64) -> u64 {
    let virt = crate::memory::physical_memory_offset() + phys;
    core::ptr::read_unaligned(virt as *const u64)
}

/// Write a u64 to a physical address via HHDM.
unsafe fn write_phys_u64(phys: u64, val: u64) {
    let virt = crate::memory::physical_memory_offset() + phys;
    core::ptr::write_volatile(virt as *mut u64, val);
}

/// Write a u32 to a physical address via HHDM.
unsafe fn write_phys_u32(phys: u64, val: u32) {
    let virt = crate::memory::physical_memory_offset() + phys;
    core::ptr::write_volatile(virt as *mut u32, val);
}

/// Locate the DMAR ACPI table by walking RSDP → RSDT/XSDT.
///
/// Returns the physical address of the DMAR table, or None.
fn locate_dmar_table() -> Option<u64> {
    let rsdp_phys = crate::limine::rsdp_addr()?;
    let pmio = crate::memory::physical_memory_offset();

    // Read RSDP fields
    let rsdp_virt = pmio + rsdp_phys;

    // RSDP revision (offset 15): 0 = ACPI 1.0 (RSDT), 2+ = ACPI 2.0+ (XSDT)
    let revision = unsafe { core::ptr::read_volatile((rsdp_virt + 15) as *const u8) };

    let (table_list_phys, table_count_offset) = if revision >= 2 {
        // XSDT: 64-bit pointers (offset 24)
        let xsdt_phys = unsafe { read_phys_u64(rsdp_phys + 24) };
        if xsdt_phys == 0 {
            return None;
        }
        // XSDT entry count: (total_length - 36) / 8
        let xsdt_length = unsafe { read_phys_u32(xsdt_phys + 4) };
        let count = ((xsdt_length as u64 - 36) / 8) as usize;
        crate::serial_write(&alloc::format!(
            "[IOMMU] XSDT at 0x{:x}, {} entries\n",
            xsdt_phys,
            count
        ));
        (xsdt_phys, count)
    } else {
        // RSDT: 32-bit pointers (offset 24)
        let rsdt_phys = unsafe { read_phys_u32(rsdp_phys + 24) } as u64;
        if rsdt_phys == 0 {
            return None;
        }
        let rsdt_length = unsafe { read_phys_u32(rsdt_phys + 4) };
        let count = ((rsdt_length as u64 - 36) / 4) as usize;
        crate::serial_write(&alloc::format!(
            "[IOMMU] RSDT at 0x{:x}, {} entries\n",
            rsdt_phys,
            count
        ));
        (rsdt_phys, count)
    };

    // Walk the SDT entries to find DMAR
    let is_xsdt = revision >= 2;
    let entry_size = if is_xsdt { 8u64 } else { 4u64 };

    for i in 0..table_count_offset {
        let entry_phys = table_list_phys + 36 + (i as u64) * entry_size;
        let sdt_phys = if is_xsdt {
            unsafe { read_phys_u64(entry_phys) }
        } else {
            unsafe { read_phys_u32(entry_phys) as u64 }
        };

        if sdt_phys == 0 {
            continue;
        }

        // Read signature (first 4 bytes of SDT header)
        let sig = unsafe { read_phys_u32(sdt_phys) };
        if sig == DMAR_SIGNATURE {
            crate::serial_write(&alloc::format!(
                "[IOMMU] DMAR table found at 0x{:x}\n",
                sdt_phys
            ));
            return Some(sdt_phys);
        }
    }

    crate::serial_write("[IOMMU] DMAR table not found\n");
    None
}

/// Parse the DMAR ACPI table at the given physical address.
///
/// Returns the parsed table structure, or None on error.
fn parse_dmar_table(dmar_phys: u64) -> Option<DmarTable> {
    // SDT header: signature(4) + length(4) + revision(1) + checksum(1) + oem_id(6) + oem_table_id(8) + oem_revision(4) + creator_id(4) + creator_revision(4) = 36 bytes
    let table_length = unsafe { read_phys_u32(dmar_phys + 4) } as u64;

    // DMAR-specific fields (after 36-byte header)
    let haw = unsafe { read_phys_u8(dmar_phys + 36) };
    let flags = unsafe { read_phys_u8(dmar_phys + 37) };

    crate::serial_write(&alloc::format!(
        "[IOMMU] DMAR: haw={}, flags=0x{:x}, length={}\n",
        haw,
        flags,
        table_length
    ));

    let mut iommu_units = Vec::new();
    let mut offset: u64 = 48; // Skip header (36) + HAW (1) + flags (1) + reserved (10)

    // Parse remapping structures
    while offset + 4 <= table_length {
        let structure_type = unsafe { read_phys_u16(dmar_phys + offset) };
        let structure_length = unsafe { read_phys_u16(dmar_phys + offset + 2) } as u64;

        if structure_length < 4 {
            break; // Avoid infinite loop on malformed tables
        }

        match structure_type {
            DMAR_TYPE_HARDWARE_UNIT => {
                if let Some(unit) = parse_hardware_unit(dmar_phys + offset, structure_length) {
                    crate::serial_write(&alloc::format!(
                        "[IOMMU] IOMMU unit: segment={}, mmio=0x{:x}, {} device scopes\n",
                        unit.segment,
                        unit.mmio_base,
                        unit.device_scopes.len()
                    ));
                    iommu_units.push(unit);
                }
            }
            DMAR_TYPE_RESERVED_MEMORY => {
                crate::serial_write("[IOMMU] Reserved Memory Region found\n");
            }
            DMAR_TYPE_ATSR => {
                crate::serial_write("[IOMMU] ATSR structure found\n");
            }
            DMAR_TYPE_RHSA => {
                crate::serial_write("[IOMMU] RHSA structure found\n");
            }
            _ => {
                crate::serial_write(&alloc::format!(
                    "[IOMMU] Unknown DMAR structure type 0x{:x}\n",
                    structure_type
                ));
            }
        }

        offset += structure_length;
    }

    Some(DmarTable {
        haw,
        flags,
        iommu_units,
    })
}

/// Parse a DMA Remapping Hardware Unit structure (type 0x00).
///
/// Layout:
///   [0:1]  Type = 0x0000
///   [2:3]  Length
///   [4:5]  Flags
///   [6:7]  Reserved
///   [8:9]  PCI Segment Number
///   [10:17] MMIO Base Address (64-bit)
///   [18..] Device Scope entries
fn parse_hardware_unit(base_phys: u64, total_length: u64) -> Option<DmarIommuUnit> {
    let flags = unsafe { read_phys_u16(base_phys + 4) };
    let segment = unsafe { read_phys_u16(base_phys + 8) };
    let mmio_base = unsafe { read_phys_u64(base_phys + 10) };

    let mut device_scopes = Vec::new();
    let mut ds_offset: u64 = 18; // After fixed fields

    // Parse device scope entries
    while ds_offset + 6 <= total_length {
        let ds_type = unsafe { read_phys_u8(base_phys + ds_offset) };
        let ds_length = unsafe { read_phys_u8(base_phys + ds_offset + 1) } as u64;
        let enumeration_id = unsafe { read_phys_u8(base_phys + ds_offset + 2) };
        let start_bus = unsafe { read_phys_u8(base_phys + ds_offset + 3) };

        if ds_length < 6 || ds_offset + ds_length > total_length {
            break;
        }

        // Parse PCI path: starts at offset +4, each path entry is 2 bytes (device, function)
        let path_bytes = ds_length - 6; // length minus 4-byte header minus 2 reserved bytes
        let mut path = Vec::new();
        let mut path_idx: u64 = 0;
        while path_idx + 2 <= path_bytes {
            let device = unsafe { read_phys_u8(base_phys + ds_offset + 6 + path_idx) };
            let function = unsafe { read_phys_u8(base_phys + ds_offset + 7 + path_idx) };
            path.push((device, function));
            path_idx += 2;
        }

        crate::serial_write(&alloc::format!(
            "[IOMMU]   Device Scope: type=0x{:x} bus={:02x} path={:?}\n",
            ds_type,
            start_bus,
            path
        ));

        device_scopes.push(DeviceScope {
            scope_type: ds_type,
            enumeration_id,
            start_bus,
            path,
        });

        ds_offset += ds_length;
    }

    Some(DmarIommuUnit {
        flags,
        segment,
        mmio_base,
        device_scopes,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// IO Page Table Management
// ═══════════════════════════════════════════════════════════════════════

/// Allocate a 4KiB-aligned physical frame for IO page tables.
///
/// Returns the physical address, or None if allocation fails.
fn alloc_io_frame() -> Option<u64> {
    let frame = crate::memory::buddy::BUDDY_ALLOCATOR
        .lock()
        .allocate_contiguous(0)?;
    let phys = frame.as_u64();
    // Zero the frame
    let virt = crate::memory::physical_memory_offset() + phys;
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, 4096);
    }
    Some(phys)
}

/// IO page table set for a single IOMMU unit.
struct IoPageTables {
    /// Physical address of the Root Entry Table.
    root_table_phys: u64,
    /// MMIO base of this IOMMU unit.
    mmio_base: u64,
    /// PCI segment.
    segment: u16,
}

impl IoPageTables {
    /// Create a new IO page table set for an IOMMU unit.
    fn new(mmio_base: u64, segment: u16) -> Option<Self> {
        let root_table_phys = alloc_io_frame()?;
        crate::serial_write(&alloc::format!(
            "[IOMMU] RET allocated at 0x{:x}\n",
            root_table_phys
        ));
        Some(IoPageTables {
            root_table_phys,
            mmio_base,
            segment,
        })
    }

    /// Get or create the Context Entry for a given BDF.
    // ponytail: raw pointer to physical memory via HHDM — not a borrow of self.
    #[allow(clippy::mut_from_ref)]
    fn get_or_create_context_entry(
        &self,
        bus: u8,
        device: u8,
        function: u8,
    ) -> Option<(&mut ContextEntry, u64)> {
        let ret_virt = crate::memory::physical_memory_offset() + self.root_table_phys;
        let ret = unsafe { &mut *(ret_virt as *mut [RootEntry; VT_D_RET_ENTRIES]) };
        let re = &mut ret[bus as usize];

        if !re.is_present() {
            return None;
        }

        let ct_phys = re.low & 0x000F_FFFF_FFFF_F000;
        let ct_virt = crate::memory::physical_memory_offset() + ct_phys;
        let ct = unsafe { &mut *(ct_virt as *mut [ContextEntry; VT_D_CTE_ENTRIES]) };

        let idx = ((device as usize) << 3) | (function as usize);
        let ce = &mut ct[idx];

        if !ce.is_present() {
            // Allocate IO page table for this device
            let pt_l1_phys = alloc_io_frame()?;
            // BDF as source ID: bus[15:8] | device[7:3] | func[2:0]
            let source_id = ((bus as u16) << 8) | ((device as u16) << 3) | (function as u16);
            ce.set_translated(pt_l1_phys, source_id);
        }

        Some((ce, ct_phys))
    }

    /// Identity-map a single 4KiB page for a device.
    fn map_page(&self, bus: u8, device: u8, function: u8, phys_addr: u64) -> bool {
        let (ce, _ct_phys) = match self.get_or_create_context_entry(bus, device, function) {
            Some(v) => v,
            None => return false,
        };

        let pt_l1_phys = ce.low & 0x000F_FFFF_FFFF_F000;

        // For identity mapping with 4KiB pages:
        // Level 1 (PDPT/PML4 analog): index = bits[47:39] → but for 3-level VT-d:
        //   Level 1 (512 entries): bits[47:30] → each covers 1 GiB
        //   Level 2 (512 entries): bits[29:21] → each covers 2 MiB
        //   Level 3 (512 entries): bits[20:12] → each covers 4 KiB (leaf)
        //
        // For identity mapping: IOVA == phys_addr
        let idx_l1 = ((phys_addr >> 30) & 0x1FF) as usize;

        let l1_virt = crate::memory::physical_memory_offset() + pt_l1_phys;
        let l1_table = unsafe { &mut *(l1_virt as *mut [IoPtEntry; VT_D_PT_ENTRIES]) };

        // Ensure L2 table exists
        if l1_table[idx_l1].val == 0 {
            let l2_phys = match alloc_io_frame() {
                Some(p) => p,
                None => return false,
            };
            l1_table[idx_l1].set_directory(l2_phys, 0);
        }

        let l2_phys = l1_table[idx_l1].val & 0x000F_FFFF_FFFF_F000;
        let idx_l2 = ((phys_addr >> 21) & 0x1FF) as usize;

        let l2_virt = crate::memory::physical_memory_offset() + l2_phys;
        let l2_table = unsafe { &mut *(l2_virt as *mut [IoPtEntry; VT_D_PT_ENTRIES]) };

        // Ensure L3 table exists
        if l2_table[idx_l2].val == 0 {
            let l3_phys = match alloc_io_frame() {
                Some(p) => p,
                None => return false,
            };
            l2_table[idx_l2].set_directory(l3_phys, 0);
        }

        let l3_phys = l2_table[idx_l2].val & 0x000F_FFFF_FFFF_F000;
        let idx_l3 = ((phys_addr >> 12) & 0x1FF) as usize;

        let l3_virt = crate::memory::physical_memory_offset() + l3_phys;
        let l3_table = unsafe { &mut *(l3_virt as *mut [IoPtEntry; VT_D_PT_ENTRIES]) };

        // Identity map: IOVA == phys
        let flags = VT_D_WR | VT_D_READ;
        l3_table[idx_l3].set_leaf(phys_addr, flags);

        true
    }

    /// Identity-map a range [phys_base, phys_base + size) for a device.
    fn map_range(&self, bus: u8, device: u8, function: u8, phys_base: u64, size: u64) {
        let page_size = 4096u64;
        let start = phys_base & !(page_size - 1);
        let end = (phys_base + size + page_size - 1) & !(page_size - 1);
        let mut addr = start;
        let mut mapped = 0u64;
        while addr < end {
            if self.map_page(bus, device, function, addr) {
                mapped += 1;
            }
            addr += page_size;
        }
        if mapped > 0 {
            crate::serial_write(&alloc::format!(
                "[IOMMU] Mapped {} pages for {:02x}:{:02x}.{:x}\n",
                mapped,
                bus,
                device,
                function
            ));
        }
    }

    /// Clear (unmap) 4KiB pages for a device by clearing the present bit.
    fn clear_pages(&self, bus: u8, device: u8, function: u8, phys_base: u64, size: u64) {
        let page_size = 4096u64;
        let start = phys_base & !(page_size - 1);
        let end = (phys_base + size + page_size - 1) & !(page_size - 1);
        let mut addr = start;
        while addr < end {
            self.clear_page(bus, device, function, addr);
            addr += page_size;
        }
    }

    /// Clear a single 4KiB page mapping by clearing the present bit.
    fn clear_page(&self, bus: u8, device: u8, function: u8, phys_addr: u64) {
        // Walk the same 3-level page table as map_page, then clear present.
        let (ce, _ct_phys) = match self.get_or_create_context_entry(bus, device, function) {
            Some(v) => v,
            None => return,
        };
        let pt_l1_phys = ce.low & 0x000F_FFFF_FFFF_F000;
        let idx_l1 = ((phys_addr >> 30) & 0x1FF) as usize;
        let l1_virt = crate::memory::physical_memory_offset() + pt_l1_phys;
        let l1_table = unsafe { &mut *(l1_virt as *mut [IoPtEntry; VT_D_PT_ENTRIES]) };
        if l1_table[idx_l1].val == 0 {
            return;
        }
        let l2_phys = l1_table[idx_l1].val & 0x000F_FFFF_FFFF_F000;
        let idx_l2 = ((phys_addr >> 21) & 0x1FF) as usize;
        let l2_virt = crate::memory::physical_memory_offset() + l2_phys;
        let l2_table = unsafe { &mut *(l2_virt as *mut [IoPtEntry; VT_D_PT_ENTRIES]) };
        if l2_table[idx_l2].val == 0 {
            return;
        }
        let l3_phys = l2_table[idx_l2].val & 0x000F_FFFF_FFFF_F000;
        let idx_l3 = ((phys_addr >> 12) & 0x1FF) as usize;
        let l3_virt = crate::memory::physical_memory_offset() + l3_phys;
        let l3_table = unsafe { &mut *(l3_virt as *mut [IoPtEntry; VT_D_PT_ENTRIES]) };
        // Clear present bit (bit 0) to invalidate the mapping
        l3_table[idx_l3].val &= !VT_D_PRESENT;
    }

    /// Build complete identity-mapped IO page tables for the entire physical
    /// address range. This is a flat identity map covering all of physical memory
    /// so devices can DMA to any address they need.
    fn identity_map_full_range(&self, max_phys: u64) {
        let page_size = 4096u64;
        let end = (max_phys + page_size - 1) & !(page_size - 1);
        let mut addr = 0u64;
        let mut mapped = 0u64;

        while addr < end {
            // We need a BDF to map through. Use a synthetic "all devices" approach:
            // map through function 0 of device 0, bus 0 (catches most devices)
            // Also map through the specific device scopes later.
            if self.map_page(0, 0, 0, addr) {
                mapped += 1;
            }
            addr += page_size;
        }

        crate::serial_write(&alloc::format!(
            "[IOMMU] Identity-mapped {} pages ({} MiB) into default context\n",
            mapped,
            mapped * 4096 / (1024 * 1024)
        ));
    }

    /// Program the VT-d MMIO registers to enable translation.
    fn program_hardware(&self) {
        let mmio = self.mmio_base;
        crate::serial_write(&alloc::format!(
            "[IOMMU] Programming IOMMU at MMIO 0x{:x}\n",
            mmio
        ));

        unsafe {
            // Read capability
            let cap = read_phys_u64(mmio + regs::CAP);
            let max_domains = ((cap >> 56) & 0xFF) as u16;
            let mgaw = (cap & 0x3F) as u8;
            crate::serial_write(&alloc::format!(
                "[IOMMU] CAP: max_domains={}, mgaw={}\n",
                max_domains,
                mgaw
            ));

            // Read extended capability
            let ecap = read_phys_u64(mmio + regs::ECAP);
            let iotlb = (ecap >> 5) & 0x1F;
            let sc = (ecap >> 4) & 1;
            crate::serial_write(&alloc::format!(
                "[IOMMU] ECAP: iotlb_modes={}, sc_support={}\n",
                iotlb,
                sc
            ));

            // Set Root Table Pointer (64-bit address split across two 32-bit regs)
            let rt_low = (self.root_table_phys as u32) | 1; // bit 0 = present
            let rt_high = (self.root_table_phys >> 32) as u32;
            write_phys_u32(mmio + regs::ROOT_TABLE_LOW, rt_low);
            write_phys_u32(mmio + regs::ROOT_TABLE_HIGH, rt_high);

            // Issue SRTP command and wait
            write_phys_u32(mmio + regs::GLOBAL_CMD, regs::GC_SRTP);
            wait_for_status(mmio, regs::GS_RTPS, "SRTP");

            // Invalidate context cache (global)
            write_phys_u64(mmio + regs::CONTEXT_CMD, regs::CC_GLOBAL as u64);
            wait_for_context_invalidation(mmio);

            crate::serial_write("[IOMMU] Root table set, context cache invalidated\n");
        }
    }

    /// Enable translation by setting the TE bit.
    fn enable_translation(&self) {
        let mmio = self.mmio_base;
        unsafe {
            let current_cmd = read_phys_u32(mmio + regs::GLOBAL_CMD);
            write_phys_u32(mmio + regs::GLOBAL_CMD, current_cmd | regs::GC_TE);
            wait_for_status(mmio, regs::GS_TES, "TE");
            crate::serial_write("[IOMMU] Translation Enabled\n");
        }
    }
}

/// Wait for a status bit to be set in the Global Status register.
unsafe fn wait_for_status(mmio: u64, bit: u32, name: &str) {
    // Timeout after ~1000 iterations (avoid infinite hang)
    for _ in 0..1000 {
        let status = read_phys_u32(mmio + regs::GLOBAL_STATUS);
        if status & bit != 0 {
            return;
        }
        core::hint::spin_loop();
    }
    crate::serial_write(&alloc::format!(
        "[IOMMU] WARNING: timeout waiting for {}\n",
        name
    ));
}

/// Wait for context invalidation to complete.
unsafe fn wait_for_context_invalidation(mmio: u64) {
    for _ in 0..1000 {
        let cmd = read_phys_u64(mmio + regs::CONTEXT_CMD);
        if cmd & (1u64 << 63) == 0 {
            // ICC bit cleared = done
            return;
        }
        core::hint::spin_loop();
    }
    crate::serial_write("[IOMMU] WARNING: timeout waiting for context invalidation\n");
}

/// Perform a full-domain IOTLB invalidation on the given IOMMU unit.
///
/// Writes an INVALIDATION_DESCRIPTOR to the IOTLB_INVD register
/// (offset 0x0058) with domain=0xFFFF and source_id=0xFFFF
/// (global invalidation), then polls the IRGS bit in Global Status
/// until it clears, confirming the invalidation completed.
///
/// This must be called after any IO page table modification
/// (map/unmap) so the IOMMU does not use stale cached translations.
unsafe fn invalidate_iotlb_global(mmio: u64) {
    // IOTLB_INVD descriptor layout (Intel VT-d spec 5.2.3):
    //   bits [1:0]   = type: 0b10 = write-invalidate
    //   bits [3:2]   = granularity: 0b11 = don't-care (global)
    //   bits [31:16] = reserved
    //   bits [47:32] = Domain ID: 0xFFFF = global
    //   bits [63:48] = Source ID: 0xFFFF = global
    //   bit  [63]    = 1 = initiate invalidation
    let descriptor: u64 = (1u64 << 63)       // Invalidate bit
        | (0xFFFFu64 << 48)                   // Source ID = global
        | (0xFFFFu64 << 32)                   // Domain ID = global
        | (0b11u64 << 2)                      // Granularity = don't-care
        | (0b10u64 << 0); // Type = write-invalidate

    write_phys_u64(mmio + regs::IOTLB_INVD, descriptor);

    // Poll IRGS (bit 4) in Global Status register until it clears,
    // confirming the hardware completed the invalidation.
    for _ in 0..10000 {
        let status = read_phys_u32(mmio + regs::GLOBAL_STATUS);
        if status & regs::GS_IRGS == 0 {
            return;
        }
        core::hint::spin_loop();
    }
    crate::serial_write("[IOMMU] WARNING: timeout waiting for IOTLB invalidation\n");
}

// ═══════════════════════════════════════════════════════════════════════
// Global State
// ═══════════════════════════════════════════════════════════════════════

/// Whether IOMMU hardware is present and initialized.
static IOMMU_ENABLED: AtomicBool = AtomicBool::new(false);

/// IOVA region: a contiguous range mapped to physical memory.
#[derive(Debug, Clone)]
struct IovaRegion {
    iova: u64,
    phys: u64,
    size: u64,
    flags: u64,
}

/// Per-device-group IOMMU context.
struct IommuContext {
    bdf: u16,
    mmio_base: u64,
    mappings: BTreeMap<u64, IovaRegion>,
    root_table_phys: u64,
    programmed: bool,
}

/// Global IOMMU state.
pub struct IommuState {
    contexts: BTreeMap<u16, IommuContext>,
    default_context: Option<IommuContext>,
    next_iova: u64,
    /// Parsed DMAR table (populated at init).
    dmar: Option<DmarTable>,
    /// IO page table sets per IOMMU unit, keyed by MMIO base.
    io_tables: BTreeMap<u64, IoPageTables>,
}

static IOMMU_STATE: Mutex<IommuState> = Mutex::new(IommuState {
    contexts: BTreeMap::new(),
    default_context: None,
    next_iova: 0x1_0000_0000,
    dmar: None,
    io_tables: BTreeMap::new(),
});

const IOMMU_PAGE_SIZE: u64 = 4096;

// ═══════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════

/// Initialize the IOMMU subsystem.
///
/// Called during boot after ACPI and PCI enumeration.
/// Parses the DMAR ACPI table, builds IO page tables, and enables
/// DMA remapping if hardware is present.
pub fn init() {
    crate::serial_write("[IOMMU] init...\n");

    // Step 1: Locate and parse the DMAR ACPI table
    let dmar_phys = match locate_dmar_table() {
        Some(p) => p,
        None => {
            crate::serial_write("[IOMMU] no DMAR table — passthrough mode\n");
            crate::serial_write("[IOMMU] init done\n");
            return;
        }
    };

    let dmar = match parse_dmar_table(dmar_phys) {
        Some(t) => t,
        None => {
            crate::serial_write("[IOMMU] failed to parse DMAR table — passthrough mode\n");
            crate::serial_write("[IOMMU] init done\n");
            return;
        }
    };

    if dmar.iommu_units.is_empty() {
        crate::serial_write("[IOMMU] DMAR table has no IOMMU units — passthrough mode\n");
        crate::serial_write("[IOMMU] init done\n");
        return;
    }

    crate::serial_write(&alloc::format!(
        "[IOMMU] DMAR: {} IOMMU unit(s) found\n",
        dmar.iommu_units.len()
    ));

    // Step 2: Map IOMMU MMIO regions into HHDM and build IO page tables
    let max_phys = crate::limine::max_physical_address();
    let mut state = IOMMU_STATE.lock();

    for unit in &dmar.iommu_units {
        crate::serial_write(&alloc::format!(
            "[IOMMU] Unit: segment={}, mmio=0x{:x}, flags=0x{:x}\n",
            unit.segment,
            unit.mmio_base,
            unit.flags
        ));

        // The IOMMU MMIO region should already be mapped into HHDM by the
        // boot loop in main.rs which maps all Limine memory map entries.
        // Verify the MMIO base is accessible via a probe read.
        let mmio_probe = unsafe { read_phys_u64(unit.mmio_base) };
        crate::serial_write(&alloc::format!(
            "[IOMMU] MMIO probe at 0x{:x}: 0x{:016x}
",
            unit.mmio_base,
            mmio_probe
        ));

        // Create IO page tables for this IOMMU unit
        let unit_mmio = unit.mmio_base;
        match IoPageTables::new(unit_mmio, unit.segment) {
            Some(io_tables) => {
                // Build identity mapping for device scopes
                for scope in &unit.device_scopes {
                    if scope.scope_type == DEV_SCOPE_PCI_ENDPOINT
                        || scope.scope_type == DEV_SCOPE_PCI_SUBHIERARCHY
                    {
                        for &(dev, func) in &scope.path {
                            io_tables.map_range(
                                scope.start_bus,
                                dev,
                                func,
                                0,
                                max_phys.min(256 * 1024 * 1024),
                            );
                        }
                    }
                }

                // Default identity map for unscoped devices (bus 0, dev 0, func 0)
                io_tables.identity_map_full_range(max_phys.min(256 * 1024 * 1024));

                io_tables.program_hardware();

                state.io_tables.insert(unit_mmio, io_tables);
            }
            None => {
                crate::serial_write("[IOMMU] Failed to allocate IO page tables\n");
            }
        }

        // Store device scope → BDF mapping for later use
        for scope in &unit.device_scopes {
            for &(dev, func) in &scope.path {
                let bdf = ((scope.start_bus as u16) << 8) | ((dev as u16) << 3) | (func as u16);
                let ctx = IommuContext {
                    bdf,
                    mmio_base: unit.mmio_base,
                    mappings: BTreeMap::new(),
                    root_table_phys: 0,
                    programmed: false,
                };
                state.contexts.insert(bdf, ctx);
            }
        }
    }

    // Step 3: Enable translation on all units
    for io_tables in state.io_tables.values() {
        io_tables.enable_translation();
    }

    // Store parsed DMAR for later queries
    state.dmar = Some(dmar);

    IOMMU_ENABLED.store(true, Ordering::Release);
    crate::serial_write("[IOMMU] DMA remapping enabled\n");
    crate::serial_write("[IOMMU] init done\n");
}

/// Map an IOVA region for a device.
pub fn iommu_map(device_bdf: u16, iova: u64, phys: u64, size: u64, flags: u64) -> u64 {
    let page_size = IOMMU_PAGE_SIZE;
    let aligned_iova = iova & !(page_size - 1);
    let aligned_phys = phys & !(page_size - 1);
    let aligned_size = (size + page_size - 1) & !(page_size - 1);

    let region = IovaRegion {
        iova: aligned_iova,
        phys: aligned_phys,
        size: aligned_size,
        flags,
    };

    let mut state = IOMMU_STATE.lock();

    {
        let ctx = state
            .contexts
            .entry(device_bdf)
            .or_insert_with(|| IommuContext {
                bdf: device_bdf,
                mmio_base: 0,
                mappings: BTreeMap::new(),
                root_table_phys: 0,
                programmed: false,
            });
        ctx.mappings.insert(aligned_iova, region);
    }

    let mmio_to_invalidate = if IOMMU_ENABLED.load(Ordering::Acquire) {
        let bus = (device_bdf >> 8) as u8;
        let device = ((device_bdf >> 3) & 0x1F) as u8;
        let function = (device_bdf & 0x7) as u8;

        let ctx_mmio = state
            .contexts
            .get(&device_bdf)
            .map(|c| c.mmio_base)
            .unwrap_or(0);
        let mut mmio = 0u64;
        if let Some(io_tables) = state.io_tables.get(&ctx_mmio) {
            let mut addr = aligned_phys;
            let end = aligned_phys + aligned_size;
            while addr < end {
                io_tables.map_page(bus, device, function, addr);
                addr += page_size;
            }
            mmio = io_tables.mmio_base;
        }
        mmio
    } else {
        0
    };

    // Flush stale IOTLB entries after page table modifications
    if mmio_to_invalidate != 0 {
        // SAFETY: mmio_to_invalidate is a valid IOMMU MMIO base from init
        unsafe { invalidate_iotlb_global(mmio_to_invalidate) };
    }

    aligned_iova
}

/// Unmap an IOVA region for a device.
pub fn iommu_unmap(device_bdf: u16, iova: u64, size: u64) -> bool {
    let page_size = IOMMU_PAGE_SIZE;
    let aligned_iova = iova & !(page_size - 1);
    let aligned_size = (size + page_size - 1) & !(page_size - 1);

    let mut state = IOMMU_STATE.lock();

    let mut removed = false;
    let mut to_remove = Vec::new();
    if let Some(ctx) = state.contexts.get_mut(&device_bdf) {
        for (&iova_addr, region) in ctx
            .mappings
            .range(aligned_iova..aligned_iova + aligned_size)
        {
            to_remove.push((iova_addr, region.phys, region.size));
            removed = true;
        }
        for &(addr, _, _) in &to_remove {
            ctx.mappings.remove(&addr);
        }
    }

    if removed {
        // Clear page table entries and flush IOTLB
        let bus = (device_bdf >> 8) as u8;
        let device = ((device_bdf >> 3) & 0x1F) as u8;
        let function = (device_bdf & 0x7) as u8;

        let ctx_mmio = state
            .contexts
            .get(&device_bdf)
            .map(|c| c.mmio_base)
            .unwrap_or(0);
        if let Some(io_tables) = state.io_tables.get(&ctx_mmio) {
            for &(_iova_addr, phys_base, phys_size) in &to_remove {
                io_tables.clear_pages(bus, device, function, phys_base, phys_size);
            }
            // Flush stale IOTLB entries after page table modifications
            unsafe { invalidate_iotlb_global(io_tables.mmio_base) };
        }
    }

    removed
}

/// Allocate a fresh IOVA from the global pool.
pub fn alloc_iova(size: u64) -> u64 {
    let page_size = IOMMU_PAGE_SIZE;
    let aligned_size = (size + page_size - 1) & !(page_size - 1);
    let mut state = IOMMU_STATE.lock();
    let iova = state.next_iova;
    state.next_iova += aligned_size;
    iova
}

/// Get the physical address for an IOVA on a given device.
pub fn iommu_translate(device_bdf: u16, iova: u64) -> Option<u64> {
    let state = IOMMU_STATE.lock();
    if let Some(ctx) = state.contexts.get(&device_bdf) {
        if let Some((&_iova_base, region)) = ctx.mappings.range(..=iova).next_back() {
            if iova < region.iova + region.size {
                let offset = iova - region.iova;
                return Some(region.phys + offset);
            }
        }
    }
    None
}

/// Check if IOMMU hardware is enabled.
pub fn is_enabled() -> bool {
    IOMMU_ENABLED.load(Ordering::Acquire)
}

/// Get the parsed DMAR table (if available).
pub fn dmar_table() -> Option<DmarTable> {
    IOMMU_STATE.lock().dmar.clone()
}

/// Get the list of device scope BDFs for a given MMIO base.
pub fn get_scoped_devices(mmio_base: u64) -> Vec<u16> {
    let state = IOMMU_STATE.lock();
    state
        .contexts
        .iter()
        .filter(|(_, ctx)| ctx.mmio_base == mmio_base)
        .map(|(&bdf, _)| bdf)
        .collect()
}

/// Set the MMIO base for an IOMMU context (called during PCI enumeration).
pub fn set_context_mmio(device_bdf: u16, mmio_base: u64) {
    let mut state = IOMMU_STATE.lock();
    if let Some(ctx) = state.contexts.get_mut(&device_bdf) {
        ctx.mmio_base = mmio_base;
    }
}

/// vahi-arch IOMMU trait implementation for the kernel's IOMMU subsystem.
#[cfg(feature = "iommu")]
mod vahi_arch_iommu {
    use alloc::sync::Arc;
    use vahi_arch::iommu::Iommu;

    struct KernelIommu;

    impl Iommu for KernelIommu {
        fn map(&self, device_bdf: u16, iova: u64, phys: u64, size: u64, flags: u64) -> u64 {
            super::iommu_map(device_bdf, iova, phys, size, flags)
        }

        fn unmap(&self, device_bdf: u16, iova: u64, size: u64) -> bool {
            super::iommu_unmap(device_bdf, iova, size)
        }

        fn translate(&self, device_bdf: u16, iova: u64) -> Option<u64> {
            super::iommu_translate(device_bdf, iova)
        }

        fn is_enabled(&self) -> bool {
            super::is_enabled()
        }
    }

    /// Register the kernel's IOMMU implementation with vahi-arch.
    pub fn register() {
        vahi_arch::iommu::register_iommu(Arc::new(KernelIommu));
    }
}

#[cfg(feature = "iommu")]
pub use vahi_arch_iommu::register as register_vahi_arch_iommu;

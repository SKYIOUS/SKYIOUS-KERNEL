use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;
use crate::drivers::block::{BlockDevice, BlockDeviceError};

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MbrPartitionEntry {
    pub boot_flag: u8,
    pub chs_start: [u8; 3],
    pub type_: u8,
    pub chs_end: [u8; 3],
    pub lba_start: u32,
    pub sector_count: u32,
}

#[derive(Debug, Clone)]
pub struct Partition {
    pub index: u8,
    pub lba_start: u64,
    pub sector_count: u64,
    pub type_: u8,
    pub is_gpt: bool,
    pub gpt_type_guid: [u8; 16],
    pub name: [u8; 72],
}

pub fn parse_mbr(device: &Arc<Mutex<dyn BlockDevice>>) -> Result<Vec<Partition>, ()> {
    let mut mbr = [0u8; 512];
    device.lock().read_sector(0, &mut mbr).map_err(|_| ())?;

    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return Err(());
    }

    let mut parts = Vec::new();
    for i in 0..4 {
        let offset = 0x1BE + i * 16;
        let entry: MbrPartitionEntry = unsafe { core::ptr::read_unaligned(mbr.as_ptr().add(offset) as *const MbrPartitionEntry) };
        if entry.type_ == 0 {
            continue;
        }
        let lba_start = entry.lba_start as u64;
        let sector_count = entry.sector_count as u64;
        if sector_count == 0 {
            continue;
        }
        parts.push(Partition {
            index: (i + 1) as u8,
            lba_start,
            sector_count,
            type_: entry.type_,
            is_gpt: false,
            gpt_type_guid: [0u8; 16],
            name: [0u8; 72],
        });
    }
    Ok(parts)
}

fn read_gpt_entries(device: &Arc<Mutex<dyn BlockDevice>>, entry_lba: u64, num_entries: u32, entry_size: u32) -> Result<Vec<Partition>, ()> {
    let mut parts = Vec::new();
    let entries_per_sector = 512 / entry_size as usize;
    let mut buf = [0u8; 512];

    for i in 0..num_entries as usize {
        let sector_idx = entry_lba + (i as u64 / entries_per_sector as u64);
        let entry_offset = (i % entries_per_sector) * entry_size as usize;

        if i % entries_per_sector == 0 {
            device.lock().read_sector(sector_idx, &mut buf).map_err(|_| ())?;
        }

        let entry_ptr = unsafe { buf.as_ptr().add(entry_offset) };
        let type_guid = unsafe { core::ptr::read_unaligned(entry_ptr as *const [u8; 16]) };

        if type_guid == [0u8; 16] {
            continue;
        }

        let _unique_guid = unsafe { core::ptr::read_unaligned(entry_ptr.add(16) as *const [u8; 16]) };
        let starting_lba = unsafe { core::ptr::read_unaligned(entry_ptr.add(32) as *const u64) };
        let ending_lba = unsafe { core::ptr::read_unaligned(entry_ptr.add(40) as *const u64) };
        let _attributes = unsafe { core::ptr::read_unaligned(entry_ptr.add(48) as *const u64) };
        let mut name = [0u8; 72];
        for j in 0..36 {
            let c = unsafe { core::ptr::read_unaligned(entry_ptr.add(56 + j * 2) as *const u16) };
            name[j] = if c < 256 { c as u8 } else { b'?' };
        }

        let sector_count = ending_lba.wrapping_sub(starting_lba).wrapping_add(1);

        parts.push(Partition {
            index: (i + 1) as u8,
            lba_start: starting_lba,
            sector_count,
            type_: 0xEE,
            is_gpt: true,
            gpt_type_guid: type_guid,
            name,
        });
    }
    Ok(parts)
}

pub fn parse_gpt(device: &Arc<Mutex<dyn BlockDevice>>) -> Result<Vec<Partition>, ()> {
    let mut mbr = [0u8; 512];
    device.lock().read_sector(0, &mut mbr).map_err(|_| ())?;

    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return Err(());
    }

    let mut header = [0u8; 512];
    device.lock().read_sector(1, &mut header).map_err(|_| ())?;

    let signature = &header[0..8];
    if signature != b"EFI PART" {
        return Err(());
    }

    let _revision = u32::from_le_bytes(header[8..12].try_into().unwrap());
    let header_size = u32::from_le_bytes(header[12..16].try_into().unwrap());
    let entry_lba = u64::from_le_bytes(header[72..80].try_into().unwrap());
    let num_entries = u32::from_le_bytes(header[80..84].try_into().unwrap());
    let entry_size = u32::from_le_bytes(header[84..88].try_into().unwrap());

    if header_size < 92 || entry_size < 128 {
        return Err(());
    }

    read_gpt_entries(device, entry_lba, num_entries, entry_size)
}

pub fn parse_partitions(device: &Arc<Mutex<dyn BlockDevice>>) -> Vec<Partition> {
    if let Ok(parts) = parse_gpt(device) {
        if !parts.is_empty() {
            return parts;
        }
    }
    if let Ok(parts) = parse_mbr(device) {
        return parts;
    }
    Vec::new()
}

pub struct PartitionDevice {
    parent: Arc<Mutex<dyn BlockDevice>>,
    lba_start: u64,
    sector_count: u64,
}

impl PartitionDevice {
    pub fn new(parent: Arc<Mutex<dyn BlockDevice>>, lba_start: u64, sector_count: u64) -> Self {
        PartitionDevice { parent, lba_start, sector_count }
    }
}

impl BlockDevice for PartitionDevice {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        if sector >= self.sector_count {
            return Err(BlockDeviceError::InvalidSector);
        }
        self.parent.lock().read_sector(self.lba_start + sector, buf)
    }

    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        if sector >= self.sector_count {
            return Err(BlockDeviceError::InvalidSector);
        }
        self.parent.lock().write_sector(self.lba_start + sector, buf)
    }

    fn sector_count(&self) -> Result<u64, BlockDeviceError> {
        Ok(self.sector_count)
    }
}

pub struct PartitionInfo {
    pub parent_index: usize,
    pub partition: Partition,
}

pub fn scan_all_partitions() -> Vec<PartitionInfo> {
    let mut result = Vec::new();
    let devices = crate::drivers::block::BLOCK_DEVICES.lock();
    for (i, dev) in devices.iter().enumerate() {
        let parts = parse_partitions(dev);
        for p in parts {
            result.push(PartitionInfo { parent_index: i, partition: p });
        }
    }
    result
}

use super::{SkyFS, BLOCK_SIZE};
use crate::drivers::block::BlockDevice;
use crate::alloc::sync::Arc;
use spin::Mutex;

const JOURNAL_MAGIC: u64 = 0x4A4F55524E414C5F;
const MAX_TRANSACTION_BLOCKS: u32 = 256;

#[repr(C, packed)]
struct JournalHeader {
    magic: u64,
    sequence: u64,
    num_blocks: u32,
    checksum: u32,
    state: u8,
    _pad: [u8; 4059],
}

#[repr(C, packed)]
struct JournalBlock {
    data: [u8; BLOCK_SIZE],
}

pub struct Journal {
    pub start_block: u64,
    pub num_blocks: u64,
    pub sequence: u64,
    pub next_free: u64,
}

impl Journal {
    pub fn new(start_block: u64, num_blocks: u64) -> Self {
        Journal { start_block, num_blocks, sequence: 1, next_free: 1 }
    }

    pub fn init_device(dev: &mut dyn BlockDevice, start_block: u64, num_blocks: u64) -> Result<(), ()> {
        let hdr = JournalHeader {
            magic: JOURNAL_MAGIC,
            sequence: 0,
            num_blocks: num_blocks as u32,
            checksum: 0,
            state: 0,
            _pad: [0u8; 4059],
        };
        let mut buf = [0u8; BLOCK_SIZE];
        let src = unsafe {
            core::slice::from_raw_parts(&hdr as *const JournalHeader as *const u8, core::mem::size_of::<JournalHeader>())
        };
        buf[..src.len()].copy_from_slice(src);
        SkyFS::write_block(dev, start_block, &buf)?;
        for i in 1..num_blocks {
            let zero = [0u8; BLOCK_SIZE];
            SkyFS::write_block(dev, start_block + i, &zero)?;
        }
        Ok(())
    }

    pub fn begin_transaction(dev: &mut dyn BlockDevice, journal: &mut Journal) -> Result<u64, ()> {
        if journal.next_free + 1 >= journal.num_blocks {
            journal.next_free = 1;
        }
        journal.sequence += 1;
        let seq = journal.sequence;
        let block = journal.start_block + journal.next_free;
        let hdr = JournalHeader {
            magic: JOURNAL_MAGIC,
            sequence: seq,
            num_blocks: 1,
            checksum: 0,
            state: 1,
            _pad: [0u8; 4059],
        };
        let mut buf = [0u8; BLOCK_SIZE];
        let src = unsafe {
            core::slice::from_raw_parts(&hdr as *const JournalHeader as *const u8, core::mem::size_of::<JournalHeader>())
        };
        buf[..src.len()].copy_from_slice(src);
        SkyFS::write_block(dev, block, &buf)?;
        journal.next_free += 1;
        Ok(block)
    }

    pub fn commit_transaction(dev: &mut dyn BlockDevice, _journal: &mut Journal, header_block: u64) -> Result<(), ()> {
        let mut buf = [0u8; BLOCK_SIZE];
        SkyFS::read_block(dev, header_block, &mut buf)?;
        let hdr: &mut JournalHeader = unsafe { &mut *(buf.as_mut_ptr() as *mut JournalHeader) };
        hdr.state = 2;
        let checksum = simple_checksum(&buf);
        hdr.checksum = checksum;
        SkyFS::write_block(dev, header_block, &buf)
    }

    pub fn journal_data(dev: &mut dyn BlockDevice, journal: &mut Journal, data: &[u8]) -> Result<u64, ()> {
        if journal.next_free >= journal.num_blocks {
            return Err(());
        }
        let jblock = journal.start_block + journal.next_free;
        let mut buf = [0u8; BLOCK_SIZE];
        let len = data.len().min(BLOCK_SIZE);
        buf[..len].copy_from_slice(&data[..len]);
        SkyFS::write_block(dev, jblock, &buf)?;
        journal.next_free += 1;
        Ok(jblock)
    }

    pub fn recover_from_dev(dev: &mut dyn BlockDevice, journal: &mut Journal) -> Result<(), ()> {
        for i in 0..journal.num_blocks {
            let block = journal.start_block + i;
            let mut buf = [0u8; BLOCK_SIZE];
            SkyFS::read_block(dev, block, &mut buf)?;
            let hdr: &JournalHeader = unsafe { &*(buf.as_ptr() as *const JournalHeader) };
            if hdr.magic == JOURNAL_MAGIC && hdr.state == 1 {
                let expected_cs = simple_checksum(&buf);
                if hdr.checksum == 0 || hdr.checksum == expected_cs {
                    for j in 1..hdr.num_blocks as u64 {
                        let jb = block + j;
                        let mut dbuf = [0u8; BLOCK_SIZE];
                        SkyFS::read_block(dev, jb, &mut dbuf)?;
                        crate::println!("JOURNAL: replayed block {}", jb);
                    }
                }
            }
        }
        journal.sequence = 0;
        journal.next_free = 1;
        Ok(())
    }

    pub fn recover(fs: &Arc<Mutex<SkyFS>>, journal: &mut Journal) -> Result<(), ()> {
        let dev_arc = fs.lock().device.clone();
        let mut dev = dev_arc.lock();
        for i in 0..journal.num_blocks {
            let block = journal.start_block + i;
            let mut buf = [0u8; BLOCK_SIZE];
            SkyFS::read_block(&mut *dev, block, &mut buf)?;
            let hdr: &JournalHeader = unsafe { &*(buf.as_ptr() as *const JournalHeader) };
            if hdr.magic == JOURNAL_MAGIC && hdr.state == 1 {
                let expected_cs = simple_checksum(&buf);
                if hdr.checksum == 0 || hdr.checksum == expected_cs {
                    for j in 1..hdr.num_blocks as u64 {
                        let jb = block + j;
                        let mut dbuf = [0u8; BLOCK_SIZE];
                        SkyFS::read_block(&mut *dev, jb, &mut dbuf)?;
                        crate::println!("JOURNAL: replayed block {}", jb);
                    }
                }
            }
        }
        journal.sequence = 0;
        journal.next_free = 1;
        Ok(())
    }
}

fn simple_checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
}

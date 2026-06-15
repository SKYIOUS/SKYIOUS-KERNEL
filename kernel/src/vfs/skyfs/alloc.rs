#![allow(dead_code)]
use super::{SkyFS, BLOCK_SIZE};
use crate::drivers::block::BlockDevice;
use crate::alloc::vec;
use crate::alloc::sync::Arc;
use spin::Mutex;

pub fn allocate_block_inner(fs: &Arc<Mutex<SkyFS>>, dev: &mut dyn BlockDevice) -> Result<u64, ()> {
    let sb = &fs.lock().sb;
    let total_bitmap_blocks = sb.bitmap_blocks;
    let mut bitmap_buf = vec![0u8; BLOCK_SIZE];

    for b in 0..total_bitmap_blocks {
        let block_num = sb.bitmap_start + b;
        SkyFS::read_block(dev, block_num, &mut bitmap_buf)?;
        for byte_idx in 0..BLOCK_SIZE {
            let byte = bitmap_buf[byte_idx];
            if byte != 0xFF {
                for bit_idx in 0..8 {
                    if byte & (1 << bit_idx) == 0 {
                        bitmap_buf[byte_idx] |= 1 << bit_idx;
                        SkyFS::write_block(dev, block_num, &bitmap_buf)?;
                        let block = (b * BLOCK_SIZE as u64 + byte_idx as u64) * 8 + bit_idx as u64;
                        if block < sb.total_blocks {
                            return Ok(block);
                        }
                    }
                }
            }
        }
    }
    Err(())
}

#[allow(dead_code)]
pub fn free_block(fs: &Arc<Mutex<SkyFS>>, block: u64) -> Result<(), ()> {
    let sb = &fs.lock().sb;
    let bit_index = block;
    let bitmap_block = sb.bitmap_start + bit_index / (BLOCK_SIZE as u64 * 8);
    let byte_index = (bit_index / 8) as usize % BLOCK_SIZE;
    let bit_offset = (bit_index % 8) as u8;

    let binding = fs.lock();
    let mut dev = binding.device.lock();
    let mut buf = [0u8; BLOCK_SIZE];
    SkyFS::read_block(&mut *dev, bitmap_block, &mut buf)?;
    buf[byte_index] &= !(1 << bit_offset);
    SkyFS::write_block(&mut *dev, bitmap_block, &buf)
}

#![allow(dead_code)]
use super::{SkyFS, SkyfsInode, INODE_SIZE, BLOCK_SIZE};
use crate::alloc::sync::Arc;
use spin::Mutex;

pub fn alloc_inode_inner(fs: &Arc<Mutex<SkyFS>>) -> Result<u64, ()> {
    let sb = &fs.lock().sb;
    let total_inodes = sb.inode_count;
    let inodes_per_block = BLOCK_SIZE as u64 / INODE_SIZE as u64;

    let binding = fs.lock();
    let mut dev = binding.device.lock();
    let mut buf = [0u8; BLOCK_SIZE];

    for ino in 1..total_inodes {
        let block = sb.inode_start + ino / inodes_per_block;
        SkyFS::read_block(&mut *dev, block, &mut buf)?;
        let offset = ((ino % inodes_per_block) * INODE_SIZE as u64) as usize;
        let raw = &buf[offset..offset + INODE_SIZE];
        if raw.len() < 2 { continue; }
        let mode = u16::from_le_bytes([raw[0], raw[1]]);
        if mode == 0 {
            return Ok(ino);
        }
    }
    Err(())
}

pub fn init_inode_inner(fs: &Arc<Mutex<SkyFS>>, ino: u64, mode: u32) -> Result<(), ()> {
    let inode = SkyfsInode {
        mode: mode as u16,
        uid: 0,
        gid: 0,
        size: 0,
        atime: 0,
        mtime: 0,
        ctime: 0,
        links: 1,
        flags: 0,
        block_count: 0,
        extent_count: 0,
        data: [0u8; 256],
    };

    let sb = &fs.lock().sb;
    let inodes_per_block = BLOCK_SIZE as u64 / INODE_SIZE as u64;
    let block = sb.inode_start + ino / inodes_per_block;
    let offset = ((ino % inodes_per_block) * INODE_SIZE as u64) as usize;

    let binding = fs.lock();
    let mut dev = binding.device.lock();
    let mut buf = [0u8; BLOCK_SIZE];
    SkyFS::read_block(&mut *dev, block, &mut buf)?;
    let dst = &mut buf[offset..offset + INODE_SIZE];
    let src = unsafe { core::slice::from_raw_parts(&inode as *const SkyfsInode as *const u8, INODE_SIZE) };
    dst.copy_from_slice(src);
    SkyFS::write_block(&mut *dev, block, &buf)
}

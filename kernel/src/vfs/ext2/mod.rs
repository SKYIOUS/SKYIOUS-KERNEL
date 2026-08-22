//! EXT2 filesystem implementation.
//!
//! This module provides a read/write EXT2 driver for the Vahi kernel,
//! decomposed into:
//! - `types`      – on-disk structures and constants
//! - `allocation` – block/inode allocation and bitmap management
//! - `directory`  – directory entry find/add/remove
//! - `mod`        – core file I/O, indirect block mapping, VFS glue

mod allocation;
mod directory;
pub(crate) mod types;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use crate::drivers::block::BlockDevice;
use crate::sync::IrqSafeMutex as Mutex;
use crate::vfs::{FileSystem, VfsNode};

use types::{Inode, S_IFMT};

/// Core in-memory representation of a mounted EXT2 filesystem.
pub struct Ext2FileSystem {
    pub(crate) device: Arc<Mutex<dyn BlockDevice>>,
    pub(crate) block_size: usize,
    pub(crate) inodes_per_group: u32,
    pub(crate) inode_size: u16,
    pub(crate) blocks_per_group: u32,
    _total_blocks: u32,
    _total_inodes: u32,
}

impl Ext2FileSystem {
    pub fn new(
        device: Arc<Mutex<dyn BlockDevice>>,
    ) -> Result<Arc<Mutex<Self>>, ()> {
        let mut buf = [0u8; 1024];
        device
            .lock()
            .read_sector(2, &mut buf)
            .map_err(|_| ())?;
        let sb =
            unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const types::Superblock) };
        if sb.s_magic != types::EXT2_SUPER_MAGIC {
            return Err(());
        }
        let block_size = 1024 << sb.s_log_block_size;
        let inode_size = if sb.s_rev_level > 0 {
            sb.s_inode_size
        } else {
            128
        };
        Ok(Arc::new(Mutex::new(Ext2FileSystem {
            device,
            block_size,
            inodes_per_group: sb.s_inodes_per_group,
            inode_size,
            blocks_per_group: sb.s_blocks_per_group,
            _total_blocks: sb.s_blocks_count,
            _total_inodes: sb.s_inodes_count,
        })))
    }

    fn gd_block(&self) -> u64 {
        if self.block_size == 1024 { 2 } else { 1 }
    }

    fn gd_sector(&self) -> u64 {
        self.gd_block() * self.block_size as u64 / 512
    }

    pub(crate) fn inode_group(&self, inum: u32) -> (u32, u32) {
        (
            (inum - 1) / self.inodes_per_group,
            (inum - 1) % self.inodes_per_group,
        )
    }

    pub(crate) fn block_group(&self, block: u32) -> (u32, u32) {
        (
            block / self.blocks_per_group,
            block % self.blocks_per_group,
        )
    }

    // ── inode I/O ────────────────────────────────────────────────────

    pub(crate) fn read_inode(&self, inode_num: u32) -> Result<Inode, ()> {
        let (group, index) = self.inode_group(inode_num);
        let gd_raw = self.read_gd_raw()?;
        let gd_ptr = gd_raw.as_ptr();
        let gd = unsafe {
            &*(gd_ptr.add(group as usize * 32) as *const types::GroupDescriptor)
        };
        let table_block = gd.bg_inode_table;
        let offset = index as u64 * self.inode_size as u64;
        let sec =
            (table_block as u64 * self.block_size as u64 + offset) / 512;
        let sec_off =
            (table_block as u64 * self.block_size as u64 + offset) % 512;
        let mut sector_buf = [0u8; 512];
        self.device
            .lock()
            .read_sector(sec, &mut sector_buf)
            .map_err(|_| ())?;
        Ok(unsafe {
            core::ptr::read_unaligned(
                sector_buf.as_ptr().add(sec_off as usize) as *const Inode,
            )
        })
    }

    pub(crate) fn write_inode(
        &self,
        inode_num: u32,
        inode: &Inode,
    ) -> Result<(), ()> {
        let (group, index) = self.inode_group(inode_num);
        let gd_raw = self.read_gd_raw()?;
        let gd_ptr = gd_raw.as_ptr();
        let gd = unsafe {
            &*(gd_ptr.add(group as usize * 32) as *const types::GroupDescriptor)
        };
        let table_block = gd.bg_inode_table;
        let offset = index as u64 * self.inode_size as u64;
        let sec =
            (table_block as u64 * self.block_size as u64 + offset) / 512;
        let sec_off =
            (table_block as u64 * self.block_size as u64 + offset) % 512;
        let mut sector_buf = [0u8; 512];
        self.device
            .lock()
            .read_sector(sec, &mut sector_buf)
            .map_err(|_| ())?;
        unsafe {
            core::ptr::write_unaligned(
                sector_buf.as_mut_ptr().add(sec_off as usize) as *mut Inode,
                *inode,
            );
        }
        self.device
            .lock()
            .write_sector(sec, &sector_buf)
            .map_err(|_| ())
    }

    // ── block I/O ────────────────────────────────────────────────────

    pub(crate) fn read_block(&self, block_num: u32) -> Result<Vec<u8>, ()> {
        let mut buf = vec![0u8; self.block_size];
        let sector = block_num as u64 * self.block_size as u64 / 512;
        self.device
            .lock()
            .read_sector(sector, &mut buf)
            .map_err(|_| ())?;
        Ok(buf)
    }

    pub(crate) fn write_block(
        &self,
        block_num: u32,
        data: &[u8],
    ) -> Result<(), ()> {
        let sector = block_num as u64 * self.block_size as u64 / 512;
        let spb = self.block_size / 512;
        for i in 0..spb {
            let off = i * 512;
            let mut buf = [0u8; 512];
            let clen = core::cmp::min(512, data.len().saturating_sub(off));
            if clen > 0 {
                buf[..clen].copy_from_slice(&data[off..off + clen]);
            }
            self.device
                .lock()
                .write_sector(sector + i as u64, &buf)
                .map_err(|_| ())?;
        }
        Ok(())
    }

    pub(crate) fn now(&self) -> u32 {
        crate::interrupts::get_ticks() as u32
    }

    // ── indirect block mapping ───────────────────────────────────────

    pub(crate) fn read_all_block_indices(
        &self,
        inode: &Inode,
    ) -> Result<Vec<u32>, ()> {
        let mut blocks = Vec::new();
        for i in 0..12 {
            blocks.push(inode.i_block[i]);
        }
        let entries = self.block_size / 4;
        if inode.i_block[12] != 0 {
            blocks.append(&mut self.read_indirect(inode.i_block[12], 1)?);
        } else {
            blocks.extend(core::iter::repeat(0).take(entries));
        }
        if inode.i_block[13] != 0 {
            blocks.append(&mut self.read_indirect(inode.i_block[13], 2)?);
        } else {
            blocks.extend(core::iter::repeat(0).take(entries * entries));
        }
        if inode.i_block[14] != 0 {
            blocks.append(&mut self.read_indirect(inode.i_block[14], 3)?);
        } else {
            // Empty triple-indirect level is a pure hole — no reader
            // reaches it (reads stop at inode size). Cap at level-2 size
            // to avoid a 67 MB Vec per inode.
            blocks.extend(core::iter::repeat(0).take(entries * entries));
        }
        Ok(blocks)
    }

    fn read_indirect(
        &self,
        block_num: u32,
        level: u32,
    ) -> Result<Vec<u32>, ()> {
        let entries = self.block_size / 4;
        let buf = self.read_block(block_num)?;
        let ptrs = unsafe {
            core::slice::from_raw_parts(buf.as_ptr() as *const u32, entries)
        };
        let mut out = Vec::new();
        if level == 1 {
            for &p in ptrs {
                out.push(p);
            }
        } else {
            for &p in ptrs {
                if p == 0 {
                    let sub_entries = entries.pow(level - 1);
                    out.extend(core::iter::repeat(0).take(sub_entries));
                } else {
                    out.append(&mut self.read_indirect(p, level - 1)?);
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn set_block_ptr(
        fs: &Ext2FileSystem,
        start_block: &mut u32,
        level: u32,
        idx: usize,
        epb: usize,
        target: u32,
    ) -> Result<(), ()> {
        if *start_block == 0 {
            *start_block = fs.allocate_block()?;
        }
        if level == 1 {
            let mut buf = vec![0u8; fs.block_size];
            let sec = *start_block as u64 * fs.block_size as u64 / 512;
            let _ = fs.device.lock().read_sector(sec, &mut buf);
            unsafe {
                *(buf.as_mut_ptr() as *mut u32).add(idx) = target;
            }
            fs.device
                .lock()
                .write_sector(sec, &buf)
                .map_err(|_| ())
        } else {
            let buf = fs.read_block(*start_block)?;
            let span = epb.pow(level - 1);
            let sub_idx = idx / span;
            let mut sub =
                unsafe { *(buf.as_ptr() as *const u32).add(sub_idx) };
            Self::set_block_ptr(
                fs,
                &mut sub,
                level - 1,
                idx % span,
                epb,
                target,
            )?;
            let mut buf2 = fs.read_block(*start_block)?;
            unsafe {
                *(buf2.as_mut_ptr() as *mut u32).add(sub_idx) = sub;
            }
            fs.device
                .lock()
                .write_sector(
                    *start_block as u64 * fs.block_size as u64 / 512,
                    &buf2,
                )
                .map_err(|_| ())
        }
    }

    pub(crate) fn write_file_blocks(
        &self,
        inode: &mut Inode,
        data: &[u8],
    ) -> Result<(), ()> {
        let bs = self.block_size;
        let needed = if data.is_empty() {
            0
        } else {
            (data.len() + bs - 1) / bs
        };
        let epb = bs / 4;

        let old_blocks = self.read_all_block_indices(inode)?;
        for i in 0..needed {
            let off = i * bs;
            let len = core::cmp::min(bs, data.len() - off);
            let mut block_data = vec![0u8; bs];
            if len > 0 {
                block_data[..len].copy_from_slice(&data[off..off + len]);
            }
            let bnum = if i < old_blocks.len() && old_blocks[i] != 0 {
                old_blocks[i]
            } else if i < 12 {
                let nb = self.allocate_block()?;
                inode.i_block[i] = nb;
                nb
            } else {
                let ndb = self.allocate_block()?;
                let idx = i - 12;
                if idx < epb {
                    let mut blk = inode.i_block[12];
                    Self::set_block_ptr(self, &mut blk, 1, idx, epb, ndb)?;
                    inode.i_block[12] = blk;
                } else if idx < epb + epb * epb {
                    let mut blk = inode.i_block[13];
                    Self::set_block_ptr(
                        self,
                        &mut blk,
                        2,
                        idx - epb,
                        epb,
                        ndb,
                    )?;
                    inode.i_block[13] = blk;
                } else {
                    let mut blk = inode.i_block[14];
                    Self::set_block_ptr(
                        self,
                        &mut blk,
                        3,
                        idx - epb - epb * epb,
                        epb,
                        ndb,
                    )?;
                    inode.i_block[14] = blk;
                }
                ndb
            };
            self.write_block(bnum, &block_data)?;
        }

        // Free excess blocks if data shrank.
        if needed < old_blocks.len() {
            for &b in &old_blocks[needed..] {
                if b != 0 {
                    self.free_block(b)?;
                }
            }
            for i in needed..12 {
                if i < old_blocks.len() {
                    inode.i_block[i] = 0;
                }
            }
        }

        inode.i_size_lo = data.len() as u32;
        inode.i_blocks_lo = (if needed == 0 {
            0
        } else {
            needed * bs / 512
        }) as u32;
        Ok(())
    }

    pub(crate) fn free_all_blocks(
        &self,
        inode: &Inode,
    ) -> Result<(), ()> {
        for &b in &self.read_all_block_indices(inode)? {
            if b != 0 {
                self.free_block(b)?;
            }
        }
        if inode.i_block[12] != 0 {
            self.free_indirect(inode.i_block[12], 1)?;
        }
        if inode.i_block[13] != 0 {
            self.free_indirect(inode.i_block[13], 2)?;
        }
        if inode.i_block[14] != 0 {
            self.free_indirect(inode.i_block[14], 3)?;
        }
        Ok(())
    }

    pub(crate) fn free_indirect(
        &self,
        block_num: u32,
        level: u32,
    ) -> Result<(), ()> {
        let epb = self.block_size / 4;
        let buf = self.read_block(block_num)?;
        let ptrs = unsafe {
            core::slice::from_raw_parts(buf.as_ptr() as *const u32, epb)
        };
        if level > 1 {
            for &p in ptrs {
                if p != 0 {
                    self.free_indirect(p, level - 1)?;
                }
            }
        }
        self.free_block(block_num)
    }

    // ── group descriptor I/O ──────────────────────────────────────

    pub(crate) fn read_gd_raw(&self) -> Result<Vec<u8>, ()> {
        let mut buf = vec![0u8; self.block_size];
        self.device
            .lock()
            .read_sector(self.gd_sector(), &mut buf)
            .map_err(|_| ())?;
        Ok(buf)
    }
}

// ── Public mount entry point ─────────────────────────────────────────

pub fn mount(
    device: Arc<Mutex<dyn BlockDevice>>,
) -> Result<Arc<Ext2FileSystemHandle>, ()> {
    let fs = Ext2FileSystem::new(device)?;
    {
        let l = fs.lock();
        if let Ok(mut sb) = l.read_raw_sb() {
            sb.s_state = 0;
            sb.s_mnt_count = sb.s_mnt_count.wrapping_add(1);
            let _ = l.write_raw_sb(&sb);
        }
    }
    Ok(Arc::new(Ext2FileSystemHandle { fs }))
}

/// Thin handle wrapping the inner `Ext2FileSystem` for the VFS layer.
pub struct Ext2FileSystemHandle {
    fs: Arc<Mutex<Ext2FileSystem>>,
}

impl FileSystem for Ext2FileSystemHandle {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        let inode = self.fs.lock().read_inode(2)?;
        Ok(Arc::new(Ext2Node {
            fs: self.fs.clone(),
            name: String::from(""),
            inode_num: 2,
            inode,
        }))
    }
}

// ── VFS node ─────────────────────────────────────────────────────────

pub struct Ext2Node {
    fs: Arc<Mutex<Ext2FileSystem>>,
    name: String,
    inode_num: u32,
    inode: Inode,
}

impl Ext2Node {
    pub(crate) fn inode_type(inode: &Inode) -> u16 {
        inode.i_mode & S_IFMT
    }
}

mod vfs_impl;

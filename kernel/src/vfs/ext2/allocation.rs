//! Block and inode allocation for EXT2.
//!
//! Handles bitmap scanning, block/inode allocation and freeing,
//! and superblock free-count bookkeeping.

use alloc::vec;

use super::Ext2FileSystem;
use super::types::{Superblock, GroupDescriptor};

impl Ext2FileSystem {
    // ── helpers ──────────────────────────────────────────────────────

    fn read_sb_buf(&self) -> Result<[u8; 1024], ()> {
        let mut buf = [0u8; 1024];
        self.device.lock().read_sector(2, &mut buf).map_err(|_| ())?;
        Ok(buf)
    }

    pub(crate) fn read_raw_sb(&self) -> Result<Superblock, ()> {
        let buf = self.read_sb_buf()?;
        Ok(unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Superblock) })
    }

    pub(crate) fn write_raw_sb(&self, sb: &Superblock) -> Result<(), ()> {
        let mut buf = [0u8; 1024];
        unsafe { core::ptr::write_unaligned(buf.as_mut_ptr() as *mut Superblock, *sb); }
        self.device.lock().write_sector(2, &buf).map_err(|_| ())
    }

    fn write_gd_raw(&self, raw: &[u8]) -> Result<(), ()> {
        self.device.lock().write_sector(self.gd_sector(), raw).map_err(|_| ())
    }

    fn gd_ptr<'a>(raw: &'a mut [u8], group: u32) -> &'a mut GroupDescriptor {
        let off = group as usize * 32;
        unsafe { &mut *(raw.as_mut_ptr().add(off) as *mut GroupDescriptor) }
    }

    fn set_bitmap(&self, bitmap_block: u32, bit: u32, set: bool) -> Result<(), ()> {
        let byte = (bit / 8) as usize;
        let b = (bit % 8) as u8;
        let sector = bitmap_block as u64 * self.block_size as u64 / 512;
        let mut buf = vec![0u8; self.block_size];
        self.device.lock().read_sector(sector, &mut buf).map_err(|_| ())?;
        if set {
            buf[byte] |= 1 << b;
        } else {
            buf[byte] &= !(1 << b);
        }
        self.device.lock().write_sector(sector, &buf).map_err(|_| ())
    }

    // ── block allocation ─────────────────────────────────────────────

    pub(crate) fn allocate_block(&self) -> Result<u32, ()> {
        let sb = self.read_raw_sb()?;
        let group_count =
            (sb.s_blocks_count + self.blocks_per_group - 1) / self.blocks_per_group;

        for g in 0..group_count {
            let mut gd_raw = self.read_gd_raw()?;
            let gd = Self::gd_ptr(&mut gd_raw, g);
            if gd.bg_free_blocks_count == 0 {
                continue;
            }
            let bitmap_sector =
                gd.bg_block_bitmap as u64 * self.block_size as u64 / 512;
            let mut bitmap = vec![0u8; self.block_size];
            self.device
                .lock()
                .read_sector(bitmap_sector, &mut bitmap)
                .map_err(|_| ())?;
            for byte_idx in 0..self.block_size {
                if bitmap[byte_idx] == 0xFF {
                    continue;
                }
                for bit in 0..8u32 {
                    if (bitmap[byte_idx] & (1 << bit)) == 0 {
                        bitmap[byte_idx] |= 1 << bit;
                        self.device
                            .lock()
                            .write_sector(bitmap_sector, &bitmap)
                            .map_err(|_| ())?;
                        gd.bg_free_blocks_count -= 1;
                        self.write_gd_raw(&gd_raw)?;
                        let mut sb2 = self.read_raw_sb()?;
                        sb2.s_free_blocks_count -= 1;
                        self.write_raw_sb(&sb2)?;
                        return Ok(g * self.blocks_per_group + (byte_idx as u32 * 8 + bit));
                    }
                }
            }
        }
        Err(())
    }

    pub(crate) fn free_block(&self, block_num: u32) -> Result<(), ()> {
        let (group, idx) = self.block_group(block_num);
        let mut gd_raw = self.read_gd_raw()?;
        let gd = Self::gd_ptr(&mut gd_raw, group);
        self.set_bitmap(gd.bg_block_bitmap, idx, false)?;
        gd.bg_free_blocks_count += 1;
        self.write_gd_raw(&gd_raw)?;
        let mut sb = self.read_raw_sb()?;
        sb.s_free_blocks_count += 1;
        self.write_raw_sb(&sb)
    }

    // ── inode allocation ─────────────────────────────────────────────

    pub(crate) fn allocate_inode(&self) -> Result<u32, ()> {
        let sb = self.read_raw_sb()?;
        let group_count =
            (sb.s_inodes_count + self.inodes_per_group - 1) / self.inodes_per_group;

        for g in 0..group_count {
            let mut gd_raw = self.read_gd_raw()?;
            let gd = Self::gd_ptr(&mut gd_raw, g);
            if gd.bg_free_inodes_count == 0 {
                continue;
            }
            let bitmap_sector =
                gd.bg_inode_bitmap as u64 * self.block_size as u64 / 512;
            let mut bitmap = vec![0u8; self.block_size];
            self.device
                .lock()
                .read_sector(bitmap_sector, &mut bitmap)
                .map_err(|_| ())?;
            for byte_idx in 0..self.block_size {
                if bitmap[byte_idx] == 0xFF {
                    continue;
                }
                for bit in 0..8u32 {
                    if (bitmap[byte_idx] & (1 << bit)) == 0 {
                        bitmap[byte_idx] |= 1 << bit;
                        self.device
                            .lock()
                            .write_sector(bitmap_sector, &bitmap)
                            .map_err(|_| ())?;
                        gd.bg_free_inodes_count -= 1;
                        self.write_gd_raw(&gd_raw)?;
                        let mut sb2 = self.read_raw_sb()?;
                        sb2.s_free_inodes_count -= 1;
                        self.write_raw_sb(&sb2)?;
                        return Ok(
                            g * self.inodes_per_group
                                + (byte_idx as u32 * 8 + bit + 1),
                        );
                    }
                }
            }
        }
        Err(())
    }

    pub(crate) fn free_inode(&self, inode_num: u32) -> Result<(), ()> {
        let (group, idx) = self.inode_group(inode_num);
        let mut gd_raw = self.read_gd_raw()?;
        let gd = Self::gd_ptr(&mut gd_raw, group);
        self.set_bitmap(gd.bg_inode_bitmap, idx, false)?;
        gd.bg_free_inodes_count += 1;
        self.write_gd_raw(&gd_raw)?;
        let mut sb = self.read_raw_sb()?;
        sb.s_free_inodes_count += 1;
        self.write_raw_sb(&sb)
    }
}

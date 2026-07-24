use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;
use crate::drivers::block::BlockDevice;
use crate::vfs::{FileSystem, VfsNode, Stat, StatFs};

const EXT2_SUPER_MAGIC: u16 = 0xEF53;
#[allow(dead_code)]
const EXT2_ERROR_FS: u16 = 2;
const S_IFMT: u16 = 0xF000;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Superblock {
    s_inodes_count: u32,
    s_blocks_count: u32,
    s_r_blocks_count: u32,
    s_free_blocks_count: u32,
    s_free_inodes_count: u32,
    s_first_data_block: u32,
    s_log_block_size: u32,
    s_log_frag_size: u32,
    s_blocks_per_group: u32,
    s_frags_per_group: u32,
    s_inodes_per_group: u32,
    s_mtime: u32,
    s_wtime: u32,
    s_mnt_count: u16,
    s_max_mnt_count: u16,
    s_magic: u16,
    s_state: u16,
    s_errors: u16,
    s_minor_rev_level: u16,
    s_lastcheck: u32,
    s_checkinterval: u32,
    s_creator_os: u32,
    s_rev_level: u32,
    s_def_resuid: u16,
    s_def_resgid: u16,
    s_first_ino: u32,
    s_inode_size: u16,
    s_block_group_nr: u16,
    s_feature_compat: u32,
    s_feature_incompat: u32,
    s_feature_ro_compat: u32,
    s_uuid: [u8; 16],
    s_volume_name: [u8; 16],
    s_last_mounted: [u8; 64],
    s_algo_bitmap: u32,
    s_prealloc_blocks: u8,
    s_prealloc_dir_blocks: u8,
    _padding1: u16,
    s_journal_uuid: [u8; 16],
    s_journal_inum: u32,
    s_journal_dev: u32,
    s_last_orphan: u32,
    s_hash_seed: [u32; 4],
    s_def_hash_version: u8,
    _padding2: [u8; 3],
    s_default_mount_opts: u32,
    s_first_meta_bg: u32,
    _unused: [u8; 760],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct GroupDescriptor {
    bg_block_bitmap: u32,
    bg_inode_bitmap: u32,
    bg_inode_table: u32,
    bg_free_blocks_count: u16,
    bg_free_inodes_count: u16,
    bg_used_dirs_count: u16,
    bg_pad: u16,
    bg_reserved: [u8; 12],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Inode {
    i_mode: u16,
    i_uid: u16,
    i_size_lo: u32,
    i_atime: u32,
    i_ctime: u32,
    i_mtime: u32,
    i_dtime: u32,
    i_gid: u16,
    i_links_count: u16,
    i_blocks_lo: u32,
    i_flags: u32,
    i_osd1: u32,
    i_block: [u32; 15],
    i_generation: u32,
    i_file_acl: u32,
    i_dir_acl: u32,
    i_faddr: u32,
    i_osd2: [u8; 12],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DirectoryEntry {
    inode: u32,
    rec_len: u16,
    name_len: u8,
    file_type: u8,
}

pub struct Ext2FileSystem {
    device: Arc<Mutex<dyn BlockDevice>>,
    block_size: usize,
    inodes_per_group: u32,
    inode_size: u16,
    blocks_per_group: u32,
    _total_blocks: u32,
    _total_inodes: u32,
}

impl Ext2FileSystem {
    pub fn new(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<Mutex<Self>>, ()> {
        let mut buf = [0u8; 1024];
        device.lock().read_sector(2, &mut buf).map_err(|_| ())?;
        let sb = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Superblock) };
        if sb.s_magic != EXT2_SUPER_MAGIC {
            return Err(());
        }
        let block_size = 1024 << sb.s_log_block_size;
        let inode_size = if sb.s_rev_level > 0 { sb.s_inode_size } else { 128 };
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

    fn gd_block(&self) -> u64 { if self.block_size == 1024 { 2 } else { 1 } }
    fn gd_sector(&self) -> u64 { self.gd_block() * self.block_size as u64 / 512 }

    fn read_sb_buf(&self) -> Result<[u8; 1024], ()> {
        let mut buf = [0u8; 1024];
        self.device.lock().read_sector(2, &mut buf).map_err(|_| ())?;
        Ok(buf)
    }

    fn read_raw_sb(&self) -> Result<Superblock, ()> {
        let buf = self.read_sb_buf()?;
        Ok(unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Superblock) })
    }

    fn write_raw_sb(&self, sb: &Superblock) -> Result<(), ()> {
        let mut buf = [0u8; 1024];
        unsafe { core::ptr::write_unaligned(buf.as_mut_ptr() as *mut Superblock, *sb); }
        self.device.lock().write_sector(2, &buf).map_err(|_| ())
    }

    fn read_gd_raw(&self) -> Result<Vec<u8>, ()> {
        let mut buf = vec![0u8; self.block_size];
        self.device.lock().read_sector(self.gd_sector(), &mut buf).map_err(|_| ())?;
        Ok(buf)
    }

    fn write_gd_raw(&self, raw: &[u8]) -> Result<(), ()> {
        self.device.lock().write_sector(self.gd_sector(), raw).map_err(|_| ())
    }

    fn gd_ptr<'a>(raw: &'a mut [u8], group: u32) -> &'a mut GroupDescriptor {
        let off = group as usize * 32;
        unsafe { &mut *(raw.as_mut_ptr().add(off) as *mut GroupDescriptor) }
    }

    fn inode_group(&self, inum: u32) -> (u32, u32) {
        ((inum - 1) / self.inodes_per_group, (inum - 1) % self.inodes_per_group)
    }

    fn block_group(&self, block: u32) -> (u32, u32) {
        // block belongs to group based on blocks_per_group from block 0 (or first_data_block)
        (block / self.blocks_per_group, block % self.blocks_per_group)
    }

    fn read_inode(&self, inode_num: u32) -> Result<Inode, ()> {
        let (group, index) = self.inode_group(inode_num);
        let gd_raw = self.read_gd_raw()?;
        let gd_ptr = gd_raw.as_ptr();
        let gd = unsafe { &*(gd_ptr.add(group as usize * 32) as *const GroupDescriptor) };
        let table_block = gd.bg_inode_table;
        let offset = index as u64 * self.inode_size as u64;
        let sec = (table_block as u64 * self.block_size as u64 + offset) / 512;
        let sec_off = (table_block as u64 * self.block_size as u64 + offset) % 512;
        let mut sector_buf = [0u8; 512];
        self.device.lock().read_sector(sec, &mut sector_buf).map_err(|_| ())?;
        Ok(unsafe { core::ptr::read_unaligned(sector_buf.as_ptr().add(sec_off as usize) as *const Inode) })
    }

    fn write_inode(&self, inode_num: u32, inode: &Inode) -> Result<(), ()> {
        let (group, index) = self.inode_group(inode_num);
        let gd_raw = self.read_gd_raw()?;
        let gd_ptr = gd_raw.as_ptr();
        let gd = unsafe { &*(gd_ptr.add(group as usize * 32) as *const GroupDescriptor) };
        let table_block = gd.bg_inode_table;
        let offset = index as u64 * self.inode_size as u64;
        let sec = (table_block as u64 * self.block_size as u64 + offset) / 512;
        let sec_off = (table_block as u64 * self.block_size as u64 + offset) % 512;
        let mut sector_buf = [0u8; 512];
        self.device.lock().read_sector(sec, &mut sector_buf).map_err(|_| ())?;
        unsafe { core::ptr::write_unaligned(sector_buf.as_mut_ptr().add(sec_off as usize) as *mut Inode, *inode); }
        self.device.lock().write_sector(sec, &sector_buf).map_err(|_| ())
    }

    fn read_block(&self, block_num: u32) -> Result<Vec<u8>, ()> {
        let mut buf = vec![0u8; self.block_size];
        let sector = block_num as u64 * self.block_size as u64 / 512;
        self.device.lock().read_sector(sector, &mut buf).map_err(|_| ())?;
        Ok(buf)
    }

    fn write_block(&self, block_num: u32, data: &[u8]) -> Result<(), ()> {
        let sector = block_num as u64 * self.block_size as u64 / 512;
        let spb = self.block_size / 512;
        for i in 0..spb {
            let off = i * 512;
            let mut buf = [0u8; 512];
            let clen = core::cmp::min(512, data.len().saturating_sub(off));
            if clen > 0 { buf[..clen].copy_from_slice(&data[off..off + clen]); }
            self.device.lock().write_sector(sector + i as u64, &buf).map_err(|_| ())?;
        }
        Ok(())
    }

    fn set_bitmap(&self, bitmap_block: u32, bit: u32, set: bool) -> Result<(), ()> {
        let byte = (bit / 8) as usize;
        let b = (bit % 8) as u8;
        let sector = bitmap_block as u64 * self.block_size as u64 / 512;
        let mut buf = vec![0u8; self.block_size];
        self.device.lock().read_sector(sector, &mut buf).map_err(|_| ())?;
        if set { buf[byte] |= 1 << b; } else { buf[byte] &= !(1 << b); }
        self.device.lock().write_sector(sector, &buf).map_err(|_| ())
    }

    fn allocate_block(&self) -> Result<u32, ()> {
        let sb = self.read_raw_sb()?;
        let group_count = (sb.s_blocks_count + self.blocks_per_group - 1) / self.blocks_per_group;
        for g in 0..group_count {
            let mut gd_raw = self.read_gd_raw()?;
            let gd = Self::gd_ptr(&mut gd_raw, g);
            if gd.bg_free_blocks_count == 0 { continue; }
            let bitmap_sector = gd.bg_block_bitmap as u64 * self.block_size as u64 / 512;
            let mut bitmap = vec![0u8; self.block_size];
            self.device.lock().read_sector(bitmap_sector, &mut bitmap).map_err(|_| ())?;
            for byte_idx in 0..self.block_size {
                if bitmap[byte_idx] == 0xFF { continue; }
                for bit in 0..8 {
                    if (bitmap[byte_idx] & (1 << bit)) == 0 {
                        bitmap[byte_idx] |= 1 << bit;
                        self.device.lock().write_sector(bitmap_sector, &bitmap).map_err(|_| ())?;
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

    fn free_block(&self, block_num: u32) -> Result<(), ()> {
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

    fn allocate_inode(&self) -> Result<u32, ()> {
        let sb = self.read_raw_sb()?;
        let group_count = (sb.s_inodes_count + self.inodes_per_group - 1) / self.inodes_per_group;
        for g in 0..group_count {
            let mut gd_raw = self.read_gd_raw()?;
            let gd = Self::gd_ptr(&mut gd_raw, g);
            if gd.bg_free_inodes_count == 0 { continue; }
            let bitmap_sector = gd.bg_inode_bitmap as u64 * self.block_size as u64 / 512;
            let mut bitmap = vec![0u8; self.block_size];
            self.device.lock().read_sector(bitmap_sector, &mut bitmap).map_err(|_| ())?;
            for byte_idx in 0..self.block_size {
                if bitmap[byte_idx] == 0xFF { continue; }
                for bit in 0..8 {
                    if (bitmap[byte_idx] & (1 << bit)) == 0 {
                        bitmap[byte_idx] |= 1 << bit;
                        self.device.lock().write_sector(bitmap_sector, &bitmap).map_err(|_| ())?;
                        gd.bg_free_inodes_count -= 1;
                        self.write_gd_raw(&gd_raw)?;
                        let mut sb2 = self.read_raw_sb()?;
                        sb2.s_free_inodes_count -= 1;
                        self.write_raw_sb(&sb2)?;
                        return Ok(g * self.inodes_per_group + (byte_idx as u32 * 8 + bit + 1));
                    }
                }
            }
        }
        Err(())
    }

    fn free_inode(&self, inode_num: u32) -> Result<(), ()> {
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

    fn now(&self) -> u32 {
        crate::interrupts::get_ticks() as u32
    }

    fn read_all_block_indices(&self, inode: &Inode) -> Result<Vec<u32>, ()> {
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
            blocks.extend(core::iter::repeat(0).take(entries * entries * entries));
        }
        Ok(blocks)
    }

    fn read_indirect(&self, block_num: u32, level: u32) -> Result<Vec<u32>, ()> {
        let entries = self.block_size / 4;
        let buf = self.read_block(block_num)?;
        let ptrs = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u32, entries) };
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

    fn set_block_ptr(fs: &Ext2FileSystem, start_block: &mut u32, level: u32, idx: usize, epb: usize, target: u32) -> Result<(), ()> {
        if *start_block == 0 { *start_block = fs.allocate_block()?; }
        if level == 1 {
            let mut buf = vec![0u8; fs.block_size];
            let sec = *start_block as u64 * fs.block_size as u64 / 512;
            let _ = fs.device.lock().read_sector(sec, &mut buf);
            unsafe { *(buf.as_mut_ptr() as *mut u32).add(idx) = target; }
            fs.device.lock().write_sector(sec, &buf).map_err(|_| ())
        } else {
            let buf = fs.read_block(*start_block)?;
            let sub_idx = idx % epb;
            let mut sub = unsafe { *(buf.as_ptr() as *const u32).add(sub_idx) };
            Self::set_block_ptr(fs, &mut sub, level - 1, idx / epb, epb, target)?;
            let mut buf2 = fs.read_block(*start_block)?;
            unsafe { *(buf2.as_mut_ptr() as *mut u32).add(sub_idx) = sub; }
            fs.device.lock().write_sector(*start_block as u64 * fs.block_size as u64 / 512, &buf2).map_err(|_| ())
        }
    }

    fn write_file_blocks(&self, inode: &mut Inode, data: &[u8]) -> Result<(), ()> {
        let bs = self.block_size;
        let needed = if data.is_empty() { 0 } else { (data.len() + bs - 1) / bs };
        let epb = bs / 4;
        // Reuse existing blocks up to min(old_count, needed), then allocate new ones
        let old_blocks = self.read_all_block_indices(inode)?;
        for i in 0..needed {
            let off = i * bs;
            let len = core::cmp::min(bs, data.len() - off);
            let mut block_data = vec![0u8; bs];
            if len > 0 { block_data[..len].copy_from_slice(&data[off..off + len]); }
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
                } else if idx < epb * epb {
                    let mut blk = inode.i_block[13];
                    Self::set_block_ptr(self, &mut blk, 2, idx, epb, ndb)?;
                    inode.i_block[13] = blk;
                } else {
                    let mut blk = inode.i_block[14];
                    Self::set_block_ptr(self, &mut blk, 3, idx, epb, ndb)?;
                    inode.i_block[14] = blk;
                }
                ndb
            };
            self.write_block(bnum, &block_data)?;
        }
        // Free excess blocks if data shrank
        if needed < old_blocks.len() {
            for &b in &old_blocks[needed..] {
                if b != 0 { self.free_block(b)?; }
            }
            // Zero out now-unused inode block pointers
            for i in needed..12 {
                if i < old_blocks.len() { inode.i_block[i] = 0; }
            }
            // ponytail: indirect block pointer cleanup for large files — add when needed
        }
        inode.i_size_lo = data.len() as u32;
        inode.i_blocks_lo = (if needed == 0 { 0 } else { needed * bs / 512 }) as u32;
        Ok(())
    }

    fn free_all_blocks(&self, inode: &Inode) -> Result<(), ()> {
        for &b in &self.read_all_block_indices(inode)? { if b != 0 { self.free_block(b)?; } }
        if inode.i_block[12] != 0 { self.free_indirect(inode.i_block[12], 1)?; }
        if inode.i_block[13] != 0 { self.free_indirect(inode.i_block[13], 2)?; }
        if inode.i_block[14] != 0 { self.free_indirect(inode.i_block[14], 3)?; }
        Ok(())
    }

    fn free_indirect(&self, block_num: u32, level: u32) -> Result<(), ()> {
        let epb = self.block_size / 4;
        let buf = self.read_block(block_num)?;
        let ptrs = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u32, epb) };
        if level > 1 {
            for &p in ptrs { if p != 0 { self.free_indirect(p, level - 1)?; } }
        }
        self.free_block(block_num)
    }

    fn find_dentry(&self, dir_inode: &Inode, name: &str) -> Result<Option<(u32, u32, u32)>, ()> {
        let blocks = self.read_all_block_indices(dir_inode)?;
        for &bnum in &blocks {
            if bnum == 0 { continue; }
            let data = self.read_block(bnum)?;
            let mut off = 0usize;
            while off < self.block_size {
                let e = unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 { break; }
                let enm = core::str::from_utf8(
                    unsafe { core::slice::from_raw_parts(data.as_ptr().add(off + 8), e.name_len as usize) }
                ).map_err(|_| ())?;
                if enm == name {
                    return Ok(Some((e.inode, bnum, off as u32)));
                }
                if e.rec_len == 0 { break; }
                off += e.rec_len as usize;
            }
        }
        Ok(None)
    }

    fn remove_dentry(&self, dir_inode_num: u32, name: &str) -> Result<(), ()> {
        let dir_inode = self.read_inode(dir_inode_num)?;
        let blocks = self.read_all_block_indices(&dir_inode)?;
        for &bnum in &blocks {
            if bnum == 0 { continue; }
            let mut data = self.read_block(bnum)?;
            let mut off = 0usize;
            let mut prev = 0usize;
            while off < self.block_size {
                let e = unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 { break; }
                let enm = core::str::from_utf8(
                    unsafe { core::slice::from_raw_parts(data.as_ptr().add(off + 8), e.name_len as usize) }
                ).map_err(|_| ())?;
                if enm == name {
                    if off == prev {
                        unsafe { *(data.as_mut_ptr().add(off) as *mut u32) = 0; }
                    } else {
                        let pe = unsafe { &mut *(data.as_mut_ptr().add(prev) as *mut DirectoryEntry) };
                        pe.rec_len = pe.rec_len.wrapping_add(e.rec_len);
                    }
                    self.write_block(bnum, &data)?;
                    // Shrink directory: free trailing blocks that are now empty
                    self.shrink_dir_blocks(dir_inode_num, &blocks)?;
                    return Ok(());
                }
                if e.rec_len == 0 { break; }
                prev = off;
                off += e.rec_len as usize;
            }
        }
        Err(())
    }

    fn shrink_dir_blocks(&self, dir_inode_num: u32, all_blocks: &[u32]) -> Result<(), ()> {
        // Free trailing blocks that contain only zeroed entries
        for &bnum in all_blocks.iter().rev() {
            if bnum == 0 { continue; }
            let data = self.read_block(bnum)?;
            let first_entry = unsafe { &*(data.as_ptr() as *const DirectoryEntry) };
            if first_entry.inode == 0 {
                // This block is unused — free it
                self.free_block(bnum)?;
                // Update directory inode to remove the block reference
                let mut dir_inode = self.read_inode(dir_inode_num)?;
                for slot in 0..12 {
                    if dir_inode.i_block[slot] == bnum {
                        dir_inode.i_block[slot] = 0;
                        break;
                    }
                }
                dir_inode.i_size_lo = dir_inode.i_size_lo.saturating_sub(self.block_size as u32);
                dir_inode.i_blocks_lo = dir_inode.i_blocks_lo.saturating_sub((self.block_size / 512) as u32);
                self.write_inode(dir_inode_num, &dir_inode)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn add_dentry(&self, parent_inode_num: u32, child_inode_num: u32, name: &str, file_type: u8) -> Result<(), ()> {
        let parent_inode = self.read_inode(parent_inode_num)?;
        let blocks = self.read_all_block_indices(&parent_inode)?;
        let new_len = (8 + name.len() + 3) & !3;

        for &bnum in &blocks {
            if bnum == 0 { continue; }
            let mut data = self.read_block(bnum)?;
            let mut off = 0;
            while off < self.block_size {
                let e = unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 && e.rec_len as usize >= new_len {
                    let e2 = unsafe { &mut *(data.as_mut_ptr().add(off) as *mut DirectoryEntry) };
                    e2.inode = child_inode_num; e2.name_len = name.len() as u8; e2.file_type = file_type;
                    unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), data.as_mut_ptr().add(off + 8), name.len()); }
                    return self.write_block(bnum, &data);
                }
                let cur_len = (8 + e.name_len as usize + 3) & !3;
                let avail = e.rec_len as usize - cur_len;
                if avail >= new_len {
                    let old_rec = e.rec_len;
                    let e2 = unsafe { &mut *(data.as_mut_ptr().add(off) as *mut DirectoryEntry) };
                    e2.rec_len = cur_len as u16;
                    let noff = off + cur_len;
                    let ne = unsafe { &mut *(data.as_mut_ptr().add(noff) as *mut DirectoryEntry) };
                    ne.inode = child_inode_num; ne.rec_len = old_rec - cur_len as u16;
                    ne.name_len = name.len() as u8; ne.file_type = file_type;
                    unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), data.as_mut_ptr().add(noff + 8), name.len()); }
                    return self.write_block(bnum, &data);
                }
                if e.rec_len == 0 { break; }
                off += e.rec_len as usize;
            }
        }

        // Need a new block — check for a free slot before allocating
        let mut upd = parent_inode;
        let slot = (0..12).find(|&i| upd.i_block[i] == 0).ok_or(())?;
        let nb = self.allocate_block()?;
        let mut new_data = vec![0u8; self.block_size];
        let e = unsafe { &mut *(new_data.as_mut_ptr() as *mut DirectoryEntry) };
        e.inode = child_inode_num; e.rec_len = self.block_size as u16;
        e.name_len = name.len() as u8; e.file_type = file_type;
        unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), new_data.as_mut_ptr().add(8), name.len()); }
        self.write_block(nb, &new_data)?;
        upd.i_block[slot] = nb;
        upd.i_size_lo += self.block_size as u32;
        upd.i_blocks_lo += (self.block_size / 512) as u32;
        self.write_inode(parent_inode_num, &upd)
    }
}

pub fn mount(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<Ext2FileSystemHandle>, ()> {
    let fs = Ext2FileSystem::new(device)?;
    {
        let l = fs.lock();
        if let Ok(mut sb) = l.read_raw_sb() {
            sb.s_state = 0; // clear valid flag — filesystem in use
            sb.s_mnt_count = sb.s_mnt_count.wrapping_add(1);
            let _ = l.write_raw_sb(&sb);
        }
    }
    Ok(Arc::new(Ext2FileSystemHandle { fs }))
}

pub struct Ext2FileSystemHandle { fs: Arc<Mutex<Ext2FileSystem>> }

impl FileSystem for Ext2FileSystemHandle {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        let inode = self.fs.lock().read_inode(2)?;
        Ok(Arc::new(Ext2Node { fs: self.fs.clone(), name: String::from(""), inode_num: 2, inode }))
    }
}

pub struct Ext2Node {
    fs: Arc<Mutex<Ext2FileSystem>>,
    name: String,
    inode_num: u32,
    inode: Inode,
}

impl Ext2Node {
    fn inode_type(inode: &Inode) -> u16 { inode.i_mode & S_IFMT }
}

impl VfsNode for Ext2Node {
    fn name(&self) -> String { self.name.clone() }
    fn is_dir(&self) -> bool { Self::inode_type(&self.inode) == 0x4000 }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir() { return Err(()); }
        let fs = self.fs.lock();
        let mut out = Vec::new();
        for &b in &fs.read_all_block_indices(&self.inode)? {
            if b == 0 { continue; }
            let data = fs.read_block(b)?;
            let mut off = 0;
            while off < fs.block_size {
                let e = unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 { break; }
                let nm = core::str::from_utf8(
                    unsafe { core::slice::from_raw_parts(data.as_ptr().add(off + 8), e.name_len as usize) }
                ).map_err(|_| ())?;
                if nm != "." && nm != ".." {
                    if let Ok(ci) = fs.read_inode(e.inode) {
                        out.push(Arc::new(Ext2Node { fs: self.fs.clone(), name: nm, inode_num: e.inode, inode: ci }) as Arc<dyn VfsNode>);
                    }
                }
                if e.rec_len == 0 { break; }
                off += e.rec_len as usize;
            }
        }
        Ok(out)
    }

    fn find_child(&self, name: &str) -> Option<Arc<dyn VfsNode>> {
        if !self.is_dir() { return None; }
        let fs = self.fs.lock();
        let blocks = fs.read_all_block_indices(&self.inode).ok()?;
        for &b in &blocks {
            if b == 0 { continue; }
            let data = fs.read_block(b).ok()?;
            let mut off = 0;
            while off < fs.block_size {
                let e = unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 { break; }
                let enm = core::str::from_utf8(
                    unsafe { core::slice::from_raw_parts(data.as_ptr().add(off + 8), e.name_len as usize) }
                ).ok()?;
                if enm == name {
                    let ci = fs.read_inode(e.inode).ok()?;
                    return Some(Arc::new(Ext2Node { fs: self.fs.clone(), name: String::from(enm), inode_num: e.inode, inode: ci }));
                }
                if e.rec_len == 0 { break; }
                off += e.rec_len as usize;
            }
        }
        None
    }

    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        if (self.inode.i_mode & S_IFMT) == 0x4000 { return Err(()); }
        let fs = self.fs.lock();
        let size = self.inode.i_size_lo as usize;
        let mut data = Vec::with_capacity(size);
        for &b in &fs.read_all_block_indices(&self.inode)? {
            let buf = if b == 0 {
                vec![0u8; fs.block_size]
            } else {
                fs.read_block(b)?
            };
            let rem = size - data.len();
            let cp = core::cmp::min(rem, fs.block_size);
            data.extend_from_slice(&buf[..cp]);
            if data.len() >= size { break; }
        }
        Ok(data)
    }

    fn stat(&self) -> Result<Stat, ()> {
        Ok(Stat {
            st_dev: 0, st_ino: self.inode_num as u64,
            st_mode: self.inode.i_mode as u32, st_nlink: self.inode.i_links_count as u32,
            st_uid: self.inode.i_uid as u32, st_gid: self.inode.i_gid as u32,
            st_rdev: 0, st_size: self.inode.i_size_lo as i64,
            st_atime: self.inode.i_atime as i64, st_mtime: self.inode.i_mtime as i64,
            st_ctime: self.inode.i_ctime as i64,
        })
    }

    fn statfs(&self) -> Result<StatFs, ()> {
        let fs = self.fs.lock();
        let sb = fs.read_raw_sb()?;
        Ok(StatFs {
            f_type: 0xEF53,
            f_bsize: fs.block_size as u64,
            f_blocks: sb.s_blocks_count as u64,
            f_bfree: sb.s_free_blocks_count as u64,
            f_bavail: sb.s_free_blocks_count as u64,
            f_files: sb.s_inodes_count as u64,
            f_ffree: sb.s_free_inodes_count as u64,
        })
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        if (self.inode.i_mode & S_IFMT) == 0x4000 { return Err(()); }
        let mut inode = self.inode;
        let fs = self.fs.lock();
        fs.write_file_blocks(&mut inode, data)?;
        inode.i_mtime = fs.now();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let num = fs.allocate_inode()?;
        let now = fs.now();
        let inode = Inode {
            i_mode: 0x81A4, i_uid: 0, i_size_lo: 0, i_atime: now, i_ctime: now, i_mtime: now,
            i_dtime: 0, i_gid: 0, i_links_count: 1, i_blocks_lo: 0, i_flags: 0, i_osd1: 0,
            i_block: [0; 15], i_generation: 0, i_file_acl: 0, i_dir_acl: 0, i_faddr: 0, i_osd2: [0; 12],
        };
        fs.write_inode(num, &inode)?;
        fs.add_dentry(self.inode_num, num, name, 1)?;
        Ok(Arc::new(Ext2Node { fs: self.fs.clone(), name: String::from(name), inode_num: num, inode }))
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let num = fs.allocate_inode()?;
        let bn = fs.allocate_block()?;
        let now = fs.now();
        let mut inode = Inode {
            i_mode: 0x41ED, i_uid: 0, i_size_lo: fs.block_size as u32, i_atime: now, i_ctime: now, i_mtime: now,
            i_dtime: 0, i_gid: 0, i_links_count: 2, i_blocks_lo: (fs.block_size / 512) as u32,
            i_flags: 0, i_osd1: 0, i_block: [0; 15], i_generation: 0, i_file_acl: 0,
            i_dir_acl: 0, i_faddr: 0, i_osd2: [0; 12],
        };
        inode.i_block[0] = bn;
        let mut block_data = vec![0u8; fs.block_size];
        {
            let dot = unsafe { &mut *(block_data.as_mut_ptr() as *mut DirectoryEntry) };
            dot.inode = num; dot.rec_len = 12; dot.name_len = 1; dot.file_type = 2;
            block_data[8] = b'.';
        }
        {
            let dotdot = unsafe { &mut *(block_data.as_mut_ptr().add(12) as *mut DirectoryEntry) };
            dotdot.inode = self.inode_num; dotdot.rec_len = (fs.block_size - 12) as u16; dotdot.name_len = 2; dotdot.file_type = 2;
            block_data[12 + 8] = b'.'; block_data[12 + 9] = b'.';
        }
        fs.write_block(bn, &block_data)?;
        fs.write_inode(num, &inode)?;
        fs.add_dentry(self.inode_num, num, name, 2)?;
        Ok(Arc::new(Ext2Node { fs: self.fs.clone(), name: String::from(name), inode_num: num, inode }))
    }

    fn unlink(&self, name: &str) -> Result<(), ()> {
        if name == "." || name == ".." { return Err(()); }
        let fs = self.fs.lock();
        let dir_inode = fs.read_inode(self.inode_num)?;
        let (child_inum, _, _) = fs.find_dentry(&dir_inode, name)?.ok_or(())?;
        let child = fs.read_inode(child_inum)?;
        if Self::inode_type(&child) == 0x4000 {
            let blocks = fs.read_all_block_indices(&child)?;
            let mut count = 0;
            for &b in &blocks {
                if b == 0 { continue; }
                let data = fs.read_block(b)?;
                let mut off = 0;
                while off < fs.block_size {
                    let e = unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                    if e.inode == 0 { break; }
                    count += 1;
                    if e.rec_len == 0 { break; }
                    off += e.rec_len as usize;
                }
            }
            if count > 2 { return Err(()); }
        }
        let mut upd = child;
        upd.i_links_count -= 1;
        if upd.i_links_count == 0 {
            upd.i_dtime = fs.now();
            fs.write_inode(child_inum, &upd)?;
            fs.free_all_blocks(&child)?;
            fs.free_inode(child_inum)?;
        } else {
            fs.write_inode(child_inum, &upd)?;
        }
        fs.remove_dentry(self.inode_num, name)
    }

    fn chmod(&self, mode: u32) -> Result<(), ()> {
        let mut inode = self.inode;
        inode.i_mode = (inode.i_mode & 0xF000) | (mode as u16 & 0x0FFF);
        let fs = self.fs.lock();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<(), ()> {
        let mut inode = self.inode;
        inode.i_uid = uid as u16;
        inode.i_gid = gid as u16;
        let fs = self.fs.lock();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }

    fn readlink(&self) -> Result<String, ()> {
        if Self::inode_type(&self.inode) != 0xA000 { return Err(()); }
        let size = self.inode.i_size_lo as usize;
        if size <= 60 {
            let ptr = core::ptr::addr_of!(self.inode.i_block) as *const u8;
            let bytes = unsafe { core::slice::from_raw_parts(ptr, size) };
            Ok(String::from_utf8_lossy(bytes).into_owned())
        } else {
            let data = self.read(usize::MAX)?;
            Ok(String::from_utf8_lossy(&data).into_owned())
        }
    }

    fn symlink(&self, name: &str, target: &str) -> Result<(), ()> {
        let fs = self.fs.lock();
        let num = fs.allocate_inode()?;
        let now = fs.now();
        let tgt = target.as_bytes();
        let mut inode = Inode {
            i_mode: 0xA1FF, i_uid: 0, i_size_lo: tgt.len() as u32, i_atime: now, i_ctime: now, i_mtime: now,
            i_dtime: 0, i_gid: 0, i_links_count: 1, i_blocks_lo: 0, i_flags: 0, i_osd1: 0,
            i_block: [0; 15], i_generation: 0, i_file_acl: 0, i_dir_acl: 0, i_faddr: 0, i_osd2: [0; 12],
        };
        if tgt.len() <= 60 {
            let ptr = core::ptr::addr_of_mut!(inode.i_block) as *mut u8;
            unsafe { core::ptr::copy_nonoverlapping(tgt.as_ptr(), ptr, tgt.len()); }
        } else {
            let bn = fs.allocate_block()?;
            inode.i_block[0] = bn;
            inode.i_blocks_lo = (fs.block_size / 512) as u32;
            fs.write_block(bn, tgt)?;
        }
        fs.write_inode(num, &inode)?;
        fs.add_dentry(self.inode_num, num, name, 7)?;
        Ok(())
    }

    fn rename(&self, old_name: &str, new_name: &str) -> Result<(), ()> {
        if old_name == "." || old_name == ".." || new_name == "." || new_name == ".." {
            return Err(());
        }
        if !self.is_dir() { return Err(()); }
        let fs = self.fs.lock();
        let dir_inode = fs.read_inode(self.inode_num)?;

        // Find the old dentry
        let (child_inum, _old_block, _old_off) = fs.find_dentry(&dir_inode, old_name)?.ok_or(())?;

        // Check if new_name already exists
        if let Some((existing_inum, _, _)) = fs.find_dentry(&dir_inode, new_name)? {
            // Remove the existing target first
            let target_inode = fs.read_inode(existing_inum)?;
            if Self::inode_type(&target_inode) == 0x4000 { return Err(()); }
            let mut upd = target_inode;
            upd.i_links_count -= 1;
            if upd.i_links_count == 0 {
                fs.free_all_blocks(&target_inode)?;
                fs.free_inode(existing_inum)?;
            } else {
                fs.write_inode(existing_inum, &upd)?;
            }
            fs.remove_dentry(self.inode_num, new_name)?;
        }

        // Add new dentry, then remove old one
        let child = fs.read_inode(child_inum)?;
        let ftype = if Self::inode_type(&child) == 0x4000 { 2u8 } else { 1u8 };
        if let Err(_) = fs.add_dentry(self.inode_num, child_inum, new_name, ftype) {
            return Err(());
        }
        fs.remove_dentry(self.inode_num, old_name)?;
        Ok(())
    }

    fn truncate(&self, len: i64) -> Result<(), ()> {
        if (self.inode.i_mode & S_IFMT) == 0x4000 { return Err(()); }
        if len < 0 { return Err(()); }
        let new_len = len as usize;
        let fs = self.fs.lock();
        let mut inode = self.inode;
        let bs = fs.block_size;
        let new_blocks_needed = if new_len == 0 { 0 } else { (new_len + bs - 1) / bs };

        let old_blocks = fs.read_all_block_indices(&inode)?;

        // Free excess blocks if shrinking
        if new_blocks_needed < old_blocks.len() {
            for &b in &old_blocks[new_blocks_needed..] {
                if b != 0 { fs.free_block(b)?; }
            }
            // Zero out freed block pointers
            for i in new_blocks_needed..12 {
                if i < old_blocks.len() { inode.i_block[i] = 0; }
            }
            // Free indirect/double/triple index blocks and zero their inode entries
            if new_blocks_needed <= 12 {
                if inode.i_block[12] != 0 { fs.free_indirect(inode.i_block[12], 1)?; inode.i_block[12] = 0; }
                if inode.i_block[13] != 0 { fs.free_indirect(inode.i_block[13], 2)?; inode.i_block[13] = 0; }
                if inode.i_block[14] != 0 { fs.free_indirect(inode.i_block[14], 3)?; inode.i_block[14] = 0; }
            }
        }

        // Zero-fill the last partial block beyond new_len
        if new_len > 0 && new_len % bs != 0 {
            let last_block_idx = new_blocks_needed - 1;
            if last_block_idx < old_blocks.len() && old_blocks[last_block_idx] != 0 {
                let mut buf = fs.read_block(old_blocks[last_block_idx])?;
                let zero_start = new_len % bs;
                for byte in buf[zero_start..].iter_mut() { *byte = 0; }
                fs.write_block(old_blocks[last_block_idx], &buf)?;
            }
        }

        // Allocate new blocks if extending
        if new_blocks_needed > old_blocks.len() {
            let epb = bs / 4;
            for i in old_blocks.len()..new_blocks_needed {
                let block_data = vec![0u8; bs];
                let bnum = if i < 12 {
                    let nb = fs.allocate_block()?;
                    inode.i_block[i] = nb;
                    nb
                } else {
                    let ndb = fs.allocate_block()?;
                    let idx = i - 12;
                    if idx < epb {
                        let mut blk = inode.i_block[12];
                        Ext2FileSystem::set_block_ptr(&fs, &mut blk, 1, idx, epb, ndb)?;
                        inode.i_block[12] = blk;
                    } else if idx < epb * epb {
                        let mut blk = inode.i_block[13];
                        Ext2FileSystem::set_block_ptr(&fs, &mut blk, 2, idx, epb, ndb)?;
                        inode.i_block[13] = blk;
                    } else {
                        let mut blk = inode.i_block[14];
                        Ext2FileSystem::set_block_ptr(&fs, &mut blk, 3, idx, epb, ndb)?;
                        inode.i_block[14] = blk;
                    }
                    ndb
                };
                fs.write_block(bnum, &block_data)?;
            }
        }

        inode.i_size_lo = new_len as u32;
        inode.i_blocks_lo = (if new_blocks_needed == 0 { 0 } else { new_blocks_needed * bs / 512 }) as u32;
        inode.i_mtime = fs.now();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }
}

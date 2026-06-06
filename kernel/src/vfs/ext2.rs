//! # Ext2 Filesystem Driver
//!
//! Basic read-only support for the Second Extended Filesystem (Ext2).

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;
use crate::drivers::block::BlockDevice;
use crate::vfs::{FileSystem, VfsNode, Stat};

// Ext2 Magic Number
const EXT2_SUPER_MAGIC: u16 = 0xEF53;

#[repr(C, packed)]
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
    // Dynamic Revision Specific
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
    // Performance Hints
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
    i_size: u32,
    i_atime: u32,
    i_ctime: u32,
    i_mtime: u32,
    i_dtime: u32,
    i_gid: u16,
    i_links_count: u16,
    i_blocks: u32,
    i_flags: u32,
    i_osd1: u32,
    i_block: [u32; 15],
    i_generation: u32,
    i_file_acl: u32,
    i_dir_acl: u32,
    i_faddr: u32,
    i_osd2: [u8; 12],
}

pub struct Ext2FileSystem {
    device: Arc<Mutex<dyn BlockDevice>>,
    block_size: usize,
    inodes_per_group: u32,
    inode_size: u16,
}

impl Ext2FileSystem {
    pub fn new(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<Mutex<Self>>, ()> {
        let mut buf = [0u8; 1024];
        device.lock().read_sector(2, &mut buf).map_err(|_| ())?;

        let sb = unsafe { &*(buf.as_ptr() as *const Superblock) };
        if sb.s_magic != EXT2_SUPER_MAGIC {
            return Err(());
        }

        let block_size = 1024 << sb.s_log_block_size;
        let inodes_per_group = sb.s_inodes_per_group;
        let inode_size = if sb.s_rev_level > 0 { sb.s_inode_size } else { 128 };

        Ok(Arc::new(Mutex::new(Ext2FileSystem {
            device,
            block_size,
            inodes_per_group,
            inode_size,
        })))
    }

    fn read_inode(&self, inode_num: u32) -> Result<Inode, ()> {
        let group = (inode_num - 1) / self.inodes_per_group;
        let index = (inode_num - 1) % self.inodes_per_group;

        let gd_block = if self.block_size == 1024 { 2 } else { 1 };
        let mut buf = vec![0u8; self.block_size];
        self.device.lock().read_sector((gd_block as u64 * self.block_size as u64) / 512, &mut buf).map_err(|_| ())?;

        let gds = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const GroupDescriptor, self.block_size / 32) };
        let gd = &gds[group as usize];

        let inode_table_block = gd.bg_inode_table;
        let inode_offset = index as u64 * self.inode_size as u64;
        let inode_sector = (inode_table_block as u64 * self.block_size as u64 + inode_offset) / 512;
        let sector_offset = (inode_table_block as u64 * self.block_size as u64 + inode_offset) % 512;

        let mut sector_buf = [0u8; 512];
        self.device.lock().read_sector(inode_sector, &mut sector_buf).map_err(|_| ())?;

        let inode = unsafe { *(sector_buf.as_ptr().add(sector_offset as usize) as *const Inode) };
        Ok(inode)
    }

    fn write_inode(&self, inode_num: u32, inode: &Inode) -> Result<(), ()> {
        let group = (inode_num - 1) / self.inodes_per_group;
        let index = (inode_num - 1) % self.inodes_per_group;

        let gd_block = if self.block_size == 1024 { 2 } else { 1 };
        let mut buf = vec![0u8; self.block_size];
        self.device.lock().read_sector((gd_block as u64 * self.block_size as u64) / 512, &mut buf).map_err(|_| ())?;

        let gds = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const GroupDescriptor, self.block_size / 32) };
        let gd = &gds[group as usize];

        let inode_table_block = gd.bg_inode_table;
        let inode_offset = index as u64 * self.inode_size as u64;
        let inode_sector = (inode_table_block as u64 * self.block_size as u64 + inode_offset) / 512;
        let sector_offset = (inode_table_block as u64 * self.block_size as u64 + inode_offset) % 512;

        let mut sector_buf = [0u8; 512];
        self.device.lock().read_sector(inode_sector, &mut sector_buf).map_err(|_| ())?;

        unsafe {
            let inode_ptr = sector_buf.as_mut_ptr().add(sector_offset as usize) as *mut Inode;
            *inode_ptr = *inode;
        }

        self.device.lock().write_sector(inode_sector, &sector_buf).map_err(|_| ())?;
        Ok(())
    }

    fn write_block(&self, block_num: u32, data: &[u8]) -> Result<(), ()> {
        if data.len() > self.block_size { return Err(()); }
        let sector = (block_num as u64 * self.block_size as u64) / 512;
        let sectors_per_block = self.block_size / 512;
        
        for i in 0..sectors_per_block {
            let offset = i * 512;
            let mut sector_buf = [0u8; 512];
            let copy_len = core::cmp::min(512, data.len().saturating_sub(offset));
            if copy_len > 0 {
                sector_buf[..copy_len].copy_from_slice(&data[offset..offset + copy_len]);
            }
            self.device.lock().write_sector(sector + i as u64, &sector_buf).map_err(|_| ())?;
        }
        Ok(())
    }

    fn allocate_block(&self) -> Result<u32, ()> {
        // Read Superblock to find total groups
        let mut buf = [0u8; 1024];
        self.device.lock().read_sector(2, &mut buf).map_err(|_| ())?;
        let sb = unsafe { &mut *(buf.as_mut_ptr() as *mut Superblock) };
        
        let block_count = sb.s_blocks_count;
        let blocks_per_group = sb.s_blocks_per_group;
        let group_count = (block_count + blocks_per_group - 1) / blocks_per_group;

        for g in 0..group_count {
            let gd_block = if self.block_size == 1024 { 2 } else { 1 };
            let mut gd_buf = vec![0u8; self.block_size];
            self.device.lock().read_sector((gd_block as u64 * self.block_size as u64) / 512, &mut gd_buf).map_err(|_| ())?;
            let gds = unsafe { core::slice::from_raw_parts_mut(gd_buf.as_mut_ptr() as *mut GroupDescriptor, self.block_size / 32) };
            let gd = &mut gds[g as usize];

            if gd.bg_free_blocks_count > 0 {
                let bitmap_block = gd.bg_block_bitmap;
                let mut bitmap = vec![0u8; self.block_size];
                let bitmap_sector = (bitmap_block as u64 * self.block_size as u64) / 512;
                self.device.lock().read_sector(bitmap_sector, &mut bitmap).map_err(|_| ())?;

                for i in 0..self.block_size {
                    if bitmap[i] != 0xFF {
                        for bit in 0..8 {
                            if (bitmap[i] & (1 << bit)) == 0 {
                                bitmap[i] |= 1 << bit;
                                self.device.lock().write_sector(bitmap_sector, &bitmap).map_err(|_| ())?;

                                gd.bg_free_blocks_count -= 1;
                                self.device.lock().write_sector((gd_block as u64 * self.block_size as u64) / 512, &gd_buf).map_err(|_| ())?;

                                sb.s_free_blocks_count -= 1;
                                self.device.lock().write_sector(2, &buf).map_err(|_| ())?;

                                return Ok(g * blocks_per_group + (i as u32 * 8 + bit + sb.s_first_data_block));
                            }
                        }
                    }
                }
            }
        }
        Err(())
    }

    fn allocate_inode(&self) -> Result<u32, ()> {
        let mut buf = [0u8; 1024];
        self.device.lock().read_sector(2, &mut buf).map_err(|_| ())?;
        let sb = unsafe { &mut *(buf.as_mut_ptr() as *mut Superblock) };
        
        let inode_count = sb.s_inodes_count;
        let inodes_per_group = sb.s_inodes_per_group;
        let group_count = (inode_count + inodes_per_group - 1) / inodes_per_group;

        for g in 0..group_count {
            let gd_block = if self.block_size == 1024 { 2 } else { 1 };
            let mut gd_buf = vec![0u8; self.block_size];
            self.device.lock().read_sector((gd_block as u64 * self.block_size as u64) / 512, &mut gd_buf).map_err(|_| ())?;
            let gds = unsafe { core::slice::from_raw_parts_mut(gd_buf.as_mut_ptr() as *mut GroupDescriptor, self.block_size / 32) };
            let gd = &mut gds[g as usize];

            if gd.bg_free_inodes_count > 0 {
                let bitmap_block = gd.bg_inode_bitmap;
                let mut bitmap = vec![0u8; self.block_size];
                let bitmap_sector = (bitmap_block as u64 * self.block_size as u64) / 512;
                self.device.lock().read_sector(bitmap_sector, &mut bitmap).map_err(|_| ())?;

                for i in 0..self.block_size {
                    if bitmap[i] != 0xFF {
                        for bit in 0..8 {
                            if (bitmap[i] & (1 << bit)) == 0 {
                                bitmap[i] |= 1 << bit;
                                self.device.lock().write_sector(bitmap_sector, &bitmap).map_err(|_| ())?;

                                gd.bg_free_inodes_count -= 1;
                                self.device.lock().write_sector((gd_block as u64 * self.block_size as u64) / 512, &gd_buf).map_err(|_| ())?;

                                sb.s_free_inodes_count -= 1;
                                self.device.lock().write_sector(2, &buf).map_err(|_| ())?;

                                return Ok(g * inodes_per_group + (i as u32 * 8 + bit + 1));
                            }
                        }
                    }
                }
            }
        }
        Err(())
    }
}

impl Ext2FileSystem {
    /// Collects all physical block indices for an inode, traversing indirect levels.
    fn read_all_block_indices(&self, inode: &Inode) -> Result<Vec<u32>, ()> {
        let mut blocks = Vec::new();
        // Direct blocks i_block[0..11]
        for i in 0..12 {
            if inode.i_block[i] == 0 { return Ok(blocks); }
            blocks.push(inode.i_block[i]);
        }
        // Singly indirect i_block[12]
        if inode.i_block[12] != 0 {
            blocks.append(&mut self.read_indirect_list(inode.i_block[12], 1)?);
        }
        // Doubly indirect i_block[13]
        if inode.i_block[13] != 0 {
            blocks.append(&mut self.read_indirect_list(inode.i_block[13], 2)?);
        }
        // Triply indirect i_block[14]
        if inode.i_block[14] != 0 {
            blocks.append(&mut self.read_indirect_list(inode.i_block[14], 3)?);
        }
        Ok(blocks)
    }

    /// Reads a block pointer list at `level` levels of indirection.
    /// level=1: block contains direct u32 block pointers
    /// level=2: block contains pointers to level-1 blocks
    /// level=3: block contains pointers to level-2 blocks
    fn read_indirect_list(&self, block_num: u32, level: u32) -> Result<Vec<u32>, ()> {
        let entries_per_block = self.block_size / 4;
        let mut buf = vec![0u8; self.block_size];
        self.device.lock().read_sector(
            (block_num as u64 * self.block_size as u64) / 512,
            &mut buf,
        ).map_err(|_| ())?;

        let ptrs = unsafe {
            core::slice::from_raw_parts(buf.as_ptr() as *const u32, entries_per_block)
        };

        let mut result = Vec::new();
        if level == 1 {
            for &p in ptrs {
                if p == 0 { break; }
                result.push(p);
            }
        } else {
            for &p in ptrs {
                if p == 0 { break; }
                result.append(&mut self.read_indirect_list(p, level - 1)?);
            }
        }
        Ok(result)
    }

    /// Allocates and writes data through indirect blocks as needed.
    /// Returns the number of blocks written.
    fn write_blocks_indirect(&self, inode: &mut Inode, data: &[u8]) -> Result<(), ()> {
        let block_size = self.block_size;
        let blocks_needed = (data.len() + block_size - 1) / block_size;
        let entries_per_block = block_size / 4;

        // Helper: set a block pointer at given level, allocating intermediate blocks
        fn set_block_ptr(
            fs: &Ext2FileSystem,
            start_block: &mut u32,
            level: u32,
            index: usize,
            entries_per_block: usize,
            target_block: u32,
        ) -> Result<(), ()> {
            if *start_block == 0 {
                *start_block = fs.allocate_block()?;
            }
            if level == 1 {
                // Write directly into the block
                let mut buf = vec![0u8; fs.block_size];
                let sector = (*start_block as u64 * fs.block_size as u64) / 512;
                let _ = fs.device.lock().read_sector(sector, &mut buf);
                unsafe {
                    let ptrs = buf.as_mut_ptr() as *mut u32;
                    *ptrs.add(index) = target_block;
                }
                fs.device.lock().write_sector(sector, &buf).map_err(|_| ())?;
                Ok(())
            } else {
                // Read intermediate block, find sub-pointer
                let mut buf = vec![0u8; fs.block_size];
                let sector = (*start_block as u64 * fs.block_size as u64) / 512;
                fs.device.lock().read_sector(sector, &mut buf).map_err(|_| ())?;
                let sub_index = index % entries_per_block;
                let sub_level = level - 1;
                let mut sub_block = unsafe { *(buf.as_ptr() as *const u32).add(sub_index) };
                drop(buf);
                set_block_ptr(fs, &mut sub_block, sub_level, index / entries_per_block, entries_per_block, target_block)?;
                // Re-write intermediate block with updated sub-block pointer
                let mut buf2 = vec![0u8; fs.block_size];
                let sector2 = (*start_block as u64 * fs.block_size as u64) / 512;
                fs.device.lock().read_sector(sector2, &mut buf2).map_err(|_| ())?;
                unsafe {
                    let ptrs = buf2.as_mut_ptr() as *mut u32;
                    *ptrs.add(sub_index) = sub_block;
                }
                fs.device.lock().write_sector(sector2, &buf2).map_err(|_| ())?;
                Ok(())
            }
        }

        // Allocate/write each block
        for i in 0..blocks_needed {
            let offset = i * block_size;
            let len = core::cmp::min(block_size, data.len() - offset);
            let mut block_data = vec![0u8; block_size];
            block_data[..len].copy_from_slice(&data[offset..offset + len]);

            let block_num = if i < 12 {
                // Direct block
                if inode.i_block[i] == 0 {
                    inode.i_block[i] = self.allocate_block()?;
                }
                inode.i_block[i]
            } else {
                // Indirect — allocate a new data block
                let new_data_block = self.allocate_block()?;
                let idx = i - 12;
                if idx < entries_per_block {
                    // Singly indirect
                    let mut blk = inode.i_block[12];
                    set_block_ptr(self, &mut blk, 1, idx, entries_per_block, new_data_block)?;
                    inode.i_block[12] = blk;
                } else if idx < entries_per_block * entries_per_block {
                    // Doubly indirect
                    let mut blk = inode.i_block[13];
                    set_block_ptr(self, &mut blk, 2, idx, entries_per_block, new_data_block)?;
                    inode.i_block[13] = blk;
                } else {
                    // Triply indirect
                    let mut blk = inode.i_block[14];
                    set_block_ptr(self, &mut blk, 3, idx, entries_per_block, new_data_block)?;
                    inode.i_block[14] = blk;
                }
                new_data_block
            };

            self.write_block(block_num, &block_data)?;
        }

        inode.i_size = data.len() as u32;
        let total_sectors = blocks_needed * block_size / 512;
        inode.i_blocks = total_sectors as u32;
        Ok(())
    }
}

pub fn mount(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<Ext2FileSystemHandle>, ()> {
    let fs = Ext2FileSystem::new(device)?;
    Ok(Arc::new(Ext2FileSystemHandle { fs }))
}

pub struct Ext2FileSystemHandle {
    fs: Arc<Mutex<Ext2FileSystem>>,
}

impl FileSystem for Ext2FileSystemHandle {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        let fs_lock = self.fs.lock();
        let inode = fs_lock.read_inode(2)?;
        Ok(Arc::new(Ext2Node {
            fs: self.fs.clone(),
            name: String::from(""),
            inode_num: 2,
            inode,
        }))
    }
}

#[repr(C, packed)]
struct DirectoryEntry {
    inode: u32,
    rec_len: u16,
    name_len: u8,
    file_type: u8,
}

pub struct Ext2Node {
    fs: Arc<Mutex<Ext2FileSystem>>,
    name: String,
    inode_num: u32,
    inode: Inode,
}

impl VfsNode for Ext2Node {
    fn name(&self) -> String { self.name.clone() }
    fn is_dir(&self) -> bool { (self.inode.i_mode & 0xF000) == 0x4000 }
    
    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir() { return Err(()); }
        
        let fs = self.fs.lock();
        let mut children = Vec::new();
        
        // All blocks including indirect
        let block_indices = fs.read_all_block_indices(&self.inode)?;
        for block_num in block_indices {
            if block_num == 0 { break; }
            
            let mut block_buf = vec![0u8; fs.block_size];
            fs.device.lock().read_sector((block_num as u64 * fs.block_size as u64) / 512, &mut block_buf).map_err(|_| ())?;
            
            let mut offset = 0;
            while offset < fs.block_size {
                let entry = unsafe { &*(block_buf.as_ptr().add(offset) as *const DirectoryEntry) };
                if entry.inode == 0 { break; }
                
                let name_ptr = unsafe { block_buf.as_ptr().add(offset + 8) };
                let name_slice = unsafe { core::slice::from_raw_parts(name_ptr, entry.name_len as usize) };
                let name = String::from_utf8_lossy(name_slice).into_owned();
                
                if name != "." && name != ".." {
                    if let Ok(child_inode) = fs.read_inode(entry.inode) {
                        children.push(Arc::new(Ext2Node {
                            fs: self.fs.clone(),
                            name,
                            inode_num: entry.inode,
                            inode: child_inode,
                        }) as Arc<dyn VfsNode>);
                    }
                }
                
                if entry.rec_len == 0 { break; }
                offset += entry.rec_len as usize;
            }
        }
        
        Ok(children)
    }

    fn find_child(&self, name: &str) -> Option<Arc<dyn VfsNode>> {
        if !self.is_dir() { return None; }
        
        let fs = self.fs.lock();
        let block_indices = fs.read_all_block_indices(&self.inode).ok()?;
        
        for block_num in block_indices {
            if block_num == 0 { break; }
            
            let mut block_buf = vec![0u8; fs.block_size];
            fs.device.lock().read_sector((block_num as u64 * fs.block_size as u64) / 512, &mut block_buf).ok()?;
            
            let mut offset = 0;
            while offset < fs.block_size {
                let entry = unsafe { &*(block_buf.as_ptr().add(offset) as *const DirectoryEntry) };
                if entry.inode == 0 { break; }
                
                let entry_name_ptr = unsafe { block_buf.as_ptr().add(offset + 8) };
                let entry_name_slice = unsafe { core::slice::from_raw_parts(entry_name_ptr, entry.name_len as usize) };
                let entry_name = core::str::from_utf8(entry_name_slice).unwrap_or("");
                
                if entry_name == name {
                    let child_inode = fs.read_inode(entry.inode).ok()?;
                    return Some(Arc::new(Ext2Node {
                        fs: self.fs.clone(),
                        name: String::from(entry_name),
                        inode_num: entry.inode,
                        inode: child_inode,
                    }));
                }
                
                if entry.rec_len == 0 { break; }
                offset += entry.rec_len as usize;
            }
        }
        None
    }
    
    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        if self.is_dir() { return Err(()); }
        
        let fs = self.fs.lock();
        let mut data = Vec::with_capacity(self.inode.i_size as usize);
        
        // Collect all blocks including indirect levels
        let block_indices = fs.read_all_block_indices(&self.inode)?;
        for &b in &block_indices {
            if b == 0 { break; }
            let mut block_buf = vec![0u8; fs.block_size];
            fs.device.lock().read_sector((b as u64 * fs.block_size as u64) / 512, &mut block_buf).map_err(|_| ())?;
            let remaining = self.inode.i_size as usize - data.len();
            let to_copy = core::cmp::min(remaining, fs.block_size);
            data.extend_from_slice(&block_buf[..to_copy]);
            if data.len() >= self.inode.i_size as usize { break; }
        }
        
        Ok(data)
    }

    fn stat(&self) -> Result<Stat, ()> {
        Ok(Stat {
            st_dev: 0,
            st_ino: self.inode_num as u64,
            st_mode: self.inode.i_mode as u32,
            st_nlink: self.inode.i_links_count as u32,
            st_uid: self.inode.i_uid as u32,
            st_gid: self.inode.i_gid as u32,
            st_rdev: 0,
            st_size: self.inode.i_size as i64,
            st_atime: self.inode.i_atime as i64,
            st_mtime: self.inode.i_mtime as i64,
            st_ctime: self.inode.i_ctime as i64,
        })
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        if self.is_dir() { return Err(()); }

        let mut inode = self.inode;
        let fs = self.fs.lock();

        fs.write_blocks_indirect(&mut inode, data)?;
        fs.write_inode(self.inode_num, &inode)?;
        Ok(())
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs_lock = self.fs.lock();
        let new_inode_num = fs_lock.allocate_inode()?;
        
        let new_inode = Inode {
            i_mode: 0x81A4, // Regular file, 644
            i_uid: 0,
            i_size: 0,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 1,
            i_blocks: 0,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; 15],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };
        
        fs_lock.write_inode(new_inode_num, &new_inode)?;
        Self::add_directory_entry(&fs_lock, self.inode_num, new_inode_num, name, 1)?; // 1 = regular file

        Ok(Arc::new(Ext2Node {
            fs: self.fs.clone(),
            name: String::from(name),
            inode_num: new_inode_num,
            inode: new_inode,
        }))
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs_lock = self.fs.lock();
        let new_inode_num = fs_lock.allocate_inode()?;
        let new_block_num = fs_lock.allocate_block()?;

        let mut new_inode = Inode {
            i_mode: 0x41ED, // Directory, 755
            i_uid: 0,
            i_size: fs_lock.block_size as u32,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 2, // . and parent reference
            i_blocks: (fs_lock.block_size / 512) as u32,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; 15],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };
        new_inode.i_block[0] = new_block_num;
        
        // Initialize directory block with . and ..
        let mut block_data = vec![0u8; fs_lock.block_size];
        
        // . entry
        let dot = unsafe { &mut *(block_data.as_mut_ptr() as *mut DirectoryEntry) };
        dot.inode = new_inode_num;
        dot.rec_len = 12;
        dot.name_len = 1;
        dot.file_type = 2; // Directory
        block_data[8] = b'.';

        // .. entry
        let dotdot = unsafe { &mut *(block_data.as_mut_ptr().add(12) as *mut DirectoryEntry) };
        dotdot.inode = self.inode_num;
        dotdot.rec_len = (fs_lock.block_size - 12) as u16;
        dotdot.name_len = 2;
        dotdot.file_type = 2;
        block_data[12 + 8] = b'.';
        block_data[12 + 9] = b'.';

        fs_lock.write_block(new_block_num, &block_data)?;
        fs_lock.write_inode(new_inode_num, &new_inode)?;
        
        Self::add_directory_entry(&fs_lock, self.inode_num, new_inode_num, name, 2)?;

        Ok(Arc::new(Ext2Node {
            fs: self.fs.clone(),
            name: String::from(name),
            inode_num: new_inode_num,
            inode: new_inode,
        }))
    }
}

impl Ext2Node {
    fn add_directory_entry(fs: &Ext2FileSystem, parent_inode_num: u32, child_inode_num: u32, name: &str, file_type: u8) -> Result<(), ()> {
        let parent_inode = fs.read_inode(parent_inode_num)?;
        let block_num = parent_inode.i_block[0]; 
        let mut block_data = vec![0u8; fs.block_size];
        let sector = (block_num as u64 * fs.block_size as u64) / 512;
        fs.device.lock().read_sector(sector, &mut block_data).map_err(|_| ())?;

        let mut offset = 0;
        loop {
            let entry = unsafe { &mut *(block_data.as_mut_ptr().add(offset) as *mut DirectoryEntry) };
            let actual_len = (8 + entry.name_len as usize + 3) & !3;
            let available_len = entry.rec_len as usize - actual_len;
            
            let new_entry_len = (8 + name.len() + 3) & !3;
            
            if available_len >= new_entry_len {
                let old_rec_len = entry.rec_len;
                entry.rec_len = actual_len as u16;
                
                let next_offset = offset + actual_len;
                let new_entry = unsafe { &mut *(block_data.as_mut_ptr().add(next_offset) as *mut DirectoryEntry) };
                new_entry.inode = child_inode_num;
                new_entry.rec_len = old_rec_len - actual_len as u16;
                new_entry.name_len = name.len() as u8;
                new_entry.file_type = file_type;
                
                let name_ptr = unsafe { block_data.as_mut_ptr().add(next_offset + 8) };
                unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), name_ptr, name.len()); }
                
                fs.write_block(block_num, &block_data)?;
                return Ok(());
            }
            
            offset += entry.rec_len as usize;
            if offset >= fs.block_size { break; }
        }
        Err(())
    }
}

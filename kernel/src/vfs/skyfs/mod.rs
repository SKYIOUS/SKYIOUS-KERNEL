#![allow(dead_code)]

pub mod btree;
pub mod journal;
pub mod alloc;
pub mod inode;
pub mod dir;

use crate::alloc::sync::Arc;
use crate::alloc::vec::Vec;
use crate::alloc::string::String;
use spin::Mutex;
use crate::vfs::{FileSystem, VfsNode, Stat, StatFs};
use crate::drivers::block::BlockDevice;

pub const BLOCK_SIZE: usize = 4096;
pub const SECTOR_SIZE: usize = 512;
pub const SECTORS_PER_BLOCK: u64 = (BLOCK_SIZE / SECTOR_SIZE) as u64;
pub const INODE_SIZE: usize = 256;
pub const SKYFS_MAGIC: u64 = 0x315620534B59534B; // "SKYFS V1" in little-endian
pub const SKYFS_VERSION: u32 = 1;
pub const MAX_INLINE_DATA: usize = 256;
pub const BTREE_MODE: u32 = 0xFFFFFFFF;
pub const STATE_CLEAN: u8 = 1;
pub const STATE_DIRTY: u8 = 2;
pub const STATE_ERROR: u8 = 3;

#[repr(C, packed)]
pub struct Superblock {
    pub magic: u64,
    pub version: u32,
    pub block_size: u32,
    pub total_blocks: u64,
    pub journal_start: u64,
    pub journal_blocks: u64,
    pub bitmap_start: u64,
    pub bitmap_blocks: u64,
    pub inode_start: u64,
    pub inode_count: u64,
    pub inode_blocks: u64,
    pub root_inode: u64,
    pub state: u8,
    pub _padding: [u8; 4023],
}

#[repr(C, packed)]
pub struct SkyfsInode {
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub links: u32,
    pub flags: u32,
    pub block_count: u64,
    pub extent_count: u32,
    pub data: [u8; MAX_INLINE_DATA],
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct Extent {
    pub start_block: u64,
    pub block_count: u64,
}

#[repr(C, packed)]
pub struct DirEntry {
    pub inode: u64,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
}

// ── SkyFS Core ────────────────────────────────────────────────────
pub struct SkyFS {
    pub device: Arc<Mutex<dyn BlockDevice>>,
    pub sb: Superblock,
    pub journal: spin::Mutex<journal::Journal>,
}

impl SkyFS {
    pub fn format(device: &Arc<Mutex<dyn BlockDevice>>) -> Result<(), ()> {
        let mut dev = device.lock();
        let total_sectors = dev.sector_count().map_err(|_| ())?;
        let total_blocks = total_sectors / SECTORS_PER_BLOCK;
        if total_blocks < 128 { return Err(()); }
        let journal_blocks = 64u64;
        let inode_count = (total_blocks / 4).min(65536).max(256);
        let inodes_per_block = (BLOCK_SIZE / INODE_SIZE) as u64;
        let inode_blocks = (inode_count + inodes_per_block - 1) / inodes_per_block;
        let bitmap_blocks = (total_blocks + (BLOCK_SIZE as u64 * 8) - 1) / (BLOCK_SIZE as u64 * 8);
        let bitmap_start = 1 + journal_blocks;
        let inode_start = bitmap_start + bitmap_blocks;
        let data_start = inode_start + inode_blocks;
        let _data_blocks = total_blocks - data_start;

        let sb = Superblock {
            magic: SKYFS_MAGIC,
            version: SKYFS_VERSION,
            block_size: BLOCK_SIZE as u32,
            total_blocks,
            journal_start: 1,
            journal_blocks,
            bitmap_start,
            bitmap_blocks,
            inode_start,
            inode_count,
            inode_blocks,
            root_inode: 1,
            state: STATE_CLEAN,
            _padding: [0u8; 4023],
        };

        // Write superblock
        let mut sb_buf = [0u8; BLOCK_SIZE];
        let sb_src = unsafe {
            core::slice::from_raw_parts(&sb as *const Superblock as *const u8, core::mem::size_of::<Superblock>())
        };
        sb_buf[..core::mem::size_of::<Superblock>()].copy_from_slice(sb_src);
        SkyFS::write_block(&mut *dev, 0, &sb_buf)?;

        // Initialize journal area
        journal::Journal::init_device(&mut *dev, 1, journal_blocks)?;

        // Initialize bitmap: mark superblock, journal, bitmap, inode table as used
        let first_data_block = data_start;
        let mut bitmap_buf = crate::alloc::vec![0u8; BLOCK_SIZE];
        for b in 0..first_data_block {
            let byte_idx = (b / 8) as usize % BLOCK_SIZE;
            let bit_idx = (b % 8) as u8;
            if byte_idx < bitmap_buf.len() {
                bitmap_buf[byte_idx] |= 1 << bit_idx;
            }
        }
        for bm in 0..bitmap_blocks {
            SkyFS::write_block(&mut *dev, bitmap_start + bm, &bitmap_buf)?;
        }

        // Initialize root inode (inode 1)
        let root_inode = SkyfsInode {
            mode: 0o040755,
            uid: 0, gid: 0, size: 0,
            atime: 0, mtime: 0, ctime: 0,
            links: 2,
            flags: 0, block_count: 0, extent_count: 0,
            data: [0u8; MAX_INLINE_DATA],
        };
        let inode_per_block = BLOCK_SIZE / INODE_SIZE;
        let root_block = inode_start + 1 / inode_per_block as u64;
        let root_off = (1 % inode_per_block as u64) * INODE_SIZE as u64;
        let mut inode_buf = [0u8; BLOCK_SIZE];
        SkyFS::read_block(&mut *dev, root_block, &mut inode_buf)?;
        let dst = &mut inode_buf[root_off as usize..root_off as usize + INODE_SIZE];
        let src = unsafe {
            core::slice::from_raw_parts(&root_inode as *const SkyfsInode as *const u8, INODE_SIZE)
        };
        dst.copy_from_slice(src);
        SkyFS::write_block(&mut *dev, root_block, &inode_buf)?;

        // Initialize root directory with "." and ".."
        let dot = DirEntry { inode: 1, rec_len: 12, name_len: 1, file_type: 0 };
        let dot_dot = DirEntry { inode: 1, rec_len: 12, name_len: 2, file_type: 0 };
        let mut root_data = [0u8; MAX_INLINE_DATA];
        let mut off = 0usize;
        write_dirent(&mut root_data, &mut off, &dot, b".");
        write_dirent(&mut root_data, &mut off, &dot_dot, b"..");
        // Update root inode data with dir entries
        let mut buf = [0u8; BLOCK_SIZE];
        SkyFS::read_block(&mut *dev, root_block, &mut buf)?;
        let root_dst = &mut buf[root_off as usize..root_off as usize + INODE_SIZE];
        let mut updated_root: SkyfsInode;
        // Need to copy from the block buffer
        updated_root = unsafe { core::ptr::read_unaligned(root_dst.as_ptr() as *const SkyfsInode) };
        updated_root.data[..off].copy_from_slice(&root_data[..off]);
        updated_root.size = off as u64;
        let root_src = unsafe {
            core::slice::from_raw_parts(&updated_root as *const SkyfsInode as *const u8, INODE_SIZE)
        };
        root_dst.copy_from_slice(root_src);
        SkyFS::write_block(&mut *dev, root_block, &buf)?;

        drop(dev);
        Ok(())
    }

    pub fn mount(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<Mutex<SkyFS>>, ()> {
        let mut sb_buf = [0u8; BLOCK_SIZE];
        let mut dev = device.lock();
        for i in 0..SECTORS_PER_BLOCK {
            dev.read_sector(i, &mut sb_buf[i as usize * SECTOR_SIZE..(i as usize + 1) * SECTOR_SIZE])
                .map_err(|_| ())?;
        }
        drop(dev);
        let sb: &Superblock = unsafe { &*(sb_buf.as_ptr() as *const Superblock) };
        if sb.magic != SKYFS_MAGIC || sb.version != SKYFS_VERSION {
            return Err(());
        }
        // Attempt journal recovery
        let j_start = sb.journal_start;
        let j_blocks = sb.journal_blocks;
        let mut journal = journal::Journal::new(j_start, j_blocks);
        {
            let mut dev = device.lock();
            let _ = journal::Journal::recover_from_dev(&mut *dev, &mut journal);
        }
        // Mark filesystem as dirty
        {
            let mut dev = device.lock();
            if sb.state != STATE_DIRTY {
                let _ = SkyFS::set_state(&mut *dev, STATE_DIRTY);
            }
        }

        let fs = Arc::new(Mutex::new(SkyFS {
            device,
            journal: spin::Mutex::new(journal::Journal::new(j_start, j_blocks)),
            sb: Superblock {
                magic: sb.magic, version: sb.version, block_size: sb.block_size,
                total_blocks: sb.total_blocks, journal_start: sb.journal_start,
                journal_blocks: sb.journal_blocks, bitmap_start: sb.bitmap_start,
                bitmap_blocks: sb.bitmap_blocks, inode_start: sb.inode_start,
                inode_count: sb.inode_count, inode_blocks: sb.inode_blocks,
                root_inode: sb.root_inode, state: sb.state, _padding: [0u8; 4023],
            },
        }));
        Ok(fs)
    }

    pub fn block_to_sector(block: u64) -> u64 { block * SECTORS_PER_BLOCK }

    pub fn read_block(dev: &mut dyn BlockDevice, block: u64, buf: &mut [u8]) -> Result<(), ()> {
        if buf.len() < BLOCK_SIZE { return Err(()); }
        for i in 0..SECTORS_PER_BLOCK {
            let sector = Self::block_to_sector(block) + i;
            dev.read_sector(sector, &mut buf[i as usize * SECTOR_SIZE..(i as usize + 1) * SECTOR_SIZE])
                .map_err(|_| ())?;
        }
        Ok(())
    }

    pub fn write_block(dev: &mut dyn BlockDevice, block: u64, buf: &[u8]) -> Result<(), ()> {
        if buf.len() < BLOCK_SIZE { return Err(()); }
        for i in 0..SECTORS_PER_BLOCK {
            let sector = Self::block_to_sector(block) + i;
            dev.write_sector(sector, &buf[i as usize * SECTOR_SIZE..(i as usize + 1) * SECTOR_SIZE])
                .map_err(|_| ())?;
        }
        Ok(())
    }

    pub fn set_state(dev: &mut dyn BlockDevice, state: u8) -> Result<(), ()> {
        let mut sb_buf = [0u8; BLOCK_SIZE];
        for i in 0..SECTORS_PER_BLOCK {
            dev.read_sector(i, &mut sb_buf[i as usize * SECTOR_SIZE..(i as usize + 1) * SECTOR_SIZE])
                .map_err(|_| ())?;
        }
        sb_buf[38] = state; // offset of `state` field in Superblock
        for i in 0..SECTORS_PER_BLOCK {
            dev.write_sector(i, &sb_buf[i as usize * SECTOR_SIZE..(i as usize + 1) * SECTOR_SIZE])
                .map_err(|_| ())?;
        }
        Ok(())
    }
}

// ── FileSystem trait impl ─────────────────────────────────────────
pub struct SkyFSHandle {
    pub fs: Arc<Mutex<SkyFS>>,
}

impl SkyFSHandle {
    pub fn format(device: Arc<Mutex<dyn BlockDevice>>) -> Result<(), ()> {
        SkyFS::format(&device)
    }

    pub fn mount(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<SkyFSHandle>, ()> {
        let fs = SkyFS::mount(device)?;
        Ok(Arc::new(SkyFSHandle { fs }))
    }
}

impl FileSystem for SkyFSHandle {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let root_ino = fs.sb.root_inode;
        let inode = read_inode_inner(&self.fs, root_ino)?;
        Ok(Arc::new(SkyfsNode {
            fs: self.fs.clone(),
            ino: root_ino,
            inode: Mutex::new(inode),
        }) as Arc<dyn VfsNode>)
    }
}

// ── Inode helpers ─────────────────────────────────────────────────
fn read_inode_inner(fs: &Arc<Mutex<SkyFS>>, ino: u64) -> Result<SkyfsInode, ()> {
    let fs_lock = fs.lock();
    let inode_block = fs_lock.sb.inode_start + (ino * INODE_SIZE as u64) / BLOCK_SIZE as u64;
    let offset = ((ino * INODE_SIZE as u64) % BLOCK_SIZE as u64) as usize;
    let mut buf = [0u8; BLOCK_SIZE];
    let mut dev = fs_lock.device.lock();
    SkyFS::read_block(&mut *dev, inode_block, &mut buf)?;
    drop(dev);
    drop(fs_lock);
    if offset + INODE_SIZE > BLOCK_SIZE { return Err(()); }
    let raw = unsafe { &*(buf[offset..].as_ptr() as *const SkyfsInode) };
    Ok(SkyfsInode {
        mode: raw.mode, uid: raw.uid, gid: raw.gid, size: raw.size,
        atime: raw.atime, mtime: raw.mtime, ctime: raw.ctime,
        links: raw.links, flags: raw.flags, block_count: raw.block_count,
        extent_count: raw.extent_count, data: raw.data,
    })
}

fn write_inode_inner(fs: &Arc<Mutex<SkyFS>>, ino: u64, inode: &SkyfsInode) -> Result<(), ()> {
    let fs_lock = fs.lock();
    let inode_block = fs_lock.sb.inode_start + (ino * INODE_SIZE as u64) / BLOCK_SIZE as u64;
    let offset = ((ino * INODE_SIZE as u64) % BLOCK_SIZE as u64) as usize;
    let mut buf = [0u8; BLOCK_SIZE];
    let mut dev = fs_lock.device.lock();
    SkyFS::read_block(&mut *dev, inode_block, &mut buf)?;
    if offset + INODE_SIZE > BLOCK_SIZE { return Err(()); }
    let dst = &mut buf[offset..offset + INODE_SIZE];
    let src = unsafe { core::slice::from_raw_parts(inode as *const SkyfsInode as *const u8, INODE_SIZE) };
    dst.copy_from_slice(src);

    // Journal the write
    let mut journal_lock = fs_lock.journal.lock();
    let hdr = journal::Journal::begin_transaction(&mut *dev, &mut *journal_lock)?;
    journal::Journal::journal_data(&mut *dev, &mut *journal_lock, &buf)?;
    SkyFS::write_block(&mut *dev, inode_block, &buf)?;
    journal::Journal::commit_transaction(&mut *dev, &mut *journal_lock, hdr)?;
    drop(journal_lock);
    drop(dev);
    drop(fs_lock);
    Ok(())
}

fn read_file_data(fs: &Arc<Mutex<SkyFS>>, inode: &SkyfsInode) -> Result<Vec<u8>, ()> {
    let size = inode.size as usize;
    if size == 0 { return Ok(Vec::new()); }
    if size <= MAX_INLINE_DATA {
        return Ok(inode.data[..size].to_vec());
    }
    let fs_lock = fs.lock();
    let mut dev = fs_lock.device.lock();
    if inode.extent_count == BTREE_MODE {
        let root_block = u64::from_le_bytes(inode.data[..8].try_into().unwrap());
        let mut data = Vec::with_capacity(size);
        let mut logical_idx = 0u64;
        while data.len() < size {
            if let Some(extent) = btree::lookup_extent(fs, root_block, logical_idx) {
                for b in 0..extent.block_count {
                    if data.len() >= size { break; }
                    let mut block_buf = [0u8; BLOCK_SIZE];
                    SkyFS::read_block(&mut *dev, extent.start_block + b, &mut block_buf)?;
                    let remaining = size - data.len();
                    let to_copy = BLOCK_SIZE.min(remaining);
                    data.extend_from_slice(&block_buf[..to_copy]);
                }
                logical_idx += extent.block_count;
            } else {
                break;
            }
        }
        Ok(data)
    } else {
        let extents = unsafe {
            core::slice::from_raw_parts(inode.data.as_ptr() as *const Extent, inode.extent_count as usize)
        };
        let mut data = Vec::with_capacity(size);
        for ext in extents {
            for b in 0..ext.block_count {
                let mut block_buf = [0u8; BLOCK_SIZE];
                SkyFS::read_block(&mut *dev, ext.start_block + b, &mut block_buf)?;
                data.extend_from_slice(&block_buf);
                if data.len() >= size { break; }
            }
            if data.len() >= size { break; }
        }
        data.truncate(size);
        Ok(data)
    }
}

fn read_dir_data(fs: &Arc<Mutex<SkyFS>>, inode: &SkyfsInode) -> Result<Vec<u8>, ()> {
    read_file_data(fs, inode)
}

fn store_data_blocks(fs: &Arc<Mutex<SkyFS>>, inode: &mut SkyfsInode,
    data: &[u8]) -> Result<(), ()> {
    let data_len = data.len();
    if data_len <= MAX_INLINE_DATA {
        inode.data[..data_len].copy_from_slice(data);
        if data_len < MAX_INLINE_DATA {
            inode.data[data_len..].fill(0);
        }
        inode.extent_count = 0;
        inode.size = data_len as u64;
        return Ok(());
    }
    let blocks_needed = (data_len + BLOCK_SIZE - 1) / BLOCK_SIZE;
    let mut extents: Vec<Extent> = crate::alloc::vec![];
    let mut offset = 0;
    let fs_lock = fs.lock();
    let mut dev = fs_lock.device.lock();
    for _ in 0..blocks_needed {
        let block = alloc::allocate_block_inner(fs, &mut *dev)?;
        let mut block_buf = [0u8; BLOCK_SIZE];
        let to_copy = BLOCK_SIZE.min(data_len - offset);
        block_buf[..to_copy].copy_from_slice(&data[offset..offset + to_copy]);
        SkyFS::write_block(&mut *dev, block, &block_buf)?;
        offset += BLOCK_SIZE;
        if let Some(last) = extents.last_mut() {
            if last.start_block + last.block_count == block {
                last.block_count += 1;
                continue;
            }
        }
        extents.push(Extent { start_block: block, block_count: 1 });
    }
    drop(dev);
    drop(fs_lock);

    let extent_count = extents.len();
    let extent_bytes = unsafe {
        core::slice::from_raw_parts(extents.as_ptr() as *const u8,
            extent_count * core::mem::size_of::<Extent>())
    };

    // If extents fit inline, store them there
    if extent_count * core::mem::size_of::<Extent>() <= MAX_INLINE_DATA && inode.extent_count != BTREE_MODE {
        let copy_len = extent_bytes.len().min(MAX_INLINE_DATA);
        inode.data[..copy_len].copy_from_slice(&extent_bytes[..copy_len]);
        if copy_len < MAX_INLINE_DATA {
            inode.data[copy_len..].fill(0);
        }
        inode.extent_count = extent_count as u32;
    } else {
        // Switch to btree mode
        let mut root_block = if inode.extent_count == BTREE_MODE {
            u64::from_le_bytes(inode.data[..8].try_into().unwrap())
        } else { 0 };
        for (i, ext) in extents.iter().enumerate() {
            btree::insert_extent(fs, &mut root_block, i as u64, *ext)?;
        }
        inode.data[..8].copy_from_slice(&root_block.to_le_bytes());
        if 8 < MAX_INLINE_DATA {
            inode.data[8..].fill(0);
        }
        inode.extent_count = BTREE_MODE;
    }
    inode.size = data_len as u64;
    Ok(())
}

fn add_entry_impl(fs: &Arc<Mutex<SkyFS>>, ino: u64, name: &str, child_ino: u64) -> Result<(), ()> {
    let inode = read_inode_inner(fs, ino)?;
    let mut data = read_dir_data(fs, &inode)?;
    let entry = DirEntry {
        inode: child_ino,
        rec_len: ((12 + name.len() + 3) & !3) as u16,
        name_len: name.len() as u8,
        file_type: 0,
    };
    write_dirent_append(&mut data, &entry, name.as_bytes());
    let mut inode = read_inode_inner(fs, ino)?;
    store_data_blocks(fs, &mut inode, &data)?;
    write_inode_inner(fs, ino, &inode)
}

// ── SkyFS VFS Node ────────────────────────────────────────────────
pub struct SkyfsNode {
    pub fs: Arc<Mutex<SkyFS>>,
    pub ino: u64,
    pub inode: Mutex<SkyfsInode>,
}

impl SkyfsNode {
    pub fn new(fs: Arc<Mutex<SkyFS>>, ino: u64) -> Result<Self, ()> {
        let inode = read_inode_inner(&fs, ino)?;
        Ok(SkyfsNode { fs, ino, inode: Mutex::new(inode) })
    }
}

impl VfsNode for SkyfsNode {
    fn name(&self) -> String {
        crate::alloc::format!("ino:{}", self.ino)
    }

    fn is_dir(&self) -> bool {
        (self.inode.lock().mode & 0o170000) == 0o040000
    }

    fn read(&self, max_len: usize) -> Result<Vec<u8>, ()> {
        let inode = self.inode.lock();
        if (inode.mode & 0o170000) != 0o100000 && (inode.mode & 0o170000) != 0o040000 {
            return Err(());
        }
        let size = inode.size as usize;
        let len = size.min(max_len);
        if size <= MAX_INLINE_DATA {
            let mut data = Vec::with_capacity(len);
            data.extend_from_slice(&inode.data[..len]);
            return Ok(data);
        }
        if inode.extent_count == BTREE_MODE {
            let root_block = u64::from_le_bytes(inode.data[..8].try_into().unwrap());
            let mut logical_idx = 0u64;
            let fs_lock = self.fs.lock();
            let mut dev = fs_lock.device.lock();
            let mut data = Vec::with_capacity(len);
            while data.len() < len {
                if let Some(extent) = btree::lookup_extent(&self.fs, root_block, logical_idx) {
                    for b in 0..extent.block_count {
                        if data.len() >= len { break; }
                        let mut block_buf = [0u8; BLOCK_SIZE];
                        SkyFS::read_block(&mut *dev, extent.start_block + b, &mut block_buf)?;
                        let remaining = len - data.len();
                        let to_copy = BLOCK_SIZE.min(remaining);
                        data.extend_from_slice(&block_buf[..to_copy]);
                    }
                    logical_idx += extent.block_count;
                } else { break; }
            }
            return Ok(data);
        }
        let extents = unsafe {
            core::slice::from_raw_parts(inode.data.as_ptr() as *const Extent, inode.extent_count as usize)
        };
        let mut remaining = len;
        let fs_lock = self.fs.lock();
        let mut dev = fs_lock.device.lock();
        let mut data = Vec::with_capacity(len);
        for ext in extents {
            if remaining == 0 { break; }
            for b in 0..ext.block_count {
                if remaining == 0 { break; }
                let mut block_buf = [0u8; BLOCK_SIZE];
                SkyFS::read_block(&mut *dev, ext.start_block + b, &mut block_buf)?;
                let to_copy = BLOCK_SIZE.min(remaining);
                data.extend_from_slice(&block_buf[..to_copy]);
                remaining -= to_copy;
            }
        }
        Ok(data)
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        let mut inode = self.inode.lock();
        if (inode.mode & 0o170000) != 0o100000 { return Err(()); }
        store_data_blocks(&self.fs, &mut *inode, data)?;
        write_inode_inner(&self.fs, self.ino, &inode)
    }

    fn stat(&self) -> Result<Stat, ()> {
        let inode = self.inode.lock();
        Ok(Stat {
            st_dev: 0, st_ino: self.ino, st_mode: inode.mode as u32,
            st_nlink: inode.links, st_uid: inode.uid, st_gid: inode.gid,
            st_rdev: 0, st_size: inode.size as i64,
            st_atime: inode.atime as i64, st_mtime: inode.mtime as i64,
            st_ctime: inode.ctime as i64,
        })
    }

    fn statfs(&self) -> Result<StatFs, ()> {
        let fs_lock = self.fs.lock();
        let sb = &fs_lock.sb;
        let total = sb.total_blocks;
        let used = sb.total_blocks; // rough: blocks minus free in bitmap (conservative)
        Ok(StatFs {
            f_type: SKYFS_MAGIC,
            f_bsize: BLOCK_SIZE as u64,
            f_blocks: total,
            f_bfree: total.saturating_sub(used),
            f_bavail: total.saturating_sub(used),
            f_files: sb.inode_count,
            f_ffree: 0,
        })
    }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir() { return Err(()); }
        let inode = self.inode.lock();
        let data = read_dir_data(&self.fs, &inode)?;
        drop(inode);
        let mut children = crate::alloc::vec![];
        dir::parse_entries(&data, |entry, name| {
            if name != "." && name != ".." {
                if let Ok(node) = SkyfsNode::new(self.fs.clone(), entry.inode) {
                    children.push(Arc::new(node) as Arc<dyn VfsNode>);
                }
            }
        });
        Ok(children)
    }

    fn find_child(&self, name: &str) -> Option<Arc<dyn VfsNode>> {
        let inode = self.inode.lock();
        let data = read_dir_data(&self.fs, &inode).ok()?;
        drop(inode);
        let mut found = None;
        dir::parse_entries(&data, |entry, entry_name| {
            if entry_name == name && found.is_none() {
                if let Ok(node) = SkyfsNode::new(self.fs.clone(), entry.inode) {
                    found = Some(Arc::new(node) as Arc<dyn VfsNode>);
                }
            }
        });
        found
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let new_ino = inode::alloc_inode_inner(&self.fs)?;
        inode::init_inode_inner(&self.fs, new_ino, 0o100644)?;
        add_entry_impl(&self.fs, self.ino, name, new_ino)?;
        SkyfsNode::new(self.fs.clone(), new_ino).map(|n| Arc::new(n) as Arc<dyn VfsNode>)
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let new_ino = inode::alloc_inode_inner(&self.fs)?;
        inode::init_inode_inner(&self.fs, new_ino, 0o040755)?;
        add_entry_impl(&self.fs, self.ino, name, new_ino)?;
        let dot = DirEntry { inode: new_ino, rec_len: 12, name_len: 1, file_type: 0 };
        let dot_dot = DirEntry { inode: self.ino, rec_len: 12, name_len: 2, file_type: 0 };
        let mut buf = [0u8; MAX_INLINE_DATA];
        let mut off = 0usize;
        write_dirent(&mut buf, &mut off, &dot, b".");
        write_dirent(&mut buf, &mut off, &dot_dot, b"..");
        let mut new_inode = read_inode_inner(&self.fs, new_ino)?;
        new_inode.data[..off].copy_from_slice(&buf[..off]);
        new_inode.size = off as u64;
        write_inode_inner(&self.fs, new_ino, &new_inode)?;
        SkyfsNode::new(self.fs.clone(), new_ino).map(|n| Arc::new(n) as Arc<dyn VfsNode>)
    }

    fn unlink(&self, name: &str) -> Result<(), ()> {
        let inode = self.inode.lock();
        let data = read_dir_data(&self.fs, &inode)?;
        drop(inode);
        let mut new_data = Vec::with_capacity(data.len());
        let mut found = false;
        dir::parse_entries(&data, |entry, entry_name| {
            if entry_name == name { found = true; }
            else { write_dirent_append(&mut new_data, &entry, entry_name.as_bytes()); }
        });
        if !found { return Err(()); }
        let mut inode = self.inode.lock();
        store_data_blocks(&self.fs, &mut *inode, &new_data)?;
        write_inode_inner(&self.fs, self.ino, &inode)
    }

    fn ioctl(&self, _request: u64, _argp: *mut u8) -> Result<u64, ()> { Err(()) }
}

fn write_dirent_append(buf: &mut Vec<u8>, entry: &DirEntry, name: &[u8]) {
    let entry_bytes = unsafe {
        core::slice::from_raw_parts(entry as *const DirEntry as *const u8, core::mem::size_of::<DirEntry>())
    };
    buf.extend_from_slice(entry_bytes);
    buf.extend_from_slice(name);
    while buf.len() % 4 != 0 { buf.push(0); }
}

fn write_dirent(buf: &mut [u8], off: &mut usize, entry: &DirEntry, name: &[u8]) {
    let entry_bytes = unsafe {
        core::slice::from_raw_parts(entry as *const DirEntry as *const u8, core::mem::size_of::<DirEntry>())
    };
    buf[*off..*off + entry_bytes.len()].copy_from_slice(entry_bytes);
    *off += entry_bytes.len();
    buf[*off..*off + name.len()].copy_from_slice(name);
    *off += name.len();
    while *off % 4 != 0 { *off += 1; }
}
